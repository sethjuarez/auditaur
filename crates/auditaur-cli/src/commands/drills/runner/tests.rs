use std::{cell::RefCell, collections::HashMap, fs, rc::Rc, thread};

use anyhow::anyhow;
use serde_json::json;

use super::*;
use crate::commands::drills::{
    load,
    model::DrillSuite,
    run::{read_attempt, read_run, Invocation, RunOptions, RunReport},
    select, Filters,
};

/// What the fakes saw, in order.
#[derive(Default)]
struct Shared {
    log: Vec<String>,
    /// Calls per selector, for flaky steps.
    calls: HashMap<String, u32>,
    dead: bool,
}

type State = Rc<RefCell<Shared>>;

/// Behaviour comes from the step's selector:
/// `#fail` fails, `#flaky` fails on its first call, `#slow` runs out the clock,
/// `#die` kills the app, `#shot` attaches a file, anything else passes.
struct FakeDriver {
    state: State,
}

impl AppDriver for FakeDriver {
    fn app_info(&self) -> Value {
        json!({ "pid": 42 })
    }

    fn run_step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepResult {
        let target = step
            .spec
            .get(step.action)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut state = self.state.borrow_mut();
        state.log.push(step.title.clone());
        let calls = {
            let count = state.calls.entry(target.clone()).or_default();
            *count += 1;
            *count
        };
        match target.as_str() {
            "#fail" => StepResult::Failed(ErrorInfo::new("boom")),
            "#flaky" if calls == 1 => StepResult::Failed(ErrorInfo::new("flake")),
            "#slow" => {
                drop(state);
                thread::sleep(ctx.remaining());
                StepResult::TimedOut(ErrorInfo::new("slow"))
            }
            "#wait80" => {
                drop(state);
                thread::sleep(std::time::Duration::from_millis(80).min(ctx.remaining()));
                if ctx.expired() {
                    StepResult::TimedOut(ErrorInfo::new("too slow"))
                } else {
                    StepResult::Passed
                }
            }
            "#die" => {
                state.dead = true;
                StepResult::Failed(ErrorInfo::new("connection lost"))
            }
            "#quietdie" => {
                state.dead = true;
                StepResult::Passed
            }
            "#shot" => {
                ctx.writer
                    .attach("shot", "image/png", "shot.png", b"png")
                    .unwrap();
                StepResult::Passed
            }
            _ => StepResult::Passed,
        }
    }

    fn capture_failure(&mut self, ctx: &mut StepCtx<'_>) {
        self.state.borrow_mut().log.push("capture".to_string());
        ctx.writer
            .attach("failure", "text/plain", "failure.txt", b"evidence")
            .unwrap();
    }

    fn exited(&mut self) -> Option<String> {
        self.state.borrow().dead.then(|| "crashed".to_string())
    }

    fn close(&mut self) -> Result<()> {
        self.state.borrow_mut().log.push("close".to_string());
        Ok(())
    }
}

/// Hooks named `bad` fail; suites named `nolaunch` cannot launch.
struct FakeHost {
    state: State,
}

impl Host for FakeHost {
    fn launch(&mut self, suite: &DrillSuite) -> Result<Box<dyn AppDriver>> {
        self.state.borrow_mut().log.push("launch".to_string());
        if suite.suite == "nolaunch" {
            return Err(anyhow!("no app"));
        }
        Ok(Box::new(FakeDriver {
            state: self.state.clone(),
        }))
    }

    fn run_hook(&mut self, hook: &Value, _teardown: bool) -> Result<HookResult> {
        let name = hook["name"].as_str().unwrap().to_string();
        self.state.borrow_mut().log.push(format!("hook {name}"));
        Ok(HookResult {
            passed: name != "bad",
            timed_out: false,
            report: json!({ "name": name, "env": { "token": "secret-value" } }),
        })
    }
}

struct Ran {
    report: RunReport,
    log: Vec<String>,
    dir: std::path::PathBuf,
    _temp: tempfile::TempDir,
}

fn run_with(suite: &str, options: RunnerOptions) -> Ran {
    let temp = tempfile::tempdir().unwrap();
    let drills = temp.path().join(".auditaur/drills");
    fs::create_dir_all(&drills).unwrap();
    let file = drills.join("s.drill.json");
    fs::write(&file, suite).unwrap();
    let loaded = load(&[file]);
    assert!(loaded.invalid.is_empty(), "{:?}", loaded.invalid);
    let plan = select(&loaded.suites, &Filters::default());
    let mut writer = RunWriter::create(
        RunOptions {
            workspace: temp.path().to_path_buf(),
            invocation: Invocation::default(),
            keep: 5,
        },
        &plan,
    )
    .unwrap();
    let dir = writer.dir().to_path_buf();
    let state = State::default();
    let mut host = FakeHost {
        state: state.clone(),
    };
    execute(&mut writer, &plan, &mut host, &mut NoReporter, &options).unwrap();
    let report = writer.finish(false).unwrap();
    // The persisted report matches what the writer returned.
    assert_eq!(read_run(&dir).unwrap().totals, report.totals);
    let log = state.borrow().log.clone();
    Ran {
        report,
        log,
        dir,
        _temp: temp,
    }
}

fn run(suite: &str) -> Ran {
    run_with(suite, RunnerOptions::default())
}

fn test<'a>(ran: &'a Ran, name: &str) -> &'a crate::commands::drills::run::TestReport {
    ran.report.suites[0]
        .tests
        .iter()
        .find(|test| test.name == name)
        .unwrap()
}

fn steps(ran: &Ran, name: &str, attempt: usize) -> Vec<(String, Status)> {
    let summary = &test(ran, name).attempts[attempt];
    read_attempt(&ran.dir, summary)
        .unwrap()
        .steps
        .into_iter()
        .map(|step| (step.title, step.status))
        .collect()
}

#[test]
fn passing_and_failing_tests_record_outcomes() {
    let ran = run(r##"{ "tests": [
        { "name": "good", "steps": [{ "click": "#a" }, { "click": "#b" }] },
        { "name": "bad", "steps": [{ "click": "#fail" }, { "click": "#never" }] }
    ] }"##);
    assert_eq!(test(&ran, "good").status, Status::Passed);
    assert_eq!(test(&ran, "good").outcome, Some(Outcome::Expected));
    let bad = test(&ran, "bad");
    assert_eq!(bad.status, Status::Failed);
    assert_eq!(bad.outcome, Some(Outcome::Unexpected));
    assert_eq!(bad.attempts[0].error.as_ref().unwrap().message, "boom");
    assert_eq!(bad.attempts[0].error.as_ref().unwrap().step, Some(0));
    // A hard failure stops the test; later steps never run.
    assert!(!ran.log.iter().any(|entry| entry.contains("#never")));
    assert_eq!(
        steps(&ran, "bad", 0),
        vec![("click #fail".to_string(), Status::Failed)]
    );
    assert_eq!(ran.report.totals.passed, 1);
    assert_eq!(ran.report.totals.failed, 1);
    assert_eq!(ran.report.status, Status::Failed);
    assert_eq!(ran.report.suites[0].app, Some(json!({ "pid": 42 })));
    // Launch and close are recorded as suite steps.
    let hooks: Vec<_> = ran.report.suites[0]
        .hooks
        .iter()
        .map(|step| step.action.as_str())
        .collect();
    assert_eq!(hooks, vec!["launch", "close"]);
}

#[test]
fn failures_capture_evidence() {
    let ran = run(r##"{ "tests": [{ "name": "bad", "steps": [{ "click": "#fail" }] }] }"##);
    let summary = &test(&ran, "bad").attempts[0];
    let attempt = read_attempt(&ran.dir, summary).unwrap();
    let paths: Vec<_> = attempt.steps[0]
        .attachments
        .iter()
        .map(|a| a.path.clone())
        .collect();
    assert_eq!(paths.len(), 1, "{paths:?}");
    assert!(paths[0].ends_with("failure.txt"));
    assert!(ran.dir.join(&paths[0]).is_file());
}

#[test]
fn step_attachments_are_recorded() {
    let ran = run(r##"{ "tests": [{ "name": "shot", "steps": [{ "click": "#shot" }] }] }"##);
    let summary = &test(&ran, "shot").attempts[0];
    let attempt = read_attempt(&ran.dir, summary).unwrap();
    assert_eq!(attempt.steps[0].attachments.len(), 1);
    assert_eq!(test(&ran, "shot").status, Status::Passed);
}

#[test]
fn soft_failures_continue_but_fail_the_test() {
    let ran = run(r##"{ "tests": [{ "name": "soft", "steps": [
        { "expect": "#fail", "toBeVisible": true, "soft": true },
        { "click": "#after" }
    ] }] }"##);
    let soft = test(&ran, "soft");
    assert_eq!(soft.status, Status::Failed);
    assert!(ran.log.iter().any(|entry| entry.contains("#after")));
    // Soft failures do not capture evidence.
    assert!(!ran.log.contains(&"capture".to_string()));
    let summary = &soft.attempts[0];
    let attempt = read_attempt(&ran.dir, summary).unwrap();
    assert!(attempt.steps[0].soft);
    assert_eq!(attempt.steps[0].status, Status::Failed);
    assert_eq!(attempt.steps[1].status, Status::Passed);
}

#[test]
fn hooks_wrap_each_attempt_and_after_each_always_runs() {
    let ran = run(r##"{
        "beforeAll": [{ "click": "#ba" }],
        "beforeEach": [{ "click": "#be" }],
        "afterEach": [{ "click": "#ae" }],
        "afterAll": [{ "click": "#aa" }],
        "tests": [
            { "name": "one", "steps": [{ "click": "#fail" }] },
            { "name": "two", "steps": [{ "click": "#t2" }] }
        ]
    }"##);
    assert_eq!(
        ran.log,
        vec![
            "launch",
            "click #ba",
            "click #be",
            "click #fail",
            "capture",
            "click #ae",
            "click #be",
            "click #t2",
            "click #ae",
            "click #aa",
            "close"
        ]
    );
    assert_eq!(
        steps(&ran, "one", 0),
        vec![
            ("click #be".to_string(), Status::Passed),
            ("click #fail".to_string(), Status::Failed),
            ("click #ae".to_string(), Status::Passed),
        ]
    );
    assert_eq!(test(&ran, "two").status, Status::Passed);
    assert!(ran.report.suites[0].error.is_none());
}

#[test]
fn before_each_failure_skips_steps_but_not_after_each() {
    let ran = run(r##"{
        "beforeEach": [{ "click": "#fail" }],
        "afterEach": [{ "click": "#ae" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#body" }] }]
    }"##);
    assert!(!ran.log.iter().any(|entry| entry.contains("#body")));
    assert!(ran.log.iter().any(|entry| entry.contains("#ae")));
    assert_eq!(test(&ran, "t").status, Status::Failed);
}

#[test]
fn retries_turn_a_recovered_failure_into_flaky() {
    let ran = run(
        r##"{ "tests": [{ "name": "flaky", "retries": 2, "steps": [{ "click": "#flaky" }] }] }"##,
    );
    let flaky = test(&ran, "flaky");
    assert_eq!(flaky.outcome, Some(Outcome::Flaky));
    assert_eq!(flaky.attempts.len(), 2);
    assert_eq!(flaky.attempts[0].status, Status::Failed);
    assert_eq!(flaky.attempts[1].status, Status::Passed);
    assert_eq!(ran.report.totals.flaky, 1);
}

#[test]
fn retries_fall_back_to_suite_then_cli() {
    let suite = r##"{ "tests": [{ "name": "f", "steps": [{ "click": "#fail" }] }] }"##;
    assert_eq!(run(suite).report.suites[0].tests[0].attempts.len(), 1);
    let cli = run_with(suite, RunnerOptions { retries: Some(2) });
    assert_eq!(cli.report.suites[0].tests[0].attempts.len(), 3);
    let suite_level = run_with(
        r##"{ "retries": 1, "tests": [{ "name": "f", "steps": [{ "click": "#fail" }] }] }"##,
        RunnerOptions { retries: Some(3) },
    );
    assert_eq!(suite_level.report.suites[0].tests[0].attempts.len(), 2);
}

#[test]
fn expect_fail_inverts_the_outcome_and_does_not_retry() {
    let ran = run(r##"{ "retries": 2, "tests": [
        { "name": "known", "expectFail": "bug", "steps": [{ "click": "#fail" }] },
        { "name": "fixed", "expectFail": true, "steps": [{ "click": "#ok" }] }
    ] }"##);
    let known = test(&ran, "known");
    assert_eq!(known.outcome, Some(Outcome::Expected));
    assert_eq!(known.attempts.len(), 1);
    let fixed = test(&ran, "fixed");
    assert_eq!(fixed.outcome, Some(Outcome::Unexpected));
    assert_eq!(fixed.attempts.len(), 3);
}

#[test]
fn skip_and_fixme_never_run() {
    let ran = run(r##"{ "tests": [
        { "name": "s", "skip": true, "steps": [{ "click": "#s" }] },
        { "name": "f", "fixme": "later", "steps": [{ "click": "#f" }] },
        { "name": "r", "steps": [{ "click": "#r" }] }
    ] }"##);
    assert_eq!(test(&ran, "s").outcome, Some(Outcome::Skipped));
    assert_eq!(test(&ran, "f").outcome, Some(Outcome::Skipped));
    assert!(test(&ran, "s").attempts.is_empty());
    assert_eq!(ran.report.totals.skipped, 2);
    assert_eq!(ran.report.totals.passed, 1);
    assert!(!ran.log.iter().any(|e| e == "click #s" || e == "click #f"));
}

#[test]
fn test_timeout_times_out_but_step_timeout_fails() {
    let ran = run(r##"{ "tests": [
        { "name": "budget", "timeoutMs": 50, "steps": [{ "click": "#slow" }, { "click": "#never" }] },
        { "name": "step", "timeoutMs": 5000, "steps": [{ "click": "#slow", "timeoutMs": 30 }] }
    ] }"##);
    assert_eq!(test(&ran, "budget").status, Status::TimedOut);
    assert_eq!(test(&ran, "step").status, Status::Failed);
    assert!(!ran.log.iter().any(|entry| entry.contains("#never")));
}

#[test]
fn slow_tests_get_triple_the_timeout() {
    let ran = run(r##"{ "timeoutMs": 50, "tests": [
        { "name": "slow", "slow": true, "steps": [{ "click": "#wait80" }] },
        { "name": "normal", "steps": [{ "click": "#wait80" }] }
    ] }"##);
    assert_eq!(test(&ran, "slow").status, Status::Passed);
    assert_eq!(test(&ran, "normal").status, Status::TimedOut);
}

#[test]
fn launch_failure_leaves_tests_not_run() {
    let ran = run(
        r##"{ "suite": "nolaunch", "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }] }"##,
    );
    let suite = &ran.report.suites[0];
    assert!(suite
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("app launch failed: no app"));
    assert_eq!(suite.tests[0].status, Status::Pending);
    assert_eq!(ran.report.totals.not_run, 1);
    assert_eq!(ran.report.status, Status::Failed);
    assert!(!ran.log.contains(&"close".to_string()));
}

#[test]
fn before_all_failure_fails_the_suite_and_skips_tests() {
    let ran = run(r##"{
        "beforeAll": [{ "click": "#fail" }],
        "afterAll": [{ "click": "#aa" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }]
    }"##);
    assert!(ran.report.suites[0].error.is_some());
    assert_eq!(ran.report.totals.not_run, 1);
    assert!(ran.log.contains(&"click #aa".to_string()));
    assert!(ran.log.contains(&"close".to_string()));
}

#[test]
fn app_exit_stops_the_suite() {
    let ran = run(r##"{
        "afterEach": [{ "click": "#ae" }],
        "afterAll": [{ "click": "#aa" }],
        "retries": 2,
        "tests": [
            { "name": "dies", "steps": [{ "click": "#die" }] },
            { "name": "after", "steps": [{ "click": "#a" }] }
        ]
    }"##);
    let dies = test(&ran, "dies");
    assert_eq!(dies.status, Status::Failed);
    assert_eq!(dies.attempts.len(), 1, "no retry once the app is gone");
    assert!(dies.attempts[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("app exited: crashed"));
    assert_eq!(test(&ran, "after").status, Status::Pending);
    assert!(!ran.log.contains(&"capture".to_string()));
    assert!(!ran.log.contains(&"click #ae".to_string()));
    assert!(!ran.log.contains(&"click #aa".to_string()));
    assert!(ran.log.contains(&"close".to_string()));
    assert!(ran.report.suites[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("crashed"));
}

#[test]
fn crash_after_a_passing_last_step_fails_the_test() {
    let ran = run(r##"{
        "afterAll": [{ "click": "#aa" }],
        "tests": [
            { "name": "quiet", "steps": [{ "click": "#quietdie" }] },
            { "name": "after", "steps": [{ "click": "#a" }] }
        ]
    }"##);
    let quiet = test(&ran, "quiet");
    assert_eq!(quiet.status, Status::Failed);
    assert!(quiet.attempts[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("app exited: crashed"));
    assert_eq!(test(&ran, "after").status, Status::Pending);
    assert!(!ran.log.contains(&"click #aa".to_string()));
    assert!(ran.report.suites[0].error.is_some());
}

#[test]
fn crash_in_before_all_is_reported_even_when_steps_pass() {
    let ran = run(r##"{
        "beforeAll": [{ "click": "#quietdie" }],
        "tests": [{ "name": "skipped", "skip": true, "steps": [{ "click": "#a" }] }]
    }"##);
    assert!(ran.report.suites[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("crashed"));
}

#[test]
fn setup_hook_failure_skips_launch_and_teardown_respects_always() {
    let ran = run(r##"{
        "setup": [{ "name": "bad", "run": "x" }, { "name": "later", "run": "x" }],
        "teardown": [
            { "name": "bad", "run": "x" },
            { "name": "plain", "run": "x" },
            { "name": "cleanup", "run": "x", "always": true }
        ],
        "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }]
    }"##);
    assert_eq!(
        ran.log,
        vec!["hook bad", "hook bad", "hook cleanup"],
        "setup stops at the first failure, launch never happens, teardown skips non-always hooks"
    );
    let suite = &ran.report.suites[0];
    assert!(suite
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("setup hook `bad` failed"));
    let statuses: Vec<_> = suite
        .hooks
        .iter()
        .map(|step| (step.title.as_str(), step.status))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("setup: bad", Status::Failed),
            ("teardown: bad", Status::Failed),
            ("teardown: plain", Status::Skipped),
            ("teardown: cleanup", Status::Passed),
        ]
    );
    assert_eq!(ran.report.totals.not_run, 1);
}

#[test]
fn hook_output_is_redacted() {
    let ran = run(r##"{
        "setup": [{ "name": "seed", "run": "x" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }]
    }"##);
    let hook = &ran.report.suites[0].hooks[0];
    assert_eq!(hook.status, Status::Passed);
    let written = fs::read_to_string(ran.dir.join(&hook.attachments[0].path)).unwrap();
    assert!(written.contains("seed"));
    assert!(!written.contains("secret-value"), "{written}");
}
