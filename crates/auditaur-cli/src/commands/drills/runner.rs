//! Runs selected drill tests against an app and records them with [`RunWriter`].
//!
//! Orchestration (hooks, retries, timeouts, soft failures) is separate from app
//! control: a [`Host`] launches apps and runs command hooks, and an [`AppDriver`]
//! performs steps. Unit tests swap in fakes; [`real`] drives Tauri apps.

pub mod expect;
pub mod real;
pub mod snapshot;
pub mod telemetry;

use std::cell::Cell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use auditaur_core::redaction::redact_json;
use serde_json::Value;

use super::{
    model::{
        check_env_name, CommandStep, DrillSuite, Matcher, Step, StepKind, TelemetryExpectation,
        TestCase, TestMode, ENV_FILE_VAR,
    },
    run::{ErrorInfo, Outcome, Phase, RunWriter, Status},
    title, Selected,
};
use snapshot::{SnapshotKind, SnapshotTarget, UpdateMode};

pub const DEFAULT_TEST_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_EXPECT_TIMEOUT_MS: u64 = 5_000;
/// `slow` tests get this many times the normal timeout, as in Playwright.
const SLOW_MULTIPLIER: u64 = 3;

/// Everything a driver needs while running one step.
pub struct StepCtx<'a> {
    pub writer: &'a mut RunWriter,
    /// The step must finish by this instant.
    pub deadline: Instant,
    /// Auto-retrying assertions give up after this long (bounded by `deadline`).
    pub expect_timeout: Duration,
    pub test_id: Option<String>,
    pub step_index: usize,
    /// Telemetry assertions see what the app recorded from this instant (Unix nanos) on.
    pub since_unix_nanos: i64,
    /// The baseline a snapshot assertion checks.
    pub snapshot: Option<SnapshotTarget>,
    /// Set by the driver when it created a missing baseline. The failure is soft, and the test
    /// is not retried: a retry would compare the app against itself.
    pub wrote_baseline: bool,
    /// Set by Ctrl+C or a canvas cancel; long waits (gates) give up when it is.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl StepCtx<'_> {
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepResult {
    Passed,
    Failed(ErrorInfo),
    /// The step ran out of time.
    TimedOut(ErrorInfo),
    /// The step skipped the rest of the test (a gate's `skip` choice), like `test.skip()`.
    Skipped,
}

pub trait AppDriver {
    /// Session details recorded on the suite.
    fn app_info(&self) -> Value;
    fn run_step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepResult;
    /// Attach evidence (screenshot, DOM) to the failing step. Best effort.
    fn capture_failure(&mut self, _ctx: &mut StepCtx<'_>) {}
    /// Attach a trace of the step that just ran (screenshot, DOM), for `--trace`. Best effort.
    fn capture_step(&mut self, _ctx: &mut StepCtx<'_>) {}
    /// What the app recorded since `since_unix_nanos`, saved with each attempt. Best effort;
    /// `live` is false once the app has exited.
    fn telemetry(&mut self, _since_unix_nanos: i64, _live: bool) -> Option<Value> {
        None
    }
    /// Why the app can no longer be driven, once it has died.
    fn exited(&mut self) -> Option<String>;
    fn close(&mut self) -> Result<()>;
}

/// Launches apps and runs suite command hooks.
pub trait Host {
    /// Launch the suite's app with `env` added to its environment.
    fn launch(
        &mut self,
        suite: &DrillSuite,
        env: &[(String, String)],
    ) -> Result<Box<dyn AppDriver>>;
    fn run_hook(
        &mut self,
        hook: &Value,
        teardown: bool,
        env: &[(String, String)],
    ) -> Result<HookResult>;
}

pub struct HookResult {
    pub passed: bool,
    pub timed_out: bool,
    /// Command, exit code and output; redacted before it is written.
    pub report: Value,
}

/// Receives progress while the run executes.
pub trait Reporter {
    fn suite_started(&mut self, _suite: &DrillSuite) {}
    fn suite_error(&mut self, _suite: &DrillSuite, _error: &ErrorInfo) {}
    fn test_finished(&mut self, _done: &TestDone<'_>) {}
}

pub struct TestDone<'a> {
    pub title: String,
    pub status: Status,
    /// `None` while another attempt follows.
    pub outcome: Option<Outcome>,
    pub attempt: u32,
    pub duration: Duration,
    /// The attempt's first failure.
    pub error: Option<&'a ErrorInfo>,
    /// Later failures, such as soft assertions after the first.
    pub other_errors: &'a [ErrorInfo],
}

#[derive(Debug, Clone, Default)]
pub struct RunnerOptions {
    /// Used when neither the test nor its suite sets `retries`.
    pub retries: Option<u32>,
    pub update_snapshots: UpdateMode,
    /// Which steps record a trace (screenshot and DOM after every step).
    pub trace: TraceMode,
    /// Stop starting tests once this many have failed (suite errors count too).
    pub max_failures: Option<usize>,
    /// Set (e.g. by Ctrl+C) to stop the run: the running test is cut short, remaining
    /// tests and `afterEach`/`afterAll` are skipped, and teardown hooks still run.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl RunnerOptions {
    pub fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::SeqCst))
    }
}

/// When steps record a trace, like Playwright's `trace` option. Failing steps always capture a
/// screenshot and DOM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum TraceMode {
    #[default]
    Off,
    /// Every step of every attempt, plus suite hooks.
    On,
    /// Only the first retry of a failed test.
    OnFirstRetry,
}

/// Why a run stopped before every selected test ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Cancelled,
    MaxFailures,
}

/// Run every selected test. Writer errors abort the run; everything else is recorded.
pub fn execute(
    writer: &mut RunWriter,
    plan: &[Selected<'_>],
    host: &mut dyn Host,
    reporter: &mut dyn Reporter,
    options: &RunnerOptions,
) -> Result<Option<Stopped>> {
    for (index, selected) in plan.iter().enumerate() {
        if let Some(stopped) = stop_reason(writer, options) {
            return Ok(Some(stopped));
        }
        let stopped = SuiteRun {
            writer: &mut *writer,
            host: &mut *host,
            reporter: &mut *reporter,
            options,
            index,
            selected,
            error: None,
            exited: None,
            auto_snapshots: 0,
            wrote_baseline: false,
            stopped: None,
            attempt: None,
            env: selected.suite.env.clone(),
            env_file: None,
        }
        .run()?;
        if stopped.is_some() {
            return Ok(stopped);
        }
    }
    Ok(options.cancelled().then_some(Stopped::Cancelled))
}

fn stop_reason(writer: &RunWriter, options: &RunnerOptions) -> Option<Stopped> {
    if options.cancelled() {
        return Some(Stopped::Cancelled);
    }
    let max = options.max_failures?;
    let report = writer.report();
    let suite_errors = report
        .suites
        .iter()
        .filter(|suite| suite.error.is_some())
        .count();
    (report.totals.failed + suite_errors >= max).then_some(Stopped::MaxFailures)
}

struct SuiteRun<'r, 'p> {
    writer: &'r mut RunWriter,
    host: &'r mut dyn Host,
    reporter: &'r mut dyn Reporter,
    options: &'r RunnerOptions,
    index: usize,
    selected: &'r Selected<'p>,
    /// First suite-level failure.
    error: Option<ErrorInfo>,
    /// Set once the app dies; nothing else can be driven.
    exited: Option<String>,
    /// Unnamed snapshots taken so far in this test (or suite hook phase).
    auto_snapshots: u32,
    /// A missing baseline was written during the current test.
    wrote_baseline: bool,
    /// Set when the suite stopped starting tests early.
    stopped: Option<Stopped>,
    /// The test attempt being run; `None` in suite hooks.
    attempt: Option<u32>,
    /// The suite's `env` plus what setup hooks wrote to [`ENV_FILE_VAR`].
    env: Vec<(String, String)>,
    /// Where setup hooks write environment; removed when the suite ends.
    env_file: Option<tempfile::TempPath>,
}

/// Failures collected while running a list of steps.
#[derive(Default)]
struct Tally {
    hard: Option<(Status, ErrorInfo)>,
    soft: Option<ErrorInfo>,
    /// Every failure, in the order it happened.
    all: Vec<ErrorInfo>,
    /// A step skipped the rest of the test.
    skipped: bool,
}

impl Tally {
    fn first_error(&self) -> Option<ErrorInfo> {
        self.hard
            .as_ref()
            .map(|(_, error)| error.clone())
            .or_else(|| self.soft.clone())
    }

    fn merge(&mut self, other: Tally) {
        if self.hard.is_none() {
            self.hard = other.hard;
        }
        if self.soft.is_none() {
            self.soft = other.soft;
        }
        self.all.extend(other.all);
        self.skipped |= other.skipped;
    }

    /// Failures other than [`Tally::first_error`].
    fn other_errors(&self) -> Vec<ErrorInfo> {
        let first = self.first_error();
        let mut skipped = false;
        self.all
            .iter()
            .filter(|error| {
                let is_first = !skipped && Some(*error) == first.as_ref();
                skipped |= is_first;
                !is_first
            })
            .cloned()
            .collect()
    }

    fn status(&self) -> Status {
        match &self.hard {
            Some((status, _)) => *status,
            None if self.soft.is_some() => Status::Failed,
            None if self.skipped => Status::Skipped,
            None => Status::Passed,
        }
    }
}

/// Where a list of steps runs and how long it may take.
struct Budget {
    phase: Phase,
    /// Pushed back by trace captures so evidence never eats the test's timeout.
    deadline: Cell<Instant>,
    timeout_ms: u64,
    test_id: Option<String>,
    since_unix_nanos: i64,
    /// Base for unnamed snapshot names.
    snapshot_base: String,
}

impl SuiteRun<'_, '_> {
    fn suite(&self) -> &DrillSuite {
        self.selected.suite
    }

    fn run(mut self) -> Result<Option<Stopped>> {
        let suite = self.selected.suite;
        self.writer.suite_started(self.index)?;
        self.reporter.suite_started(suite);

        if !suite.setup.is_empty() {
            self.env_file = Some(
                tempfile::Builder::new()
                    .prefix("auditaur-drill-env-")
                    .tempfile()?
                    .into_temp_path(),
            );
        }
        for (index, hook) in suite.setup.iter().enumerate() {
            if self.options.cancelled() {
                self.stopped = Some(Stopped::Cancelled);
                break;
            }
            if let Some(error) = self.hook_step(hook, false, index)? {
                self.error = Some(error);
                break;
            }
        }
        if self.options.cancelled() {
            self.stopped = Some(Stopped::Cancelled);
        }

        let mut driver = if self.error.is_none() && self.stopped.is_none() {
            self.launch()?
        } else {
            None
        };

        if let Some(driver) = driver.as_deref_mut() {
            self.run_suite_body(driver)?;
        }

        if let Some(mut driver) = driver.take() {
            self.close(driver.as_mut())?;
        }

        let mut teardown_failed = false;
        for (index, hook) in suite.teardown.iter().enumerate() {
            let skip = teardown_failed && !hook_always(hook);
            if let Some(error) = self.hook_step_maybe(hook, true, index, skip)? {
                teardown_failed = true;
                self.error.get_or_insert(error);
            }
        }

        if let Some(error) = &self.error {
            self.reporter.suite_error(suite, error);
        }
        self.writer.suite_finished(self.index, self.error.take())?;
        Ok(self.stopped)
    }

    fn launch(&mut self) -> Result<Option<Box<dyn AppDriver>>> {
        let index = self
            .writer
            .step_started(Phase::BeforeAll, "launch app", "launch")?;
        match self.host.launch(self.selected.suite, &self.env) {
            Ok(driver) => {
                self.writer.step_finished(Status::Passed, false, None)?;
                self.writer.set_suite_app(driver.app_info())?;
                Ok(Some(driver))
            }
            Err(_) if self.options.cancelled() => {
                // Cancelled while waiting for the app: an interruption, not a launch failure.
                self.writer
                    .step_finished(Status::Interrupted, false, None)?;
                self.stopped = Some(Stopped::Cancelled);
                Ok(None)
            }
            Err(error) => {
                let error = ErrorInfo {
                    step: Some(index),
                    ..ErrorInfo::new(format!("app launch failed: {error:#}"))
                };
                self.writer
                    .step_finished(Status::Failed, false, Some(error.clone()))?;
                self.error = Some(error);
                Ok(None)
            }
        }
    }

    fn run_suite_body(&mut self, driver: &mut dyn AppDriver) -> Result<()> {
        let suite = self.selected.suite;
        let timeout_ms = suite.timeout_ms.unwrap_or(DEFAULT_TEST_TIMEOUT_MS);
        let since = now_unix_nanos();

        self.auto_snapshots = 0;
        let tally = self.run_steps(
            driver,
            &suite.before_all,
            &Budget::new(Phase::BeforeAll, timeout_ms, None, since),
        )?;
        if let Some(error) = tally.first_error() {
            self.error = Some(error);
        }
        self.probe_exit(driver);

        // A skip in `beforeAll` skips every test in the suite, like `test.skip()` there.
        let skip_all = tally.skipped;
        if self.error.is_none() && self.exited.is_none() {
            for (index, test) in self.selected.tests.iter().enumerate() {
                if let Some(stopped) = stop_reason(self.writer, self.options) {
                    self.stopped = Some(stopped);
                    break;
                }
                if skip_all {
                    self.skip_test(index, test)?;
                    continue;
                }
                self.run_test(driver, index, test)?;
                if self.exited.is_some() {
                    break;
                }
            }
        }

        if self.exited.is_none() && !self.options.cancelled() {
            self.auto_snapshots = 0;
            let tally = self.run_steps(
                driver,
                &suite.after_all,
                &Budget::new(Phase::AfterAll, timeout_ms, None, since),
            )?;
            if let Some(error) = tally.first_error() {
                self.error.get_or_insert(error);
            }
        }
        // Catch a crash that no later bridge call would have noticed.
        self.probe_exit(driver);
        if let Some(reason) = &self.exited {
            self.error
                .get_or_insert_with(|| ErrorInfo::new(format!("app exited: {reason}")));
        }
        Ok(())
    }

    fn skip_test(&mut self, index: usize, test: &TestCase) -> Result<()> {
        self.writer.test_skipped(self.index, index)?;
        self.reporter.test_finished(&TestDone {
            title: title(self.selected.suite, test),
            status: Status::Skipped,
            outcome: Some(Outcome::Skipped),
            attempt: 0,
            duration: Duration::ZERO,
            error: None,
            other_errors: &[],
        });
        Ok(())
    }

    fn run_test(
        &mut self,
        driver: &mut dyn AppDriver,
        index: usize,
        test: &TestCase,
    ) -> Result<()> {
        let suite = self.selected.suite;
        if matches!(test.mode, TestMode::Skip | TestMode::Fixme) {
            return self.skip_test(index, test);
        }
        let retries = test
            .retries
            .or(suite.retries)
            .or(self.options.retries)
            .unwrap_or(0);
        let mut timeout_ms = test
            .timeout_ms
            .or(suite.timeout_ms)
            .unwrap_or(DEFAULT_TEST_TIMEOUT_MS);
        if test.slow {
            timeout_ms = timeout_ms.saturating_mul(SLOW_MULTIPLIER);
        }
        let expected = if test.expect_fail.is_some() {
            Status::Failed
        } else {
            Status::Passed
        };
        let id = self.writer.report().suites[self.index].tests[index]
            .id
            .clone();

        self.wrote_baseline = false;
        for attempt in 0..=retries {
            self.writer.test_started(self.index, index, attempt)?;
            let started = Instant::now();
            let since = now_unix_nanos();
            self.attempt = Some(attempt);
            let tally = self.run_attempt(driver, test, timeout_ms, &id, since);
            self.attempt = None;
            let tally = tally?;
            self.probe_exit(driver);
            if let Some(telemetry) = driver.telemetry(since, self.exited.is_none()) {
                let telemetry = redact_json(&telemetry, &[]).value;
                let bytes = serde_json::to_vec_pretty(&telemetry)?;
                self.writer
                    .attach("telemetry", "application/json", "telemetry.json", &bytes)?;
            }
            let mut status = tally.status();
            let mut error = tally.first_error();
            let others = tally.other_errors();
            if let Some(reason) = &self.exited {
                if status == Status::Passed {
                    status = Status::Failed;
                }
                let message = format!("app exited: {reason}");
                error = Some(match error {
                    Some(mut error) => {
                        error.message = format!("{}\n{message}", error.message);
                        error
                    }
                    None => ErrorInfo::new(message),
                });
            }
            // Ctrl+C reaches the app too, so a cut-short test may also see it exit.
            if self.options.cancelled() && status != Status::Passed {
                status = Status::Interrupted;
            }
            let will_retry = status != expected
                && status != Status::Skipped
                && attempt < retries
                && self.exited.is_none()
                && !self.wrote_baseline
                && !self.options.cancelled();
            self.writer
                .test_finished(status, error.clone(), will_retry)?;
            let outcome = if will_retry {
                None
            } else {
                self.writer.report().suites[self.index].tests[index].outcome
            };
            self.reporter.test_finished(&TestDone {
                title: title(suite, test),
                status,
                outcome,
                attempt,
                duration: started.elapsed(),
                error: error.as_ref(),
                other_errors: &others,
            });
            if !will_retry {
                break;
            }
        }
        Ok(())
    }

    fn run_attempt(
        &mut self,
        driver: &mut dyn AppDriver,
        test: &TestCase,
        timeout_ms: u64,
        id: &str,
        since: i64,
    ) -> Result<Tally> {
        let suite = self.selected.suite;
        self.auto_snapshots = 0;
        let budget = Budget::new(Phase::BeforeEach, timeout_ms, Some(id.to_string()), since)
            .for_test(&test.name);
        let mut tally = self.run_steps(driver, &suite.before_each, &budget)?;
        if tally.hard.is_none() && !tally.skipped && self.exited.is_none() {
            let budget = Budget {
                phase: Phase::Test,
                ..budget
            };
            let steps = self.run_steps(driver, &test.steps, &budget)?;
            tally.merge(steps);
        }
        // afterEach always runs (unless the app died or the run was cancelled), with its own
        // time budget.
        if self.exited.is_none() && !self.options.cancelled() {
            let budget = Budget::new(Phase::AfterEach, timeout_ms, Some(id.to_string()), since)
                .for_test(&test.name);
            let after = self.run_steps(driver, &suite.after_each, &budget)?;
            tally.merge(after);
        }
        Ok(tally)
    }

    fn run_steps(
        &mut self,
        driver: &mut dyn AppDriver,
        steps: &[Step],
        budget: &Budget,
    ) -> Result<Tally> {
        let mut tally = Tally::default();
        let expect_timeout = Duration::from_millis(
            self.suite()
                .expect_timeout_ms
                .unwrap_or(DEFAULT_EXPECT_TIMEOUT_MS),
        );
        for step in steps {
            if self.options.cancelled() {
                // Suite hooks just stop; a test records that it was cut short.
                if budget.test_id.is_some() {
                    let error = ErrorInfo::new("interrupted");
                    tally.all.push(error.clone());
                    tally.hard = Some((Status::Interrupted, error));
                }
                break;
            }
            if Instant::now() >= budget.deadline.get() {
                let error = budget.timeout_error();
                tally.all.push(error.clone());
                tally.hard = Some((Status::TimedOut, error));
                break;
            }
            let index = self
                .writer
                .step_started(budget.phase, &step.title, step.action)?;
            // On assertions `timeoutMs` is the retry window (Playwright's `expect(..., { timeout })`),
            // so running out is a failed assertion; on actions it bounds the step.
            let assertion = matches!(
                step.kind,
                StepKind::Expect(_) | StepKind::ExpectValue(_) | StepKind::Telemetry(_)
            );
            // A person's wait is off the test clock, like Playwright's `page.pause()`; the
            // gate's own `timeoutMs` (or the step's) bounds it instead.
            let gate = matches!(step.kind, StepKind::Gate(_));
            let (deadline, expect_timeout) = match step.timeout_ms {
                _ if gate => (
                    step.timeout_ms.map_or(far_future(), |ms| {
                        Instant::now() + Duration::from_millis(ms)
                    }),
                    expect_timeout,
                ),
                Some(ms) if assertion => (budget.deadline.get(), Duration::from_millis(ms)),
                Some(ms) => (
                    (Instant::now() + Duration::from_millis(ms)).min(budget.deadline.get()),
                    expect_timeout,
                ),
                None => (budget.deadline.get(), expect_timeout),
            };
            let snapshot = self.snapshot_target(step, &budget.snapshot_base);
            let tracing = self.tracing();
            let mut ctx = StepCtx {
                writer: &mut *self.writer,
                deadline,
                expect_timeout,
                test_id: budget.test_id.clone(),
                step_index: index,
                since_unix_nanos: budget.since_unix_nanos,
                snapshot,
                wrote_baseline: false,
                cancel: self.options.cancel.clone(),
            };
            let result = match &step.kind {
                StepKind::Run(command) => {
                    run_command(&mut *self.host, &self.env, step, command, &mut ctx)
                }
                _ if gate => budget.untimed(|| driver.run_step(step, &mut ctx)),
                _ => driver.run_step(step, &mut ctx),
            };
            // Evidence captured below is not part of the step's duration.
            ctx.writer.step_ended();
            if ctx.wrote_baseline {
                self.wrote_baseline = true;
                ctx.writer.require_review();
            }
            let (step_status, mut error) = match result {
                StepResult::Passed => {
                    if tracing {
                        budget.untimed(|| driver.capture_step(&mut ctx));
                    }
                    self.writer.step_finished(Status::Passed, false, None)?;
                    continue;
                }
                StepResult::Skipped => {
                    if tracing {
                        budget.untimed(|| driver.capture_step(&mut ctx));
                    }
                    self.writer.step_finished(Status::Skipped, false, None)?;
                    tally.skipped = true;
                    break;
                }
                StepResult::Failed(error) => (Status::Failed, error),
                StepResult::TimedOut(error) => (Status::TimedOut, error),
            };
            error.step = Some(index);
            // Writing a missing baseline fails the test but, as in Playwright, lets the rest
            // run so one pass records every new baseline.
            let soft = is_soft(step) || ctx.wrote_baseline;
            if !soft {
                if let Some(reason) = driver.exited() {
                    self.exited = Some(reason);
                } else {
                    driver.capture_failure(&mut ctx);
                }
            } else if tracing {
                budget.untimed(|| driver.capture_step(&mut ctx));
            }
            self.writer
                .step_finished(step_status, soft, Some(error.clone()))?;
            tally.all.push(error.clone());
            if soft {
                tally.soft.get_or_insert(error);
                continue;
            }
            // Only running out of the whole budget times the test out; a step's own
            // timeout is an ordinary failure, as in Playwright.
            let status = if Instant::now() >= budget.deadline.get() {
                Status::TimedOut
            } else {
                Status::Failed
            };
            tally.hard = Some((status, error));
            break;
        }
        Ok(tally)
    }

    /// Resolve which baseline a snapshot assertion checks, numbering unnamed ones.
    fn snapshot_target(&mut self, step: &Step, base: &str) -> Option<SnapshotTarget> {
        let StepKind::Expect(expectation) = &step.kind else {
            return None;
        };
        let (kind, name) = match &expectation.matcher {
            Matcher::ToMatchAriaSnapshot(name) => (SnapshotKind::Aria, name),
            Matcher::ToHaveScreenshot(options) => (SnapshotKind::Screenshot, &options.name),
            _ => return None,
        };
        let name = name.clone().unwrap_or_else(|| {
            self.auto_snapshots += 1;
            snapshot::auto_name(base, self.auto_snapshots)
        });
        Some(SnapshotTarget::new(
            &self.suite().path,
            kind,
            &name,
            self.options.update_snapshots,
        ))
    }

    fn probe_exit(&mut self, driver: &mut dyn AppDriver) {
        if self.exited.is_none() {
            self.exited = driver.exited();
        }
    }

    fn tracing(&self) -> bool {
        match self.options.trace {
            TraceMode::Off => false,
            TraceMode::On => true,
            TraceMode::OnFirstRetry => self.attempt == Some(1),
        }
    }

    fn close(&mut self, driver: &mut dyn AppDriver) -> Result<()> {
        self.writer
            .step_started(Phase::AfterAll, "close app", "close")?;
        match driver.close() {
            Ok(()) => self.writer.step_finished(Status::Passed, false, None),
            Err(error) => {
                let error = ErrorInfo::new(format!("app cleanup failed: {error:#}"));
                self.writer
                    .step_finished(Status::Failed, false, Some(error.clone()))?;
                self.error.get_or_insert(error);
                Ok(())
            }
        }
    }

    fn hook_step(
        &mut self,
        hook: &Value,
        teardown: bool,
        index: usize,
    ) -> Result<Option<ErrorInfo>> {
        self.hook_step_maybe(hook, teardown, index, false)
    }

    /// Run one command hook as a suite step; returns its error, if any.
    fn hook_step_maybe(
        &mut self,
        hook: &Value,
        teardown: bool,
        index: usize,
        skip: bool,
    ) -> Result<Option<ErrorInfo>> {
        let lifecycle = if teardown { "teardown" } else { "setup" };
        let phase = if teardown {
            Phase::AfterAll
        } else {
            Phase::BeforeAll
        };
        let name = hook.get("name").and_then(Value::as_str).unwrap_or("hook");
        let step = self
            .writer
            .step_started(phase, &format!("{lifecycle}: {name}"), "hook")?;
        if skip {
            self.writer.step_finished(Status::Skipped, false, None)?;
            return Ok(None);
        }
        let mut env = self.env.clone();
        let env_file = self.env_file.as_ref().filter(|_| !teardown);
        if let Some(path) = env_file {
            env.push((ENV_FILE_VAR.to_string(), path.display().to_string()));
        }
        let (status, message) = match self.host.run_hook(hook, teardown, &env) {
            Ok(result) => {
                let report = redact_json(&result.report, &[]).value;
                self.writer.attach(
                    "hook output",
                    "application/json",
                    &format!("hooks/{lifecycle}-{index}.json"),
                    &serde_json::to_vec_pretty(&report)?,
                )?;
                let bad_env = match env_file.map(|path| read_env_file(path)) {
                    Some(Ok(vars)) => {
                        self.env = self.selected.suite.env.clone();
                        self.env.extend(vars);
                        None
                    }
                    Some(Err(problem)) => Some(format!("wrote a bad ${ENV_FILE_VAR}: {problem}")),
                    None => None,
                };
                // The hook's own failure comes first; a bad env file is often its symptom.
                let (status, message) = match (result.passed, result.timed_out, bad_env) {
                    (true, _, None) => {
                        self.writer.step_finished(Status::Passed, false, None)?;
                        return Ok(None);
                    }
                    (true, _, Some(bad)) => (Status::Failed, bad),
                    (false, true, bad) => (Status::TimedOut, with(bad, "timed out")),
                    (false, false, bad) => (Status::Failed, with(bad, "failed")),
                };
                (status, format!("{lifecycle} hook `{name}` {message}"))
            }
            Err(error) => (
                Status::Failed,
                format!("{lifecycle} hook `{name}` could not run: {error:#}"),
            ),
        };
        let error = ErrorInfo {
            step: Some(step),
            ..ErrorInfo::new(message)
        };
        self.writer
            .step_finished(status, false, Some(error.clone()))?;
        Ok(Some(error))
    }
}

/// Run a `run` step's command as a hook bounded by the step's remaining time, attach its
/// redacted output, and check its exit code and output.
fn run_command(
    host: &mut dyn Host,
    env: &[(String, String)],
    step: &Step,
    command: &CommandStep,
    ctx: &mut StepCtx<'_>,
) -> StepResult {
    let fail = |message: String| StepResult::Failed(ErrorInfo::new(message));
    let timeout_ms = u64::try_from(ctx.remaining().as_millis()).unwrap_or(u64::MAX);
    if timeout_ms == 0 {
        return StepResult::TimedOut(ErrorInfo::new("no time left to run the command"));
    }
    let hook = command.hook(&step.title, timeout_ms);
    let result = match host.run_hook(&hook, false, env) {
        Ok(result) => result,
        Err(error) => return fail(format!("could not run `{}`: {error:#}", command.run)),
    };
    let mut shown = redact_json(&result.report, &[]).value;
    if let Some(report) = shown.as_object_mut() {
        report.remove("lifecycle");
        report.remove("always");
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&shown) {
        let file = format!("step-{}-run.json", ctx.step_index);
        if let Err(error) = ctx
            .writer
            .attach("command output", "application/json", &file, &bytes)
        {
            return fail(format!("could not record command output: {error:#}"));
        }
    }
    let text = |report: &Value, key: &str| {
        report
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let tail = |key: &str| {
        let text = text(&shown, key);
        let text = text.trim();
        let start = text
            .char_indices()
            .rev()
            .nth(OUTPUT_SHOWN - 1)
            .map_or(0, |(at, _)| at);
        if text.is_empty() {
            format!("{key} was empty")
        } else if start > 0 {
            format!("{key} ended with: …{}", &text[start..])
        } else {
            format!("{key}: {text}")
        }
    };
    if result.report.get("timedOut").and_then(Value::as_bool) == Some(true) || result.timed_out {
        return StepResult::TimedOut(ErrorInfo::new(format!(
            "`{}` did not finish within {timeout_ms}ms",
            command.run
        )));
    }
    let exit_code = result
        .report
        .get("exitCode")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok());
    if exit_code != Some(command.exit_code) {
        let got = exit_code.map_or("no exit code".to_string(), |code| {
            format!("exit code {code}")
        });
        return fail(format!(
            "expected `{}` to exit with {}, got {got}; {}",
            command.run,
            command.exit_code,
            tail("stderr")
        ));
    }
    for (key, needle) in [("stdout", &command.stdout), ("stderr", &command.stderr)] {
        if let Some(needle) = needle {
            if !needle.is_in(&text(&result.report, key)) {
                return fail(format!("expected {key} to contain {needle}; {}", tail(key)));
            }
        }
    }
    StepResult::Passed
}

/// `what` happened, plus a secondary problem if there was one.
fn with(secondary: Option<String>, what: &str) -> String {
    match secondary {
        Some(secondary) => format!("{what}; it also {secondary}"),
        None => what.to_string(),
    }
}

/// Read the `KEY=VALUE` lines setup hooks appended to [`ENV_FILE_VAR`], in order. Blank
/// lines and `#` comments are skipped; a UTF-8 or UTF-16 byte order mark is honored so
/// Windows PowerShell's `>>` works.
fn read_env_file(path: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("could not read it: {error}"))?;
    let text = if let Some(wide) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        if wide.len() % 2 != 0 {
            return Err("it is not valid UTF-16".to_string());
        }
        let units: Vec<u16> = wide
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| "it is not valid UTF-16".to_string())?
    } else {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        String::from_utf8(bytes.to_vec()).map_err(|_| "it is not valid UTF-8".to_string())?
    };
    let mut vars = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        // A BOM can land mid-file when hooks append with different encodings.
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected KEY=VALUE", number + 1));
        };
        check_env_name(name).map_err(|problem| format!("line {}: {problem}", number + 1))?;
        vars.push((name.to_string(), value.to_string()));
    }
    Ok(vars)
}

/// How much of a command's output a failure message shows.
const OUTPUT_SHOWN: usize = 500;

impl Budget {
    fn new(phase: Phase, timeout_ms: u64, test_id: Option<String>, since_unix_nanos: i64) -> Self {
        Self {
            phase,
            deadline: Cell::new(Instant::now() + Duration::from_millis(timeout_ms)),
            timeout_ms,
            test_id,
            since_unix_nanos,
            snapshot_base: format!("{phase:?}"),
        }
    }

    /// Unnamed snapshots in tests are named after the test.
    fn for_test(mut self, name: &str) -> Self {
        self.snapshot_base = name.to_string();
        self
    }

    /// Run `work` (trace capture, a human gate) without charging its time to the budget.
    fn untimed<T>(&self, work: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = work();
        self.deadline.set(self.deadline.get() + started.elapsed());
        result
    }

    fn timeout_error(&self) -> ErrorInfo {
        ErrorInfo::new(format!("timeout of {}ms exceeded", self.timeout_ms))
    }
}

/// A deadline that never arrives in practice, for waits bounded by something else.
fn far_future() -> Instant {
    Instant::now() + Duration::from_secs(365 * 24 * 60 * 60)
}

/// Wall-clock now, floored to the millisecond and backed off one more: the webview stamps
/// telemetry as `Date.now() * 1e6`, which is whole milliseconds rounded through an f64, so a
/// record from this very millisecond can land just below the boundary.
fn now_unix_nanos() -> i64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let millis = i64::try_from(nanos / 1_000_000).unwrap_or(i64::MAX / 1_000_000);
    millis.saturating_sub(1) * 1_000_000
}

fn hook_always(hook: &Value) -> bool {
    hook.get("always").and_then(Value::as_bool).unwrap_or(false)
}

/// Soft assertions record a failure but let the test continue.
pub fn is_soft(step: &Step) -> bool {
    match &step.kind {
        StepKind::Expect(expectation) => expectation.soft,
        StepKind::ExpectValue(expectation) => expectation.soft,
        StepKind::Telemetry(expectation) => match expectation {
            TelemetryExpectation::Ipc { soft, .. }
            | TelemetryExpectation::Event { soft, .. }
            | TelemetryExpectation::Log { soft, .. }
            | TelemetryExpectation::Span { soft, .. }
            | TelemetryExpectation::NoErrors { soft, .. }
            | TelemetryExpectation::NoTelemetry { soft, .. } => *soft,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests;
