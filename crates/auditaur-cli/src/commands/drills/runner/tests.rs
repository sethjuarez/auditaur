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
    /// `(since, live)` per telemetry request.
    telemetry: Vec<(i64, bool)>,
    /// `since` seen by each step, in order.
    since: Vec<i64>,
    /// Flipped by a `#cancel` step, like Ctrl+C.
    cancel: Option<Arc<AtomicBool>>,
}

type State = Rc<RefCell<Shared>>;

/// Behaviour comes from the step's selector:
/// `#fail` fails, `#flaky` fails on its first call, `#slow` runs out the clock,
/// `#die` kills the app, `#shot` attaches a file, `#newbaseline` writes a missing baseline,
/// anything else passes.
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
        if let Some(snapshot) = &ctx.snapshot {
            let file = snapshot.baseline.file_name().unwrap().to_string_lossy();
            let dir = snapshot.baseline.parent().unwrap();
            assert!(dir.ends_with("__snapshots__/s"), "{}", dir.display());
            state.log.push(format!("snapshot {} {file}", snapshot.name));
        }
        state.since.push(ctx.since_unix_nanos);
        if matches!(step.kind, StepKind::Gate(_)) {
            // A person taking longer than the test's whole timeout to answer.
            drop(state);
            thread::sleep(std::time::Duration::from_millis(150));
            let mut state = self.state.borrow_mut();
            let unbounded = ctx.remaining() > std::time::Duration::from_secs(3600);
            state.log.push(format!("gate unbounded={unbounded}"));
            return StepResult::Passed;
        }
        let calls = {
            let count = state.calls.entry(target.clone()).or_default();
            *count += 1;
            *count
        };
        match target.as_str() {
            "#fail" => StepResult::Failed(ErrorInfo::new("boom")),
            "#skip" => StepResult::Skipped,
            "#newbaseline" => {
                ctx.wrote_baseline = true;
                StepResult::Failed(ErrorInfo::new("wrote baseline"))
            }
            "#flaky" if calls == 1 => StepResult::Failed(ErrorInfo::new("flake")),
            "#slow" => {
                drop(state);
                thread::sleep(ctx.remaining());
                StepResult::TimedOut(ErrorInfo::new("slow"))
            }
            "#wait200" => {
                drop(state);
                thread::sleep(std::time::Duration::from_millis(200).min(ctx.remaining()));
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
            "#cancel" => {
                state
                    .cancel
                    .as_ref()
                    .expect("pass a cancel flag")
                    .store(true, Ordering::SeqCst);
                StepResult::Passed
            }
            "#shot" => {
                ctx.writer
                    .attach("shot", "image/png", "shot.png", b"png")
                    .unwrap();
                StepResult::Passed
            }
            "#window" => {
                state
                    .log
                    .push(format!("window {}", ctx.expect_timeout.as_millis()));
                if ctx.deadline < Instant::now() + std::time::Duration::from_secs(10) {
                    state.log.push("short deadline".to_string());
                }
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

    fn capture_step(&mut self, ctx: &mut StepCtx<'_>) {
        // Slower than the timeout in `trace_capture_does_not_count_against_the_timeout`.
        thread::sleep(std::time::Duration::from_millis(80));
        self.state
            .borrow_mut()
            .log
            .push(format!("trace {}", ctx.step_index));
    }

    fn exited(&mut self) -> Option<String> {
        self.state.borrow().dead.then(|| "crashed".to_string())
    }

    /// Only tests that ran a `#tel` step have telemetry.
    fn telemetry(&mut self, since_unix_nanos: i64, live: bool) -> Option<Value> {
        let mut state = self.state.borrow_mut();
        state.telemetry.push((since_unix_nanos, live));
        state.calls.contains_key("#tel").then(|| {
            json!({ "since": since_unix_nanos, "live": live, "logs": [{ "token": "secret-value" }] })
        })
    }

    fn close(&mut self) -> Result<()> {
        self.state.borrow_mut().log.push("close".to_string());
        Ok(())
    }
}

/// Hooks named `bad` fail and `cancel` cancels the run; suites named `nolaunch` cannot launch.
struct FakeHost {
    state: State,
}

impl Host for FakeHost {
    fn launch(&mut self, suite: &DrillSuite) -> Result<Box<dyn AppDriver>> {
        self.state.borrow_mut().log.push("launch".to_string());
        if suite.suite == "nolaunch" {
            return Err(anyhow!("no app"));
        }
        if suite.suite == "cancelledlaunch" {
            let state = self.state.borrow();
            state.cancel.as_ref().unwrap().store(true, Ordering::SeqCst);
            return Err(anyhow!("cancelled while waiting for the app"));
        }
        Ok(Box::new(FakeDriver {
            state: self.state.clone(),
        }))
    }

    fn run_hook(&mut self, hook: &Value, _teardown: bool) -> Result<HookResult> {
        let name = hook["name"].as_str().unwrap().to_string();
        let mut state = self.state.borrow_mut();
        state.log.push(format!("hook {name}"));
        if name == "cancel" {
            state.cancel.as_ref().unwrap().store(true, Ordering::SeqCst);
        }
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
    telemetry: Vec<(i64, bool)>,
    since: Vec<i64>,
    dir: std::path::PathBuf,
    stopped: Option<Stopped>,
    /// `(title, error messages)` per reported test attempt.
    reported: Vec<(String, Vec<String>)>,
    _temp: tempfile::TempDir,
}

#[derive(Default)]
struct Recorder {
    reported: Vec<(String, Vec<String>)>,
}

impl Reporter for Recorder {
    fn test_finished(&mut self, done: &TestDone<'_>) {
        let errors = done
            .error
            .into_iter()
            .chain(done.other_errors)
            .map(|error| error.message.clone())
            .collect();
        self.reported.push((done.title.clone(), errors));
    }
}

fn run_with(suite: &str, options: RunnerOptions) -> Ran {
    run_files(&[suite], options)
}

/// Runs each suite from its own file (`s`, `s1`, `s2`, ...), in order.
fn run_files(suites: &[&str], options: RunnerOptions) -> Ran {
    let temp = tempfile::tempdir().unwrap();
    let drills = temp.path().join(".auditaur/drills");
    fs::create_dir_all(&drills).unwrap();
    let files: Vec<_> = suites
        .iter()
        .enumerate()
        .map(|(index, suite)| {
            let stem = if index == 0 {
                "s".to_string()
            } else {
                format!("s{index}")
            };
            let file = drills.join(format!("{stem}.drill.json"));
            fs::write(&file, suite).unwrap();
            file
        })
        .collect();
    let loaded = load(&files);
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
    state.borrow_mut().cancel = options.cancel.clone();
    let mut host = FakeHost {
        state: state.clone(),
    };
    let mut recorder = Recorder::default();
    let stopped = execute(&mut writer, &plan, &mut host, &mut recorder, &options).unwrap();
    let report = writer.finish(stopped.is_some()).unwrap();
    // The persisted report matches what the writer returned.
    assert_eq!(read_run(&dir).unwrap().totals, report.totals);
    let log = state.borrow().log.clone();
    let telemetry = state.borrow().telemetry.clone();
    let since = state.borrow().since.clone();
    Ran {
        report,
        log,
        telemetry,
        since,
        dir,
        stopped,
        reported: recorder.reported,
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
    let cli = run_with(
        suite,
        RunnerOptions {
            retries: Some(2),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(cli.report.suites[0].tests[0].attempts.len(), 3);
    let suite_level = run_with(
        r##"{ "retries": 1, "tests": [{ "name": "f", "steps": [{ "click": "#fail" }] }] }"##,
        RunnerOptions {
            retries: Some(3),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(suite_level.report.suites[0].tests[0].attempts.len(), 2);
}

#[test]
fn trace_modes_capture_every_step_or_only_the_first_retry() {
    let suite = r##"{ "tests": [{ "name": "t", "retries": 2, "steps": [
        { "click": "#ok" }, { "click": "#flaky" }
    ] }] }"##;
    let traces = |trace| -> Vec<String> {
        run_with(
            suite,
            RunnerOptions {
                trace,
                ..RunnerOptions::default()
            },
        )
        .log
        .into_iter()
        .filter(|line| line.starts_with("trace") || line == "capture")
        .collect()
    };
    // The failing step captures failure evidence instead of a trace.
    assert_eq!(traces(TraceMode::Off), ["capture"]);
    assert_eq!(
        traces(TraceMode::On),
        ["trace 0", "capture", "trace 0", "trace 1"]
    );
    assert_eq!(
        traces(TraceMode::OnFirstRetry),
        ["capture", "trace 0", "trace 1"]
    );
}

#[test]
fn trace_capture_does_not_count_against_the_timeout() {
    let ran = run_with(
        r##"{ "tests": [{ "name": "t", "timeoutMs": 60, "steps": [
            { "click": "#ok" }, { "click": "#ok" }, { "click": "#ok" }
        ] }] }"##,
        RunnerOptions {
            trace: TraceMode::On,
            ..RunnerOptions::default()
        },
    );
    assert_eq!(test(&ran, "t").status, Status::Passed);
}

#[test]
fn gate_waits_do_not_count_against_the_timeout() {
    let ran = run(r##"{ "tests": [
        { "name": "t", "timeoutMs": 100, "steps": [
            { "gate": { "name": "g", "instructions": "wait", "manualContinue": true } },
            { "click": "#ok" }
        ] },
        { "name": "budget", "timeoutMs": 100, "steps": [
            { "gate": { "name": "g", "instructions": "wait", "manualContinue": true } },
            { "click": "#wait200" }
        ] },
        { "name": "capped", "timeoutMs": 100, "steps": [
            { "gate": { "name": "g", "instructions": "wait", "manualContinue": true }, "timeoutMs": 5000 }
        ] }
    ] }"##);
    // The gate outlasts the 100ms budget, yet the steps after it still get the whole budget
    // (and no more).
    assert_eq!(test(&ran, "t").status, Status::Passed);
    assert_eq!(test(&ran, "budget").status, Status::TimedOut);
    assert_eq!(test(&ran, "capped").status, Status::Passed);
    let gates: Vec<_> = ran
        .log
        .iter()
        .filter(|e| e.starts_with("gate unbounded"))
        .cloned()
        .collect();
    assert_eq!(
        gates,
        [
            "gate unbounded=true",
            "gate unbounded=true",
            "gate unbounded=false"
        ]
    );
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
fn a_skipping_step_skips_the_rest_of_the_test_without_retrying() {
    let ran = run(r##"{
        "retries": 2,
        "beforeEach": [{ "click": "#be" }],
        "afterEach": [{ "click": "#ae" }],
        "tests": [
            { "name": "body", "steps": [{ "click": "#skip" }, { "click": "#never" }] },
            { "name": "known", "expectFail": true, "steps": [{ "click": "#skip" }] }
        ]
    }"##);
    let body = test(&ran, "body");
    assert_eq!(body.status, Status::Skipped);
    assert_eq!(body.outcome, Some(Outcome::Skipped));
    assert_eq!(body.attempts.len(), 1);
    assert!(body.attempts[0].error.is_none());
    assert_eq!(
        steps(&ran, "body", 0),
        vec![
            ("click #be".to_string(), Status::Passed),
            ("click #skip".to_string(), Status::Skipped),
            ("click #ae".to_string(), Status::Passed),
        ]
    );
    assert!(!ran.log.iter().any(|entry| entry.contains("#never")));
    assert_eq!(test(&ran, "known").outcome, Some(Outcome::Skipped));
    assert_eq!(ran.report.totals.skipped, 2);

    // Skipping in beforeEach skips the body.
    let ran = run(r##"{
        "beforeEach": [{ "click": "#skip" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#body" }] }]
    }"##);
    assert!(!ran.log.iter().any(|entry| entry.contains("#body")));
    assert_eq!(test(&ran, "t").status, Status::Skipped);

    // Skipping in beforeAll skips every test; afterAll still runs.
    let ran = run(r##"{
        "beforeAll": [{ "click": "#skip" }, { "click": "#never" }],
        "afterAll": [{ "click": "#aa" }],
        "tests": [
            { "name": "a", "steps": [{ "click": "#a" }] },
            { "name": "b", "steps": [{ "click": "#b" }] }
        ]
    }"##);
    assert!(!ran
        .log
        .iter()
        .any(|e| e.contains("#never") || e == "click #a" || e == "click #b"));
    assert!(ran.log.iter().any(|e| e == "click #aa"));
    assert_eq!(test(&ran, "a").outcome, Some(Outcome::Skipped));
    assert_eq!(test(&ran, "b").outcome, Some(Outcome::Skipped));
    assert_eq!(ran.report.totals.skipped, 2);
    assert!(ran.report.suites[0].error.is_none());
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
    let ran = run(r##"{ "timeoutMs": 100, "tests": [
        { "name": "slow", "slow": true, "steps": [{ "click": "#wait200" }] },
        { "name": "normal", "steps": [{ "click": "#wait200" }] }
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

#[test]
fn each_attempt_saves_its_telemetry_slice() {
    let ran = run(r##"{
        "beforeAll": [{ "click": "#setup" }],
        "tests": [
            { "name": "tel", "retries": 1, "steps": [{ "click": "#tel" }, { "click": "#flaky" }] },
            { "name": "dies", "steps": [{ "click": "#quietdie" }] }
        ]
    }"##);
    let tel = test(&ran, "tel");
    assert_eq!(tel.attempts.len(), 2);
    for summary in &tel.attempts {
        let attempt = read_attempt(&ran.dir, summary).unwrap();
        assert_eq!(attempt.attachments.len(), 1, "{:?}", attempt.attachments);
        let saved = &attempt.attachments[0];
        assert_eq!(saved.name, "telemetry");
        let written = fs::read_to_string(ran.dir.join(&saved.path)).unwrap();
        assert!(!written.contains("secret-value"), "{written}");
    }
    // Each attempt reads from its own start, after the suite's; steps see the same instant.
    let (first, second) = (ran.telemetry[0].0, ran.telemetry[1].0);
    assert!(
        ran.since[0] <= first && first <= second,
        "{:?} {:?}",
        ran.since,
        ran.telemetry
    );
    assert_eq!(&ran.since[1..3], &[first, first]);
    assert_eq!(&ran.since[3..5], &[second, second]);
    assert!(first % 1_000_000 == 0, "floored to the millisecond");
    // Once the app has gone, the slice is read without a live flush.
    assert_eq!(ran.telemetry.last(), Some(&(ran.telemetry[2].0, false)));
    assert!(ran.telemetry[..2].iter().all(|(_, live)| *live));
}

#[test]
fn timeout_on_an_assertion_is_its_retry_window() {
    let ran = run(r##"{ "tests": [{ "name": "t", "steps": [
        { "expect": "#window", "toBeVisible": true, "timeoutMs": 50 },
        { "click": "#window", "timeoutMs": 50 },
        { "expect": "#window", "toBeVisible": true }
    ] }] }"##);
    let windows: Vec<_> = ran
        .log
        .iter()
        .filter(|e| e.starts_with("window") || e.as_str() == "short deadline")
        .cloned()
        .collect();
    assert_eq!(
        windows,
        vec!["window 50", "window 5000", "short deadline", "window 5000"]
    );
}

#[test]
fn snapshots_resolve_beside_the_suite_and_number_per_attempt() {
    let ran = run(r##"{ "suite": "s", "retries": 1,
            "beforeAll": [{ "expect": "main", "toMatchAriaSnapshot": true }],
            "beforeEach": [{ "expect": "main", "toMatchAriaSnapshot": true }],
            "tests": [{ "name": "Main view!", "steps": [
                { "expect": "main", "toMatchAriaSnapshot": true },
                { "expect": "body", "toHaveScreenshot": true },
                { "expect": "nav", "toMatchAriaSnapshot": "menu" },
                { "click": "#flaky" }
            ] }] }"##);
    let os = std::env::consts::OS;
    let snapshots: Vec<String> = ran
        .log
        .iter()
        .filter(|line| line.starts_with("snapshot "))
        .cloned()
        .collect();
    let attempt = [
        "snapshot main-view-1 main-view-1.aria.yml".to_string(),
        "snapshot main-view-2 main-view-2.aria.yml".to_string(),
        format!("snapshot main-view-3 main-view-3-{os}.png"),
        "snapshot menu menu.aria.yml".to_string(),
    ];
    let mut expected = vec!["snapshot beforeall-1 beforeall-1.aria.yml".to_string()];
    // The retry starts numbering again, so it checks the same baselines.
    expected.extend(attempt.iter().cloned());
    expected.extend(attempt.iter().cloned());
    assert_eq!(snapshots, expected);
    assert_eq!(test(&ran, "Main view!").outcome, Some(Outcome::Flaky));
}

#[test]
fn writing_a_missing_baseline_is_never_retried_into_a_pass() {
    let ran = run(r##"{ "suite": "s", "retries": 2, "tests": [
            { "name": "new", "steps": [{ "click": "#newbaseline" }, { "click": "#next" }] },
            { "name": "later", "steps": [{ "click": "#flaky" }] },
            { "name": "known", "expectFail": "bug", "steps": [{ "click": "#newbaseline" }] }
        ] }"##);
    assert_eq!(test(&ran, "new").attempts.len(), 1);
    // Like a soft failure, the rest of the test still runs and records its baselines.
    assert_eq!(
        steps(&ran, "new", 0)
            .into_iter()
            .map(|(_, status)| status)
            .collect::<Vec<_>>(),
        [Status::Failed, Status::Passed]
    );
    assert_eq!(test(&ran, "new").outcome, Some(Outcome::Unexpected));
    // The flag is per test: the next test still retries.
    assert_eq!(test(&ran, "later").outcome, Some(Outcome::Flaky));
    // A new baseline needs review even when the test was expected to fail.
    assert_eq!(test(&ran, "known").outcome, Some(Outcome::Unexpected));
    assert_eq!(ran.report.totals.failed, 2);
}

fn cancel_flag() -> RunnerOptions {
    RunnerOptions {
        cancel: Some(Arc::new(AtomicBool::new(false))),
        ..RunnerOptions::default()
    }
}

#[test]
fn max_failures_stops_starting_tests_but_cleans_up() {
    let ran = run_with(
        r##"{
        "afterAll": [{ "click": "#aa" }],
        "teardown": [{ "name": "down", "run": "x" }],
        "tests": [
            { "name": "one", "steps": [{ "click": "#fail" }] },
            { "name": "two", "steps": [{ "click": "#fail" }] },
            { "name": "three", "steps": [{ "click": "#a" }] }
        ] }"##,
        RunnerOptions {
            max_failures: Some(1),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(ran.stopped, Some(Stopped::MaxFailures));
    assert_eq!(test(&ran, "one").outcome, Some(Outcome::Unexpected));
    assert_eq!(test(&ran, "two").status, Status::Pending);
    assert_eq!(test(&ran, "three").status, Status::Pending);
    assert_eq!(ran.report.totals.not_run, 2);
    assert_eq!(ran.report.status, Status::Interrupted);
    // afterAll, close and teardown still run.
    for entry in ["click #aa", "close", "hook down"] {
        assert!(
            ran.log.contains(&entry.to_string()),
            "{entry}: {:?}",
            ran.log
        );
    }
}

#[test]
fn max_failures_counts_across_suites_and_suite_errors() {
    let ran = run_files(
        &[
            r##"{ "suite": "nolaunch", "tests": [{ "name": "a", "steps": [{ "click": "#a" }] }] }"##,
            r##"{ "tests": [{ "name": "b", "steps": [{ "click": "#fail" }] }] }"##,
            r##"{ "tests": [{ "name": "c", "steps": [{ "click": "#a" }] }] }"##,
        ],
        RunnerOptions {
            max_failures: Some(2),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(ran.stopped, Some(Stopped::MaxFailures));
    assert_eq!(
        ran.report.suites[1].tests[0].outcome,
        Some(Outcome::Unexpected)
    );
    assert_eq!(ran.report.suites[2].status, Status::Pending);
    assert_eq!(ran.report.suites[2].tests[0].status, Status::Pending);
}

#[test]
fn max_failures_reached_by_the_last_test_is_not_a_stop() {
    let ran = run_with(
        r##"{ "tests": [
            { "name": "a", "steps": [{ "click": "#a" }] },
            { "name": "b", "steps": [{ "click": "#fail" }] }
        ] }"##,
        RunnerOptions {
            max_failures: Some(1),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(ran.stopped, None);
    assert_eq!(ran.report.status, Status::Failed);
}

#[test]
fn cancel_cuts_the_test_short_and_still_runs_teardown() {
    let ran = run_with(
        r##"{
        "retries": 2,
        "afterEach": [{ "click": "#ae" }],
        "afterAll": [{ "click": "#aa" }],
        "teardown": [{ "name": "down", "run": "x" }],
        "tests": [
            { "name": "one", "steps": [{ "click": "#cancel" }, { "click": "#after" }] },
            { "name": "two", "steps": [{ "click": "#a" }] }
        ] }"##,
        cancel_flag(),
    );
    assert_eq!(ran.stopped, Some(Stopped::Cancelled));
    let one = test(&ran, "one");
    assert_eq!(one.status, Status::Interrupted);
    assert_eq!(one.outcome, Some(Outcome::Unexpected));
    assert_eq!(one.attempts.len(), 1, "a cancelled test is not retried");
    assert_eq!(test(&ran, "two").status, Status::Pending);
    assert_eq!(ran.report.status, Status::Interrupted);
    for skipped in ["click #after", "click #ae", "click #aa", "click #a"] {
        assert!(!ran.log.contains(&skipped.to_string()), "{skipped} ran");
    }
    for entry in ["close", "hook down"] {
        assert!(
            ran.log.contains(&entry.to_string()),
            "{entry}: {:?}",
            ran.log
        );
    }
    assert_eq!(ran.reported[0].1, vec!["interrupted"]);
}

#[test]
fn cancel_after_the_last_step_keeps_the_pass() {
    let ran = run_with(
        r##"{ "tests": [{ "name": "one", "steps": [{ "click": "#a" }, { "click": "#cancel" }] }] }"##,
        cancel_flag(),
    );
    assert_eq!(ran.stopped, Some(Stopped::Cancelled));
    assert_eq!(test(&ran, "one").status, Status::Passed);
}

#[test]
fn reporter_sees_every_failure_in_order() {
    let ran = run(r##"{
        "afterEach": [{ "click": "#fail" }],
        "tests": [{ "name": "t", "steps": [
            { "expect": "#fail", "toBeVisible": true, "soft": true },
            { "expect": "#fail", "toBeVisible": true, "soft": true },
            { "click": "#fail" },
            { "click": "#never" }
        ] }] }"##);
    let (title, errors) = &ran.reported[0];
    assert_eq!(title, "s > t");
    // The hard failure comes first, then the soft ones and afterEach, in order.
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert!(errors.iter().all(|message| message == "boom"));
    let summary = &test(&ran, "t").attempts[0];
    assert_eq!(summary.error.as_ref().unwrap().step, Some(2));
}

#[test]
fn last_failed_picks_failed_and_unfinished_tests() {
    let ran = run_with(
        r##"{ "suite": "s", "tests": [
            { "name": "pass", "steps": [{ "click": "#a" }] },
            { "name": "fail", "steps": [{ "click": "#fail" }] },
            { "name": "flaky", "retries": 1, "steps": [{ "click": "#flaky" }] },
            { "name": "skip", "skip": true, "steps": [{ "click": "#a" }] },
            { "name": "unrun", "steps": [{ "click": "#a" }] }
        ] }"##,
        RunnerOptions {
            max_failures: Some(1),
            ..RunnerOptions::default()
        },
    );
    let workspace = ran
        .dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let ids = crate::commands::drills::test_command::last_failed_ids(workspace).unwrap();
    let mut names: Vec<_> = ran.report.suites[0]
        .tests
        .iter()
        .filter(|test| ids.contains(&test.id))
        .map(|test| test.name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, ["fail", "flaky", "skip", "unrun"]);
}

#[test]
fn junit_report_maps_outcomes() {
    let ran = run_files(
        &[
            r##"{ "suite": "s", "tests": [
                { "name": "pass <ok>", "steps": [{ "click": "#a" }] },
                { "name": "fail", "steps": [{ "click": "#fail" }] },
                { "name": "flaky", "retries": 1, "steps": [{ "click": "#flaky" }] },
                { "name": "skip", "skip": "not yet", "steps": [{ "click": "#a" }] }
            ] }"##,
            r##"{ "suite": "nolaunch", "tests": [{ "name": "never", "steps": [{ "click": "#a" }] }] }"##,
        ],
        RunnerOptions::default(),
    );
    let xml = crate::commands::drills::junit::render(&ran.report);
    assert!(
        xml.contains(
            r#"<testsuites name="auditaur drills" tests="5" failures="1" skipped="1" errors="1""#
        ),
        "{xml}"
    );
    assert!(
        xml.contains(r#"<testcase name="pass &lt;ok&gt;" classname="s""#),
        "{xml}"
    );
    assert!(
        xml.contains(
            r#"<failure message="step 1: boom" type="AssertionError">step 1: boom</failure>"#
        ),
        "{xml}"
    );
    assert!(xml.contains("flaky: passed on retry #1"), "{xml}");
    assert!(xml.contains(r#"<skipped message="not yet"/>"#), "{xml}");
    assert!(
        xml.contains(r#"<error message="app launch failed: no app" type="SuiteError"/>"#),
        "{xml}"
    );
    assert!(
        xml.contains(&format!(
            "attempt: .auditaur/drill-runs/{}/",
            ran.report.run_id
        )),
        "{xml}"
    );
}

#[test]
fn cancel_during_the_last_setup_hook_skips_launch() {
    let ran = run_with(
        r##"{
        "setup": [{ "name": "cancel", "run": "x" }],
        "teardown": [{ "name": "down", "run": "x" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }]
    }"##,
        cancel_flag(),
    );
    assert_eq!(ran.stopped, Some(Stopped::Cancelled));
    assert_eq!(ran.log, vec!["hook cancel", "hook down"]);
    assert_eq!(ran.report.totals.not_run, 1);
    assert_eq!(ran.report.status, Status::Interrupted);
}

#[test]
fn cancel_while_waiting_for_the_app_interrupts_instead_of_failing() {
    let ran = run_with(
        r##"{
        "suite": "cancelledlaunch",
        "teardown": [{ "name": "down", "run": "x" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }]
    }"##,
        cancel_flag(),
    );
    assert_eq!(ran.stopped, Some(Stopped::Cancelled));
    assert_eq!(ran.log, vec!["launch", "hook down"]);
    assert!(ran.report.suites[0].error.is_none());
    assert_eq!(ran.report.totals.not_run, 1);
    assert_eq!(ran.report.status, Status::Interrupted);
}

#[test]
fn junit_reports_a_suite_error_after_every_test_passed() {
    let ran = run(r##"{
        "afterAll": [{ "click": "#fail" }],
        "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }]
    }"##);
    assert_eq!(test(&ran, "t").outcome, Some(Outcome::Expected));
    let xml = crate::commands::drills::junit::render(&ran.report);
    assert!(
        xml.contains(r#"tests="2" failures="0" skipped="0" errors="1""#),
        "{xml}"
    );
    assert!(xml.contains(r#"<testcase name="(suite)""#), "{xml}");
    assert!(xml.contains(r#"type="SuiteError">"#), "{xml}");
}

#[test]
fn last_failed_skips_a_run_still_in_progress() {
    let ran = run(r##"{ "suite": "s", "tests": [
        { "name": "pass", "steps": [{ "click": "#a" }] },
        { "name": "fail", "steps": [{ "click": "#fail" }] }
    ] }"##);
    let workspace = ran
        .dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    // A concurrent run owned by this (live) process, with nothing finished yet.
    let loaded = load(&[workspace.join(".auditaur/drills/s.drill.json")]);
    let plan = select(&loaded.suites, &Filters::default());
    let live = RunWriter::create(
        RunOptions {
            workspace: workspace.to_path_buf(),
            invocation: Invocation::default(),
            keep: 5,
        },
        &plan,
    )
    .unwrap();
    // An unreadable folder that sorts last is passed over too.
    fs::create_dir_all(workspace.join(".auditaur/drill-runs/99991231-235959-999")).unwrap();
    let ids = crate::commands::drills::test_command::last_failed_ids(workspace).unwrap();
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&test(&ran, "fail").id));
    drop(live);
}
