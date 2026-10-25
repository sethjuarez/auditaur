//! Runs selected drill tests against an app and records them with [`RunWriter`].
//!
//! Orchestration (hooks, retries, timeouts, soft failures) is separate from app
//! control: a [`Host`] launches apps and runs command hooks, and an [`AppDriver`]
//! performs steps. Unit tests swap in fakes; [`real`] drives Tauri apps.

pub mod real;

use std::time::{Duration, Instant};

use anyhow::Result;
use auditaur_core::redaction::redact_json;
use serde_json::Value;

use super::{
    model::{DrillSuite, Step, StepKind, TelemetryExpectation, TestCase, TestMode},
    run::{ErrorInfo, Outcome, Phase, RunWriter, Status},
    title, Selected,
};

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
    #[allow(dead_code)] // Used by DOM assertions in the next step.
    pub expect_timeout: Duration,
    pub test_id: Option<String>,
    pub step_index: usize,
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
}

pub trait AppDriver {
    /// Session details recorded on the suite.
    fn app_info(&self) -> Value;
    fn run_step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepResult;
    /// Attach evidence (screenshot, DOM) to the failing step. Best effort.
    fn capture_failure(&mut self, _ctx: &mut StepCtx<'_>) {}
    /// Why the app can no longer be driven, once it has died.
    fn exited(&mut self) -> Option<String>;
    fn close(&mut self) -> Result<()>;
}

/// Launches apps and runs suite command hooks.
pub trait Host {
    fn launch(&mut self, suite: &DrillSuite) -> Result<Box<dyn AppDriver>>;
    fn run_hook(&mut self, hook: &Value, teardown: bool) -> Result<HookResult>;
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
    pub error: Option<&'a ErrorInfo>,
}

#[cfg(test)]
pub struct NoReporter;
#[cfg(test)]
impl Reporter for NoReporter {}

#[derive(Debug, Clone, Default)]
pub struct RunnerOptions {
    /// Used when neither the test nor its suite sets `retries`.
    pub retries: Option<u32>,
}

/// Run every selected test. Writer errors abort the run; everything else is recorded.
pub fn execute(
    writer: &mut RunWriter,
    plan: &[Selected<'_>],
    host: &mut dyn Host,
    reporter: &mut dyn Reporter,
    options: &RunnerOptions,
) -> Result<()> {
    for (index, selected) in plan.iter().enumerate() {
        SuiteRun {
            writer: &mut *writer,
            host: &mut *host,
            reporter: &mut *reporter,
            options,
            index,
            selected,
            error: None,
            exited: None,
        }
        .run()?;
    }
    Ok(())
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
}

/// Failures collected while running a list of steps.
#[derive(Default)]
struct Tally {
    hard: Option<(Status, ErrorInfo)>,
    soft: Option<ErrorInfo>,
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
    }

    fn status(&self) -> Status {
        match &self.hard {
            Some((status, _)) => *status,
            None if self.soft.is_some() => Status::Failed,
            None => Status::Passed,
        }
    }
}

/// Where a list of steps runs and how long it may take.
struct Budget {
    phase: Phase,
    deadline: Instant,
    timeout_ms: u64,
    test_id: Option<String>,
}

impl SuiteRun<'_, '_> {
    fn suite(&self) -> &DrillSuite {
        self.selected.suite
    }

    fn run(mut self) -> Result<()> {
        let suite = self.selected.suite;
        self.writer.suite_started(self.index)?;
        self.reporter.suite_started(suite);

        for (index, hook) in suite.setup.iter().enumerate() {
            if let Some(error) = self.hook_step(hook, false, index)? {
                self.error = Some(error);
                break;
            }
        }

        let mut driver = if self.error.is_none() {
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
        self.writer.suite_finished(self.index, self.error.take())
    }

    fn launch(&mut self) -> Result<Option<Box<dyn AppDriver>>> {
        let index = self
            .writer
            .step_started(Phase::BeforeAll, "launch app", "launch")?;
        match self.host.launch(self.selected.suite) {
            Ok(driver) => {
                self.writer.step_finished(Status::Passed, false, None)?;
                self.writer.set_suite_app(driver.app_info())?;
                Ok(Some(driver))
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

        let tally = self.run_steps(
            driver,
            &suite.before_all,
            &Budget::new(Phase::BeforeAll, timeout_ms, None),
        )?;
        if let Some(error) = tally.first_error() {
            self.error = Some(error);
        }
        self.probe_exit(driver);

        if self.error.is_none() && self.exited.is_none() {
            for (index, test) in self.selected.tests.iter().enumerate() {
                self.run_test(driver, index, test)?;
                if self.exited.is_some() {
                    break;
                }
            }
        }

        if self.exited.is_none() {
            let tally = self.run_steps(
                driver,
                &suite.after_all,
                &Budget::new(Phase::AfterAll, timeout_ms, None),
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

    fn run_test(
        &mut self,
        driver: &mut dyn AppDriver,
        index: usize,
        test: &TestCase,
    ) -> Result<()> {
        let suite = self.selected.suite;
        if matches!(test.mode, TestMode::Skip | TestMode::Fixme) {
            self.writer.test_skipped(self.index, index)?;
            self.reporter.test_finished(&TestDone {
                title: title(suite, test),
                status: Status::Skipped,
                outcome: Some(Outcome::Skipped),
                attempt: 0,
                duration: Duration::ZERO,
                error: None,
            });
            return Ok(());
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

        for attempt in 0..=retries {
            self.writer.test_started(self.index, index, attempt)?;
            let started = Instant::now();
            let tally = self.run_attempt(driver, test, timeout_ms, &id)?;
            self.probe_exit(driver);
            let mut status = tally.status();
            let mut error = tally.first_error();
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
            let will_retry = status != expected && attempt < retries && self.exited.is_none();
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
    ) -> Result<Tally> {
        let suite = self.selected.suite;
        let budget = Budget::new(Phase::BeforeEach, timeout_ms, Some(id.to_string()));
        let mut tally = self.run_steps(driver, &suite.before_each, &budget)?;
        if tally.hard.is_none() && self.exited.is_none() {
            let budget = Budget {
                phase: Phase::Test,
                ..budget
            };
            let steps = self.run_steps(driver, &test.steps, &budget)?;
            tally.merge(steps);
        }
        // afterEach always runs (unless the app died), with its own time budget.
        if self.exited.is_none() {
            let budget = Budget::new(Phase::AfterEach, timeout_ms, Some(id.to_string()));
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
            if Instant::now() >= budget.deadline {
                tally.hard = Some((Status::TimedOut, budget.timeout_error()));
                break;
            }
            let index = self
                .writer
                .step_started(budget.phase, &step.title, step.action)?;
            let deadline = step
                .timeout_ms
                .map(|ms| (Instant::now() + Duration::from_millis(ms)).min(budget.deadline))
                .unwrap_or(budget.deadline);
            let mut ctx = StepCtx {
                writer: &mut *self.writer,
                deadline,
                expect_timeout,
                test_id: budget.test_id.clone(),
                step_index: index,
            };
            let result = driver.run_step(step, &mut ctx);
            let (step_status, mut error) = match result {
                StepResult::Passed => {
                    self.writer.step_finished(Status::Passed, false, None)?;
                    continue;
                }
                StepResult::Failed(error) => (Status::Failed, error),
                StepResult::TimedOut(error) => (Status::TimedOut, error),
            };
            error.step = Some(index);
            let soft = is_soft(step);
            if !soft {
                if let Some(reason) = driver.exited() {
                    self.exited = Some(reason);
                } else {
                    driver.capture_failure(&mut ctx);
                }
            }
            self.writer
                .step_finished(step_status, soft, Some(error.clone()))?;
            if soft {
                tally.soft.get_or_insert(error);
                continue;
            }
            // Only running out of the whole budget times the test out; a step's own
            // timeout is an ordinary failure, as in Playwright.
            let status = if Instant::now() >= budget.deadline {
                Status::TimedOut
            } else {
                Status::Failed
            };
            tally.hard = Some((status, error));
            break;
        }
        Ok(tally)
    }

    fn probe_exit(&mut self, driver: &mut dyn AppDriver) {
        if self.exited.is_none() {
            self.exited = driver.exited();
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
        let (status, message) = match self.host.run_hook(hook, teardown) {
            Ok(result) => {
                let report = redact_json(&result.report, &[]).value;
                self.writer.attach(
                    "hook output",
                    "application/json",
                    &format!("hooks/{lifecycle}-{index}.json"),
                    &serde_json::to_vec_pretty(&report)?,
                )?;
                if result.passed {
                    self.writer.step_finished(Status::Passed, false, None)?;
                    return Ok(None);
                }
                if result.timed_out {
                    (
                        Status::TimedOut,
                        format!("{lifecycle} hook `{name}` timed out"),
                    )
                } else {
                    (Status::Failed, format!("{lifecycle} hook `{name}` failed"))
                }
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

impl Budget {
    fn new(phase: Phase, timeout_ms: u64, test_id: Option<String>) -> Self {
        Self {
            phase,
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            timeout_ms,
            test_id,
        }
    }

    fn timeout_error(&self) -> ErrorInfo {
        ErrorInfo::new(format!("timeout of {}ms exceeded", self.timeout_ms))
    }
}

fn hook_always(hook: &Value) -> bool {
    hook.get("always").and_then(Value::as_bool).unwrap_or(false)
}

/// Soft assertions record a failure but let the test continue.
pub fn is_soft(step: &Step) -> bool {
    match &step.kind {
        StepKind::Expect(expectation) => expectation.soft,
        StepKind::Telemetry(expectation) => match expectation {
            TelemetryExpectation::Ipc { soft, .. }
            | TelemetryExpectation::Event { soft, .. }
            | TelemetryExpectation::Log { soft, .. }
            | TelemetryExpectation::NoErrors { soft, .. } => *soft,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests;
