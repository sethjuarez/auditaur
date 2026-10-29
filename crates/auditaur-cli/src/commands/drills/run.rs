//! Run folders: the versioned contract between the drill runner and its readers
//! (CLI, MCP tools and canvases). Readers only read; they never recompute.
//!
//! ```text
//! .auditaur/drill-runs/
//!   .gitignore                      `*`, so runs are never committed
//!   <runId>/                        newest id (lexical sort) is the latest run
//!     run.json                      RunReport snapshot, replaced atomically
//!     events.jsonl                  EventLine per line, appended live
//!     suites/<suiteId>/...          beforeAll/afterAll attachments
//!     tests/<testId>/attempt-<n>/
//!       attempt.json                AttemptReport: steps, errors, attachments
//!       ...                         attachments (screenshots, ARIA, telemetry)
//! ```
//!
//! Live readers load `run.json`, then apply events whose `seq` is greater than its `seq`.
//! Each event is appended before the snapshot that reflects it, so that always holds.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{effective_tags, model::TestMode, title, Selected};

pub const RUN_SCHEMA_VERSION: u32 = 1;
pub const RUNS_DIR: &str = ".auditaur/drill-runs";
pub const DEFAULT_KEEP_RUNS: usize = 20;
/// Creating this file in a running run's folder asks its `drill test` to stop gracefully.
pub const CANCEL_REQUEST_FILE: &str = "cancel.request";
// Served to tools and canvases in later steps; tests validate against it now.
#[allow(dead_code)]
pub const RUN_SCHEMA: &str = include_str!("../../../assets/drill-run.schema.json");
#[allow(dead_code)]
pub const RUN_SCHEMA_URL: &str = "https://auditaur.dev/schemas/drill-run.schema.json";
/// Unreadable run folders younger than this may still be starting; prune leaves them.
const PRUNE_GRACE: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    Running,
    Passed,
    Failed,
    TimedOut,
    Skipped,
    Interrupted,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Running => "running",
            Status::Passed => "passed",
            Status::Failed => "failed",
            Status::TimedOut => "timedOut",
            Status::Skipped => "skipped",
            Status::Interrupted => "interrupted",
        }
    }
}

/// How a finished test compares with what it was expected to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Expected,
    Unexpected,
    /// Failed, then passed on retry.
    Flaky,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    BeforeAll,
    BeforeEach,
    Test,
    AfterEach,
    AfterAll,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInfo {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
}

impl ErrorInfo {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub content_type: String,
    /// Relative to the run folder, with forward slashes.
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub flaky: usize,
    pub skipped: usize,
    pub not_run: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub schema_version: u32,
    pub run_id: String,
    pub status: Status,
    /// Last event `seq` reflected in this snapshot.
    pub seq: u64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    /// Process that owns a `running` run; readers treat a dead owner as interrupted.
    pub pid: u32,
    pub workspace: String,
    pub invocation: Invocation,
    pub environment: Environment,
    pub totals: Totals,
    pub suites: Vec<SuiteReport>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invocation {
    pub args: Vec<String>,
    pub patterns: Vec<String>,
    pub grep: Option<String>,
    pub grep_invert: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub os: String,
    pub arch: String,
    pub auditaur_version: String,
}

impl Environment {
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            auditaur_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteReport {
    /// Stable across runs; see [`suite_id`].
    pub id: String,
    pub suite: String,
    /// Workspace-relative drill file path with forward slashes.
    pub path: String,
    pub status: Status,
    pub started_at: Option<String>,
    pub duration_ms: Option<u64>,
    /// The app session this suite ran against, once launched.
    pub app: Option<Value>,
    /// Suite-level failure such as an app launch or hook error.
    pub error: Option<ErrorInfo>,
    /// `beforeAll` and `afterAll` steps, which belong to no single test.
    pub hooks: Vec<StepReport>,
    pub tests: Vec<TestReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    /// Stable across runs; see [`test_id`].
    pub id: String,
    pub name: String,
    pub title: String,
    pub tags: Vec<String>,
    pub mode: TestMode,
    pub mode_reason: Option<String>,
    pub expected_status: Status,
    pub status: Status,
    pub outcome: Option<Outcome>,
    pub duration_ms: Option<u64>,
    pub attempts: Vec<AttemptSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptSummary {
    pub attempt: u32,
    pub status: Status,
    pub started_at: String,
    pub duration_ms: Option<u64>,
    pub error: Option<ErrorInfo>,
    /// Relative to the run folder.
    pub dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptReport {
    pub schema_version: u32,
    pub test_id: String,
    pub title: String,
    pub attempt: u32,
    pub status: Status,
    pub started_at: String,
    pub duration_ms: u64,
    pub error: Option<ErrorInfo>,
    pub steps: Vec<StepReport>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    pub index: usize,
    pub phase: Phase,
    pub title: String,
    pub action: String,
    pub status: Status,
    pub soft: bool,
    pub started_at: String,
    pub duration_ms: u64,
    pub error: Option<ErrorInfo>,
    pub attachments: Vec<Attachment>,
}

/// One line of `events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventLine {
    pub seq: u64,
    pub ts: String,
    #[serde(flatten)]
    pub event: RunEvent,
}

/// Step, attachment and gate events belong to a test attempt (`testId` and `attempt`
/// set) or to a suite hook (`testId` and `attempt` null).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    RunStarted { run_id: String, tests: usize },
    #[serde(rename_all = "camelCase")]
    SuiteStarted {
        suite_id: String,
        suite: String,
        path: String,
    },
    #[serde(rename_all = "camelCase")]
    SuiteApp { suite_id: String, app: Value },
    #[serde(rename_all = "camelCase")]
    TestStarted {
        suite_id: String,
        test_id: String,
        title: String,
        attempt: u32,
    },
    #[serde(rename_all = "camelCase")]
    StepStarted {
        suite_id: String,
        test_id: Option<String>,
        attempt: Option<u32>,
        index: usize,
        phase: Phase,
        title: String,
        action: String,
    },
    #[serde(rename_all = "camelCase")]
    StepFinished {
        suite_id: String,
        test_id: Option<String>,
        attempt: Option<u32>,
        index: usize,
        status: Status,
        soft: bool,
        duration_ms: u64,
        error: Option<ErrorInfo>,
    },
    #[serde(rename_all = "camelCase")]
    Attachment {
        suite_id: String,
        test_id: Option<String>,
        attempt: Option<u32>,
        step: Option<usize>,
        attachment: Attachment,
    },
    #[serde(rename_all = "camelCase")]
    GateWaiting {
        suite_id: String,
        test_id: Option<String>,
        attempt: Option<u32>,
        step: usize,
        gate: Value,
    },
    #[serde(rename_all = "camelCase")]
    GateResolved {
        suite_id: String,
        test_id: Option<String>,
        attempt: Option<u32>,
        step: usize,
        response: Value,
    },
    #[serde(rename_all = "camelCase")]
    TestFinished {
        suite_id: String,
        test_id: String,
        attempt: u32,
        status: Status,
        duration_ms: u64,
        error: Option<ErrorInfo>,
        will_retry: bool,
        outcome: Option<Outcome>,
    },
    #[serde(rename_all = "camelCase")]
    SuiteFinished {
        suite_id: String,
        status: Status,
        duration_ms: u64,
        error: Option<ErrorInfo>,
    },
    #[serde(rename_all = "camelCase")]
    RunFinished {
        status: Status,
        duration_ms: u64,
        totals: Totals,
    },
}

fn fnv1a(parts: &[&str]) -> String {
    // FNV-1a 64: tiny, dependency-free and identical on every platform.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (index, part) in parts.iter().enumerate() {
        let separator: &[u8] = if index == 0 { &[] } else { &[0] };
        for byte in separator.iter().chain(part.as_bytes()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// Stable test identity: a hash of the workspace-relative file path and test name.
/// Used for artifact folders so names never need to be valid paths.
pub fn test_id(suite_path: &str, test_name: &str) -> String {
    fnv1a(&[suite_path, test_name])
}

/// Stable suite identity: a hash of the workspace-relative file path.
pub fn suite_id(suite_path: &str) -> String {
    fnv1a(&[suite_path])
}

/// `path` (as discovered, so relative paths resolve against the current directory) made
/// relative to `workspace` with forward slashes, using on-disk casing where possible.
pub fn workspace_path(workspace: &Path, path: &Path) -> String {
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let root = canonical(workspace);
    let full = canonical(path);
    full.strip_prefix(&root)
        .unwrap_or(&full)
        .to_string_lossy()
        .replace('\\', "/")
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn new_run_id() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    )
}

/// `YYYYMMDD-HHMMSS-mmm`, optionally followed by a `-NN` collision suffix.
/// Fixed widths keep lexical order equal to creation order.
pub fn is_run_id(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() != 19 && bytes.len() != 22 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let base =
        bytes[8] == b'-' && bytes[15] == b'-' && digits(0..8) && digits(9..15) && digits(16..19);
    base && (bytes.len() == 19 || (bytes[19] == b'-' && digits(20..22)))
}

fn elapsed_ms(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Write via a uniquely named temp file and rename so readers never see a partial
/// file. Retries briefly when a Windows reader holds the target open.
pub fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(value)?)
}

/// How long a write waits for another process to release the target. Losing a write
/// leaves a run that reads as interrupted, so wait out antivirus or indexer scans.
const LOCKED_FILE_WAIT: Duration = Duration::from_secs(5);

/// Whether a rename failed only because another process holds the file open. Windows
/// reports that as access denied (5), a sharing violation (32) or a lock violation (33).
fn is_transient_lock(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
        || (cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33)))
}

/// Write bytes via a uniquely named temp file and rename (see [`write_json_atomic`]).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    fs::write(&tmp, bytes).with_context(|| format!("failed to write `{}`", tmp.display()))?;
    let deadline = Instant::now() + LOCKED_FILE_WAIT;
    let mut pause = Duration::from_millis(25);
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(error) if is_transient_lock(&error) && Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(200));
            }
            Err(error) => {
                let _ = fs::remove_file(&tmp);
                return Err(error)
                    .with_context(|| format!("failed to replace `{}`", path.display()));
            }
        }
    }
}

fn rel(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A relative path of plain segments (no `:`, no `.`/`..`, no root), so it stays inside
/// its base folder on every platform. Matches the schema's `relPath`.
pub fn is_contained(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(':')
        && path
            .split(['/', '\\'])
            .all(|segment| segment.chars().any(|c| c != '.'))
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub struct RunOptions {
    pub workspace: PathBuf,
    pub invocation: Invocation,
    pub keep: usize,
}

struct ActiveAttempt {
    suite: usize,
    test: usize,
    attempt: u32,
    dir: PathBuf,
    started_at: String,
    start: Instant,
    steps: Vec<StepReport>,
    attachments: Vec<Attachment>,
    /// Fail the test whatever its expected status (see [`RunWriter::require_review`]).
    needs_review: bool,
}

struct ActiveStep {
    index: usize,
    phase: Phase,
    title: String,
    action: String,
    start: Instant,
    /// Set by [`RunWriter::step_ended`].
    ended_ms: Option<u64>,
    started_at: String,
    attachments: Vec<Attachment>,
}

/// Where steps and attachments go: the running test attempt, else the running suite's hooks.
struct Scope {
    suite: usize,
    suite_id: String,
    test_id: Option<String>,
    attempt: Option<u32>,
}

/// Writes one run folder. Methods address suites and tests by their index in the plan.
pub struct RunWriter {
    runs_root: PathBuf,
    dir: PathBuf,
    report: RunReport,
    events: File,
    seq: u64,
    start: Instant,
    suite_starts: Vec<Option<Instant>>,
    current_suite: Option<usize>,
    attempt: Option<ActiveAttempt>,
    step: Option<ActiveStep>,
    keep: usize,
    finished: bool,
    /// Set when `RunFinished` is logged but its `run.json` snapshot failed.
    final_snapshot_pending: bool,
    /// Set when an event line may be partial; nothing more is written.
    poisoned: bool,
}

impl RunWriter {
    /// Create the run folder with every selected test pending, so readers see the plan at once.
    pub fn create(options: RunOptions, plan: &[Selected<'_>]) -> Result<Self> {
        let runs_root = options.workspace.join(RUNS_DIR);
        fs::create_dir_all(&runs_root)
            .with_context(|| format!("failed to create `{}`", runs_root.display()))?;
        let gitignore = runs_root.join(".gitignore");
        if !gitignore.exists() {
            fs::write(&gitignore, "*\n")?;
        }
        let (run_id, dir) = reserve_run_dir(&runs_root)?;

        let suites = plan
            .iter()
            .map(|selected| {
                let suite = selected.suite;
                let path = workspace_path(&options.workspace, &suite.path);
                let tests = selected
                    .tests
                    .iter()
                    .map(|test| {
                        let skipped = matches!(test.mode, TestMode::Skip | TestMode::Fixme);
                        TestReport {
                            id: test_id(&path, &test.name),
                            name: test.name.clone(),
                            title: title(suite, test),
                            tags: effective_tags(suite, test),
                            mode: test.mode,
                            mode_reason: test.mode_reason.clone(),
                            expected_status: if skipped {
                                Status::Skipped
                            } else if test.expect_fail.is_some() {
                                Status::Failed
                            } else {
                                Status::Passed
                            },
                            status: Status::Pending,
                            outcome: None,
                            duration_ms: None,
                            attempts: Vec::new(),
                        }
                    })
                    .collect();
                SuiteReport {
                    id: suite_id(&path),
                    suite: suite.suite.clone(),
                    path,
                    status: Status::Pending,
                    started_at: None,
                    duration_ms: None,
                    app: None,
                    error: None,
                    hooks: Vec::new(),
                    tests,
                }
            })
            .collect::<Vec<_>>();
        let tests = suites.iter().map(|suite| suite.tests.len()).sum();

        let report = RunReport {
            schema_version: RUN_SCHEMA_VERSION,
            run_id: run_id.clone(),
            status: Status::Running,
            seq: 0,
            started_at: now(),
            ended_at: None,
            duration_ms: None,
            pid: std::process::id(),
            workspace: rel(
                &fs::canonicalize(&options.workspace).unwrap_or_else(|_| options.workspace.clone())
            ),
            invocation: options.invocation,
            environment: Environment::current(),
            totals: Totals {
                tests,
                not_run: tests,
                ..Totals::default()
            },
            suites,
        };
        let suite_count = report.suites.len();
        // Publish the plan right away so the newest folder is readable as soon as possible.
        write_json_atomic(&dir.join("run.json"), &report)?;
        let events = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(dir.join("events.jsonl"))?;
        let mut writer = Self {
            runs_root,
            dir,
            report,
            events,
            seq: 0,
            start: Instant::now(),
            suite_starts: vec![None; suite_count],
            current_suite: None,
            attempt: None,
            step: None,
            keep: options.keep.max(1),
            finished: false,
            final_snapshot_pending: false,
            poisoned: false,
        };
        writer.commit(RunEvent::RunStarted { run_id, tests }, true)?;
        Ok(writer)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn report(&self) -> &RunReport {
        &self.report
    }

    /// Append an event, then with `snapshot` replace `run.json` to reflect it. The event
    /// is durable first, so a snapshot never claims a `seq` missing from `events.jsonl`.
    fn commit(&mut self, event: RunEvent, snapshot: bool) -> Result<()> {
        if self.poisoned {
            return Err(anyhow!(
                "the run's event log failed earlier; nothing more is written"
            ));
        }
        let seq = self.seq + 1;
        let line = EventLine {
            seq,
            ts: now(),
            event,
        };
        let mut text = serde_json::to_string(&line)?;
        text.push('\n');
        if let Err(error) = self
            .events
            .write_all(text.as_bytes())
            .and_then(|()| self.events.flush())
        {
            self.poisoned = true;
            return Err(error).context("failed to append to events.jsonl");
        }
        self.seq = seq;
        if snapshot {
            self.report.seq = seq;
            write_json_atomic(&self.dir.join("run.json"), &self.report)?;
        }
        Ok(())
    }

    pub fn suite_started(&mut self, suite: usize) -> Result<()> {
        if self.attempt.is_some() || self.step.is_some() {
            return Err(anyhow!("an attempt or step is still running"));
        }
        if let Some(current) = self.current_suite {
            return Err(anyhow!("suite {current} is still running"));
        }
        let status = self
            .report
            .suites
            .get(suite)
            .ok_or_else(|| anyhow!("no suite {suite} in the plan"))?
            .status;
        if status != Status::Pending {
            return Err(anyhow!("suite {suite} already ran"));
        }
        self.suite_starts[suite] = Some(Instant::now());
        self.current_suite = Some(suite);
        let report = &mut self.report.suites[suite];
        report.status = Status::Running;
        report.started_at = Some(now());
        let event = RunEvent::SuiteStarted {
            suite_id: report.id.clone(),
            suite: report.suite.clone(),
            path: report.path.clone(),
        };
        self.commit(event, true)
    }

    /// Record the app session the running suite runs against.
    pub fn set_suite_app(&mut self, app: Value) -> Result<()> {
        let suite = self
            .current_suite
            .ok_or_else(|| anyhow!("no suite is running"))?;
        let report = &mut self.report.suites[suite];
        report.app = Some(app.clone());
        let event = RunEvent::SuiteApp {
            suite_id: report.id.clone(),
            app,
        };
        self.commit(event, true)
    }

    /// Finish a suite: `failed` on `error` or any unexpected test, `interrupted` if any
    /// test never finished, `skipped` if every test was skipped, else `passed`.
    pub fn suite_finished(&mut self, suite: usize, error: Option<ErrorInfo>) -> Result<()> {
        if self.attempt.is_some() || self.step.is_some() {
            return Err(anyhow!("finish the running attempt or step first"));
        }
        if self.current_suite != Some(suite) {
            return Err(anyhow!("suite {suite} is not running"));
        }
        let duration_ms = self.suite_starts[suite].map(elapsed_ms).unwrap_or(0);
        let report = &mut self.report.suites[suite];
        report.status = suite_status(report, error.is_some());
        report.duration_ms = Some(duration_ms);
        report.error = error.clone();
        if self.current_suite == Some(suite) {
            self.current_suite = None;
        }
        let event = RunEvent::SuiteFinished {
            suite_id: report.id.clone(),
            status: report.status,
            duration_ms,
            error,
        };
        self.commit(event, true)
    }

    /// Start an attempt (0 = first run, 1 = first retry) and return its artifact folder.
    pub fn test_started(&mut self, suite: usize, test: usize, attempt: u32) -> Result<PathBuf> {
        if self.attempt.is_some() || self.step.is_some() {
            return Err(anyhow!("an attempt or step is already running"));
        }
        self.check_test(suite, test)?;
        let report = &self.report.suites[suite].tests[test];
        let expected = match report.status {
            Status::Pending => 0,
            // Finished with `will_retry`: waiting for its next attempt.
            Status::Running if report.outcome.is_none() => report.attempts.len() as u32,
            _ => return Err(anyhow!("test `{}` already finished", report.name)),
        };
        if attempt != expected {
            return Err(anyhow!(
                "test `{}` expects attempt {expected}, not {attempt}",
                report.name
            ));
        }
        let suite_report = &mut self.report.suites[suite];
        let suite_id = suite_report.id.clone();
        let report = &mut suite_report.tests[test];
        let relative = PathBuf::from("tests")
            .join(&report.id)
            .join(format!("attempt-{attempt}"));
        let dir = self.dir.join(&relative);
        fs::create_dir_all(&dir)?;
        let started_at = now();
        report.status = Status::Running;
        report.attempts.push(AttemptSummary {
            attempt,
            status: Status::Running,
            started_at: started_at.clone(),
            duration_ms: None,
            error: None,
            dir: rel(&relative),
        });
        let event = RunEvent::TestStarted {
            suite_id,
            test_id: report.id.clone(),
            title: report.title.clone(),
            attempt,
        };
        self.attempt = Some(ActiveAttempt {
            suite,
            test,
            attempt,
            dir: dir.clone(),
            started_at,
            start: Instant::now(),
            steps: Vec::new(),
            attachments: Vec::new(),
            needs_review: false,
        });
        self.commit(event, true)?;
        Ok(dir)
    }

    fn scope(&self) -> Result<Scope> {
        if let Some(active) = &self.attempt {
            let suite = &self.report.suites[active.suite];
            return Ok(Scope {
                suite: active.suite,
                suite_id: suite.id.clone(),
                test_id: Some(suite.tests[active.test].id.clone()),
                attempt: Some(active.attempt),
            });
        }
        let suite = self
            .current_suite
            .ok_or_else(|| anyhow!("no suite or attempt is running"))?;
        Ok(Scope {
            suite,
            suite_id: self.report.suites[suite].id.clone(),
            test_id: None,
            attempt: None,
        })
    }

    fn next_step_index(&self, scope: &Scope) -> usize {
        match &self.attempt {
            Some(active) => active.steps.len(),
            None => self.report.suites[scope.suite].hooks.len(),
        }
    }

    /// Start a step in the running attempt, or a suite hook step when no attempt runs.
    /// Returns its index within that scope.
    pub fn step_started(&mut self, phase: Phase, title: &str, action: &str) -> Result<usize> {
        if self.step.is_some() {
            return Err(anyhow!("a step is already running"));
        }
        let scope = self.scope()?;
        let suite_phase = matches!(phase, Phase::BeforeAll | Phase::AfterAll);
        if suite_phase == scope.test_id.is_some() {
            return Err(anyhow!(
                "{phase:?} steps belong to {}",
                if suite_phase {
                    "the suite, outside any attempt"
                } else {
                    "a test attempt"
                }
            ));
        }
        let index = self.next_step_index(&scope);
        let event = RunEvent::StepStarted {
            suite_id: scope.suite_id,
            test_id: scope.test_id,
            attempt: scope.attempt,
            index,
            phase,
            title: title.to_string(),
            action: action.to_string(),
        };
        self.commit(event, false)?;
        self.step = Some(ActiveStep {
            index,
            phase,
            title: title.to_string(),
            action: action.to_string(),
            start: Instant::now(),
            ended_ms: None,
            started_at: now(),
            attachments: Vec::new(),
        });
        Ok(index)
    }

    /// Stop the running step's clock, so evidence captured before [`Self::step_finished`] does
    /// not count toward its duration.
    pub fn step_ended(&mut self) {
        if let Some(step) = self.step.as_mut() {
            step.ended_ms.get_or_insert_with(|| elapsed_ms(step.start));
        }
    }

    /// Finish the running step. Attachments added while it ran belong to it.
    pub fn step_finished(
        &mut self,
        status: Status,
        soft: bool,
        error: Option<ErrorInfo>,
    ) -> Result<()> {
        let scope = self.scope()?;
        let active = self
            .step
            .take()
            .ok_or_else(|| anyhow!("no step is running"))?;
        let duration_ms = active.ended_ms.unwrap_or_else(|| elapsed_ms(active.start));
        let index = active.index;
        let step = StepReport {
            index,
            phase: active.phase,
            title: active.title,
            action: active.action,
            status,
            soft,
            started_at: active.started_at,
            duration_ms,
            error: error.clone(),
            attachments: active.attachments,
        };
        // Suite hook steps live in run.json, so they need a snapshot; attempt steps land
        // in attempt.json when the attempt finishes.
        let snapshot = match &mut self.attempt {
            Some(attempt) => {
                attempt.steps.push(step);
                false
            }
            None => {
                self.report.suites[scope.suite].hooks.push(step);
                true
            }
        };
        self.commit(
            RunEvent::StepFinished {
                suite_id: scope.suite_id,
                test_id: scope.test_id,
                attempt: scope.attempt,
                index,
                status,
                soft,
                duration_ms,
                error,
            },
            snapshot,
        )
    }

    /// Write bytes into the current attempt (or suite hook) folder and record them as an
    /// attachment of the running step, else of the attempt. Suite hook attachments need
    /// a running step.
    pub fn attach(
        &mut self,
        name: &str,
        content_type: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<Attachment> {
        if !is_contained(file_name) {
            return Err(anyhow!(
                "attachment file name `{file_name}` must be a relative path inside the run"
            ));
        }
        let scope = self.scope()?;
        let step = self.step.as_ref().map(|step| step.index);
        let base = match &self.attempt {
            Some(active) => active.dir.clone(),
            None if step.is_some() => self.dir.join("suites").join(&scope.suite_id),
            None => {
                return Err(anyhow!(
                    "suite hook attachments must be added while a hook step runs"
                ))
            }
        };
        let path = base.join(file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes).with_context(|| format!("failed to write `{}`", path.display()))?;
        let attachment = Attachment {
            name: name.to_string(),
            content_type: content_type.to_string(),
            path: rel(path.strip_prefix(&self.dir).unwrap_or(&path)),
        };
        self.commit(
            RunEvent::Attachment {
                suite_id: scope.suite_id,
                test_id: scope.test_id,
                attempt: scope.attempt,
                step,
                attachment: attachment.clone(),
            },
            false,
        )?;
        match (&mut self.step, &mut self.attempt) {
            (Some(step), _) => step.attachments.push(attachment.clone()),
            (None, Some(attempt)) => attempt.attachments.push(attachment.clone()),
            (None, None) => unreachable!("checked above"),
        }
        Ok(attachment)
    }

    fn gate_scope(&self) -> Result<(Scope, usize)> {
        let step = self
            .step
            .as_ref()
            .ok_or_else(|| anyhow!("gates belong to a running step"))?
            .index;
        Ok((self.scope()?, step))
    }

    /// A manual gate in the running step is waiting for a human.
    pub fn gate_waiting(&mut self, gate: Value) -> Result<()> {
        let (scope, step) = self.gate_scope()?;
        self.commit(
            RunEvent::GateWaiting {
                suite_id: scope.suite_id,
                test_id: scope.test_id,
                attempt: scope.attempt,
                step,
                gate,
            },
            false,
        )
    }

    pub fn gate_resolved(&mut self, response: Value) -> Result<()> {
        let (scope, step) = self.gate_scope()?;
        self.commit(
            RunEvent::GateResolved {
                suite_id: scope.suite_id,
                test_id: scope.test_id,
                attempt: scope.attempt,
                step,
                response,
            },
            false,
        )
    }

    /// Make the running test's outcome unexpected even if it was expected to fail, e.g. because
    /// it wrote a baseline that someone must review.
    pub fn require_review(&mut self) {
        if let Some(active) = &mut self.attempt {
            active.needs_review = true;
        }
    }

    /// Finish the running attempt. With `will_retry`, the test stays running.
    pub fn test_finished(
        &mut self,
        status: Status,
        error: Option<ErrorInfo>,
        will_retry: bool,
    ) -> Result<()> {
        if self.step.is_some() {
            return Err(anyhow!("finish the running step first"));
        }
        let active = self
            .attempt
            .as_ref()
            .ok_or_else(|| anyhow!("no attempt is running"))?;
        let duration_ms = elapsed_ms(active.start);
        let test = &self.report.suites[active.suite].tests[active.test];
        let attempt_report = AttemptReport {
            schema_version: RUN_SCHEMA_VERSION,
            test_id: test.id.clone(),
            title: test.title.clone(),
            attempt: active.attempt,
            status,
            started_at: active.started_at.clone(),
            duration_ms,
            error: error.clone(),
            steps: active.steps.clone(),
            attachments: active.attachments.clone(),
        };
        // Keep the attempt active until its report is on disk, so a failure can be retried.
        write_json_atomic(&active.dir.join("attempt.json"), &attempt_report)?;
        let active = self.attempt.take().expect("checked above");

        let suite_id = self.report.suites[active.suite].id.clone();
        let report = &mut self.report.suites[active.suite].tests[active.test];
        if let Some(summary) = report.attempts.last_mut() {
            summary.status = status;
            summary.duration_ms = Some(duration_ms);
            summary.error = error.clone();
        }
        report.duration_ms = Some(report.duration_ms.unwrap_or(0).saturating_add(duration_ms));
        let outcome = if will_retry {
            None
        } else {
            report.status = status;
            let outcome = if active.needs_review {
                Outcome::Unexpected
            } else {
                outcome_for(report)
            };
            report.outcome = Some(outcome);
            Some(outcome)
        };
        let event = RunEvent::TestFinished {
            suite_id,
            test_id: report.id.clone(),
            attempt: active.attempt,
            status,
            duration_ms,
            error,
            will_retry,
            outcome,
        };
        self.refresh_totals();
        self.commit(event, true)
    }

    /// Record a `skip`/`fixme` test without running it.
    pub fn test_skipped(&mut self, suite: usize, test: usize) -> Result<()> {
        if self.attempt.is_some() {
            return Err(anyhow!("an attempt is still running"));
        }
        self.check_test(suite, test)?;
        if self.report.suites[suite].tests[test].status != Status::Pending {
            return Err(anyhow!("only a pending test can be skipped"));
        }
        let suite_id = self.report.suites[suite].id.clone();
        let report = &mut self.report.suites[suite].tests[test];
        report.status = Status::Skipped;
        report.outcome = Some(Outcome::Skipped);
        let event = RunEvent::TestFinished {
            suite_id,
            test_id: report.id.clone(),
            attempt: 0,
            status: Status::Skipped,
            duration_ms: 0,
            error: None,
            will_retry: false,
            outcome: Some(Outcome::Skipped),
        };
        self.refresh_totals();
        self.commit(event, true)
    }

    fn refresh_totals(&mut self) {
        self.report.totals = totals_for(&self.report.suites);
    }

    /// Tests run inside their suite, between `suite_started` and `suite_finished`.
    fn check_test(&self, suite: usize, test: usize) -> Result<()> {
        if self.current_suite != Some(suite) {
            return Err(anyhow!("suite {suite} is not running"));
        }
        if test >= self.report.suites[suite].tests.len() {
            return Err(anyhow!("no test {test} in suite {suite}"));
        }
        Ok(())
    }

    /// Finish the run, prune old runs and return the final report. The run is
    /// `interrupted` when asked (bail, Ctrl+C) or when work never finished without
    /// any failure to report.
    pub fn finish(mut self, interrupted: bool) -> Result<RunReport> {
        self.finish_inner(interrupted)?;
        Ok(self.report.clone())
    }

    fn finish_inner(&mut self, interrupted: bool) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        if self.poisoned {
            self.finished = true;
            return Ok(());
        }
        if self.step.is_some() {
            self.step_finished(Status::Interrupted, false, None)?;
        }
        if self.attempt.is_some() {
            self.test_finished(Status::Interrupted, None, false)?;
        }
        let running: Vec<usize> = (0..self.report.suites.len())
            .filter(|&i| self.report.suites[i].status == Status::Running)
            .collect();
        for index in running {
            let duration_ms = self.suite_starts[index].map(elapsed_ms).unwrap_or(0);
            let suite = &mut self.report.suites[index];
            suite.status = Status::Interrupted;
            suite.duration_ms = Some(duration_ms);
            let event = RunEvent::SuiteFinished {
                suite_id: suite.id.clone(),
                status: Status::Interrupted,
                duration_ms,
                error: None,
            };
            self.commit(event, false)?;
        }
        self.current_suite = None;
        self.refresh_totals();
        let incomplete = self.report.totals.not_run > 0
            || self
                .report
                .suites
                .iter()
                .any(|suite| suite.status == Status::Interrupted);
        let failed = self.report.totals.failed > 0
            || self.report.suites.iter().any(|suite| suite.error.is_some());
        let status = if interrupted || (incomplete && !failed) {
            Status::Interrupted
        } else if failed {
            Status::Failed
        } else {
            Status::Passed
        };
        let duration_ms = elapsed_ms(self.start);
        self.report.status = status;
        self.report.ended_at = Some(now());
        self.report.duration_ms = Some(duration_ms);
        let totals = self.report.totals.clone();
        let logged = self.seq;
        let result = self.commit(
            RunEvent::RunFinished {
                status,
                duration_ms,
                totals,
            },
            true,
        );
        if self.seq > logged {
            // The outcome is logged; never re-finish it as interrupted, only re-save it.
            self.finished = true;
            self.final_snapshot_pending = result.is_err();
        }
        result?;
        // Pruning is housekeeping; failing it must not fail the run.
        let _ = prune_runs(&self.runs_root, self.keep, &self.report.run_id);
        Ok(())
    }
}

impl Drop for RunWriter {
    fn drop(&mut self) {
        if self.final_snapshot_pending {
            let _ = write_json_atomic(&self.dir.join("run.json"), &self.report);
            return;
        }
        // A runner that bails out with `?` still leaves a readable, finished run.
        let _ = self.finish_inner(true);
    }
}

fn suite_status(report: &SuiteReport, errored: bool) -> Status {
    let outcomes = || report.tests.iter().map(|test| test.outcome);
    if errored || outcomes().any(|outcome| outcome == Some(Outcome::Unexpected)) {
        Status::Failed
    } else if outcomes().any(|outcome| outcome.is_none()) {
        Status::Interrupted
    } else if outcomes().all(|outcome| outcome == Some(Outcome::Skipped)) {
        Status::Skipped
    } else {
        Status::Passed
    }
}

fn totals_for(suites: &[SuiteReport]) -> Totals {
    let mut totals = Totals::default();
    for test in suites.iter().flat_map(|suite| &suite.tests) {
        totals.tests += 1;
        match test.outcome {
            Some(Outcome::Expected) => totals.passed += 1,
            Some(Outcome::Unexpected) => totals.failed += 1,
            Some(Outcome::Flaky) => totals.flaky += 1,
            Some(Outcome::Skipped) => totals.skipped += 1,
            None => totals.not_run += 1,
        }
    }
    totals
}

fn outcome_for(report: &TestReport) -> Outcome {
    if report.status == Status::Skipped {
        return Outcome::Skipped;
    }
    if report.status != report.expected_status {
        return Outcome::Unexpected;
    }
    let retried_after_unexpected = report
        .attempts
        .iter()
        .rev()
        .skip(1)
        .any(|attempt| attempt.status != report.expected_status);
    if retried_after_unexpected {
        Outcome::Flaky
    } else {
        Outcome::Expected
    }
}

fn reserve_run_dir(root: &Path) -> Result<(String, PathBuf)> {
    let base = new_run_id();
    for suffix in 0..100 {
        let run_id = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix:02}")
        };
        let dir = root.join(&run_id);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((run_id, dir)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to create `{}`", dir.display()))
            }
        }
    }
    Err(anyhow!(
        "could not reserve a run folder under `{}`",
        root.display()
    ))
}

/// Run folder names under `root`, oldest first.
pub fn run_ids(root: &Path) -> Result<Vec<String>> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && is_run_id(&name) {
            ids.push(name);
        }
    }
    ids.sort();
    Ok(ids)
}

/// Delete the oldest runs so at most `keep` remain. Never deletes `current`, a run whose
/// owner is still alive, or an unreadable folder young enough to still be starting.
pub fn prune_runs(root: &Path, keep: usize, current: &str) -> Result<Vec<String>> {
    let ids = run_ids(root)?;
    let excess = ids.len().saturating_sub(keep);
    let mut removed = Vec::new();
    for id in ids.into_iter().take(excess) {
        let dir = root.join(&id);
        let protected = id == current
            || match read_run(&dir) {
                Ok(report) => report.status == Status::Running,
                Err(_) => fs::metadata(&dir)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_none_or(|age| age < PRUNE_GRACE),
            };
        if !protected && fs::remove_dir_all(&dir).is_ok() {
            removed.push(id);
        }
    }
    Ok(removed)
}

/// Resolve `latest` (the newest run folder) or a run id or unique prefix.
pub fn resolve_run(root: &Path, selector: Option<&str>) -> Result<PathBuf> {
    let ids = run_ids(root)?;
    match selector {
        None | Some("latest") => ids
            .last()
            .map(|id| root.join(id))
            .ok_or_else(|| anyhow!("no drill runs found under `{}`", root.display())),
        Some(selector) => {
            if ids.iter().any(|id| id == selector) {
                return Ok(root.join(selector));
            }
            let matches: Vec<&String> = ids.iter().filter(|id| id.starts_with(selector)).collect();
            match matches.as_slice() {
                [id] => Ok(root.join(id)),
                [] => Err(anyhow!("no drill run matches `{selector}`")),
                many => Err(anyhow!(
                    "`{selector}` matches {} runs; use more of the id",
                    many.len()
                )),
            }
        }
    }
}

/// Read a run report. A `running` run whose owner process is gone reads as
/// interrupted, along with its running suites, tests and attempts.
pub fn read_run(dir: &Path) -> Result<RunReport> {
    read_run_with(dir, process_alive)
}

fn read_run_with(dir: &Path, mut alive: impl FnMut(u32) -> bool) -> Result<RunReport> {
    let path = dir.join("run.json");
    let mut report = parse_run(&path)?;
    // Check liveness once, then re-read: a runner seen dead has made its last write,
    // so only a report that is still running afterwards was really interrupted.
    if report.status != Status::Running || alive(report.pid) {
        return Ok(report);
    }
    report = parse_run(&path)?;
    if report.status == Status::Running {
        report.status = Status::Interrupted;
        for suite in &mut report.suites {
            if suite.status == Status::Running {
                suite.status = Status::Interrupted;
            }
            for test in &mut suite.tests {
                if test.status == Status::Running {
                    test.status = Status::Interrupted;
                    test.outcome = Some(Outcome::Unexpected);
                }
                for attempt in &mut test.attempts {
                    if attempt.status == Status::Running {
                        attempt.status = Status::Interrupted;
                    }
                }
            }
        }
        report.totals = totals_for(&report.suites);
    }
    Ok(report)
}

pub fn read_attempt(run_dir: &Path, summary: &AttemptSummary) -> Result<AttemptReport> {
    if !is_contained(&summary.dir) {
        return Err(anyhow!(
            "attempt folder `{}` is not inside the run",
            summary.dir
        ));
    }
    let path = run_dir.join(&summary.dir).join("attempt.json");
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid `{}`", path.display()))
}

fn parse_run(path: &Path) -> Result<RunReport> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let report: RunReport = serde_json::from_str(&text)
        .with_context(|| format!("`{}` is not a drill run report", path.display()))?;
    if report.schema_version > RUN_SCHEMA_VERSION {
        return Err(anyhow!(
            "`{}` uses run schema {}; this auditaur reads up to {RUN_SCHEMA_VERSION}",
            path.display(),
            report.schema_version
        ));
    }
    Ok(report)
}

fn process_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    // If the check itself fails, don't claim the run died.
    crate::commands::workflow::process_exists(pid).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::drills::{load, select, Filters};

    const SUITE: &str = r##"{
        "retries": 1,
        "beforeEach": [{ "click": "#reset" }],
        "tests": [
            { "name": "passes", "steps": [{ "click": "#a" }] },
            { "name": "fails", "steps": [{ "click": "#b" }] },
            { "name": "flaky", "steps": [{ "click": "#c" }] },
            { "name": "skipped", "skip": "not today", "steps": [{ "click": "#d" }] },
            { "name": "fixme", "fixme": true },
            { "name": "known bug", "expectFail": "issue 12", "steps": [{ "click": "#e" }] },
            { "name": "bug fixed", "expectFail": true, "steps": [{ "click": "#f" }] }
        ]
    }"##;

    fn validator(def: &str) -> jsonschema::Validator {
        let schema: Value = serde_json::from_str(RUN_SCHEMA).unwrap();
        let schema = if def == "run" {
            schema
        } else {
            serde_json::json!({ "$defs": schema["$defs"], "$ref": format!("#/$defs/{def}") })
        };
        jsonschema::draft202012::new(&schema).unwrap()
    }

    fn assert_valid(validator: &jsonschema::Validator, value: &Value, what: &str) {
        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{what} violates the schema: {errors:#?}");
    }

    /// One attempt: a beforeEach step and a test step, failing the test step when asked.
    fn attempt(writer: &mut RunWriter, suite: usize, test: usize, n: u32, pass: bool) -> Status {
        writer.test_started(suite, test, n).unwrap();
        writer
            .step_started(Phase::BeforeEach, "click #reset", "click")
            .unwrap();
        writer.step_finished(Status::Passed, false, None).unwrap();
        let index = writer.step_started(Phase::Test, "click", "click").unwrap();
        writer
            .gate_waiting(serde_json::json!({ "kind": "confirm" }))
            .unwrap();
        writer
            .gate_resolved(serde_json::json!({ "ok": true }))
            .unwrap();
        let error = (!pass).then(|| ErrorInfo {
            step: Some(index),
            expected: Some(serde_json::json!("visible")),
            actual: Some(serde_json::json!("hidden")),
            ..ErrorInfo::new("locator not visible")
        });
        let status = if pass { Status::Passed } else { Status::Failed };
        if !pass {
            writer
                .attach("screenshot", "image/png", "failure.png", b"png")
                .unwrap();
        }
        writer.step_finished(status, false, error).unwrap();
        status
    }

    #[test]
    fn writes_a_schema_valid_run_with_every_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let drills = workspace.join(".auditaur/drills");
        fs::create_dir_all(&drills).unwrap();
        let file = drills.join("mixed.drill.json");
        fs::write(&file, SUITE).unwrap();
        let loaded = load(&[file]);
        assert!(loaded.invalid.is_empty(), "{:?}", loaded.invalid);
        let plan = select(&loaded.suites, &Filters::default());

        let mut writer = RunWriter::create(
            RunOptions {
                workspace: workspace.clone(),
                invocation: Invocation::default(),
                keep: DEFAULT_KEEP_RUNS,
            },
            &plan,
        )
        .unwrap();
        let run_dir = writer.dir().to_path_buf();
        // Readers see the whole plan before anything runs.
        let pending = read_run(&run_dir).unwrap();
        assert_eq!(pending.status, Status::Running);
        assert_eq!(pending.totals.not_run, 7);
        assert!(pending.suites[0]
            .tests
            .iter()
            .all(|t| t.status == Status::Pending));
        assert_eq!(pending.suites[0].path, ".auditaur/drills/mixed.drill.json");
        assert_eq!(
            pending.suites[0].tests[0].id,
            test_id(".auditaur/drills/mixed.drill.json", "passes")
        );

        writer.suite_started(0).unwrap();
        assert!(writer.suite_started(0).is_err());
        assert!(writer.test_started(0, 0, 1).is_err());
        assert!(writer.test_started(0, 99, 0).is_err());
        assert!(writer.gate_waiting(serde_json::json!({})).is_err());
        assert!(writer.attach("log", "text/plain", "x.log", b"").is_err());
        writer
            .set_suite_app(serde_json::json!({ "pid": 1 }))
            .unwrap();
        // A beforeAll hook with a step attachment, outside any attempt.
        writer
            .step_started(Phase::BeforeAll, "launch", "evaluate")
            .unwrap();
        let log = writer
            .attach("log", "text/plain", "launch.log", b"ok")
            .unwrap();
        assert_eq!(
            log.path,
            format!(
                "suites/{}/launch.log",
                suite_id(".auditaur/drills/mixed.drill.json")
            )
        );
        assert!(writer.attach("log", "text/plain", "../x.log", b"").is_err());
        assert!(writer.step_started(Phase::Test, "x", "click").is_err());
        writer.step_finished(Status::Passed, false, None).unwrap();
        // passes
        let status = attempt(&mut writer, 0, 0, 0, true);
        writer.test_finished(status, None, false).unwrap();
        assert!(writer.test_started(0, 0, 1).is_err());
        assert!(writer.test_skipped(0, 0).is_err());
        // fails twice (retries: 1)
        let status = attempt(&mut writer, 0, 1, 0, false);
        writer
            .test_finished(status, Some(ErrorInfo::new("first")), true)
            .unwrap();
        assert!(writer.test_started(0, 1, 0).is_err());
        let status = attempt(&mut writer, 0, 1, 1, false);
        writer
            .test_finished(status, Some(ErrorInfo::new("again")), false)
            .unwrap();
        // flaky: fails, then passes
        let status = attempt(&mut writer, 0, 2, 0, false);
        writer
            .test_finished(status, Some(ErrorInfo::new("blip")), true)
            .unwrap();
        let status = attempt(&mut writer, 0, 2, 1, true);
        writer.test_finished(status, None, false).unwrap();
        writer.test_skipped(0, 3).unwrap();
        writer.test_skipped(0, 4).unwrap();
        // expectFail that fails is expected; one that passes is unexpected.
        let status = attempt(&mut writer, 0, 5, 0, false);
        writer
            .test_finished(status, Some(ErrorInfo::new("known")), false)
            .unwrap();
        let status = attempt(&mut writer, 0, 6, 0, true);
        writer.test_finished(status, None, false).unwrap();
        writer
            .step_started(Phase::AfterAll, "close", "evaluate")
            .unwrap();
        writer.step_finished(Status::Passed, false, None).unwrap();
        writer.suite_finished(0, None).unwrap();
        assert!(writer.test_started(0, 0, 0).is_err());
        assert!(writer.gate_waiting(serde_json::json!({})).is_err());
        let report = writer.finish(false).unwrap();

        let hooks: Vec<Phase> = report.suites[0].hooks.iter().map(|h| h.phase).collect();
        assert_eq!(hooks, [Phase::BeforeAll, Phase::AfterAll]);
        assert!(run_dir
            .join(&report.suites[0].hooks[0].attachments[0].path)
            .is_file());
        let tests = &report.suites[0].tests;
        let outcomes: Vec<Option<Outcome>> = tests.iter().map(|t| t.outcome).collect();
        assert_eq!(
            outcomes,
            [
                Some(Outcome::Expected),
                Some(Outcome::Unexpected),
                Some(Outcome::Flaky),
                Some(Outcome::Skipped),
                Some(Outcome::Skipped),
                Some(Outcome::Expected),
                Some(Outcome::Unexpected),
            ]
        );
        assert_eq!(tests[5].expected_status, Status::Failed);
        assert_eq!(tests[1].attempts.len(), 2);
        assert_eq!(
            report.totals,
            Totals {
                tests: 7,
                passed: 2,
                failed: 2,
                flaky: 1,
                skipped: 2,
                not_run: 0
            }
        );
        assert_eq!(report.status, Status::Failed);
        assert_eq!(report.suites[0].status, Status::Failed);

        // On disk matches memory and the published schema.
        let run_json: Value =
            serde_json::from_str(&fs::read_to_string(run_dir.join("run.json")).unwrap()).unwrap();
        assert_valid(&validator("run"), &run_json, "run.json");
        assert_eq!(read_run(&run_dir).unwrap().totals, report.totals);

        let attempts = validator("attempt");
        let mut attempt_files = 0;
        for test in tests {
            for summary in &test.attempts {
                let value: Value = serde_json::from_str(
                    &fs::read_to_string(run_dir.join(&summary.dir).join("attempt.json")).unwrap(),
                )
                .unwrap();
                assert_valid(&attempts, &value, &summary.dir);
                let parsed = read_attempt(&run_dir, summary).unwrap();
                assert_eq!(parsed.steps.len(), 2);
                assert_eq!(parsed.steps[0].phase, Phase::BeforeEach);
                if parsed.status == Status::Failed {
                    let shot = &parsed.steps[1].attachments[0];
                    assert!(run_dir.join(&shot.path).is_file(), "{}", shot.path);
                }
                attempt_files += 1;
            }
        }
        assert_eq!(attempt_files, 7);

        let events = validator("event");
        let lines = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let mut types = Vec::new();
        for (index, line) in lines.lines().enumerate() {
            let value: Value = serde_json::from_str(line).unwrap();
            assert_valid(&events, &value, line);
            assert_eq!(value["seq"], index + 1);
            let parsed: EventLine = serde_json::from_str(line).unwrap();
            assert_eq!(parsed.seq as usize, index + 1);
            types.push(value["type"].as_str().unwrap().to_string());
        }
        assert_eq!(read_run(&run_dir).unwrap().seq as usize, types.len());
        assert_eq!(types.first().map(String::as_str), Some("runStarted"));
        assert_eq!(types.last().map(String::as_str), Some("runFinished"));
        for expected in [
            "suiteStarted",
            "suiteApp",
            "testStarted",
            "stepStarted",
            "gateWaiting",
            "gateResolved",
            "stepFinished",
            "attachment",
            "testFinished",
            "suiteFinished",
        ] {
            assert!(types.iter().any(|t| t == expected), "missing {expected}");
        }

        let root = workspace.join(RUNS_DIR);
        assert_eq!(fs::read_to_string(root.join(".gitignore")).unwrap(), "*\n");
        assert_eq!(resolve_run(&root, None).unwrap(), run_dir);
        assert_eq!(resolve_run(&root, Some(&report.run_id)).unwrap(), run_dir);
        assert_eq!(
            resolve_run(&root, Some(&report.run_id[..8])).unwrap(),
            run_dir
        );
        assert!(resolve_run(&root, Some("1999")).is_err());
        let leftovers: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn evidence_captured_after_step_ended_is_not_timed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.drill.json");
        fs::write(
            &file,
            r##"{ "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }] }"##,
        )
        .unwrap();
        let loaded = load(&[file]);
        let plan = select(&loaded.suites, &Filters::default());
        let mut writer = RunWriter::create(
            RunOptions {
                workspace: dir.path().to_path_buf(),
                invocation: Invocation::default(),
                keep: 5,
            },
            &plan,
        )
        .unwrap();
        writer.suite_started(0).unwrap();
        writer.test_started(0, 0, 0).unwrap();
        writer.step_started(Phase::Test, "click", "click").unwrap();
        writer.step_ended();
        std::thread::sleep(std::time::Duration::from_millis(300));
        writer.step_ended();
        writer
            .attach("trace", "image/png", "step-0-trace.png", b"png")
            .unwrap();
        writer.step_finished(Status::Passed, false, None).unwrap();
        writer.test_finished(Status::Passed, None, false).unwrap();
        let summary = writer.report().suites[0].tests[0].attempts[0].clone();
        let attempt = read_attempt(writer.dir(), &summary).unwrap();
        assert!(attempt.steps[0].duration_ms < 300, "{:?}", attempt.steps[0]);
        assert_eq!(attempt.steps[0].attachments.len(), 1);
    }

    #[test]
    fn dropped_writer_leaves_an_interrupted_run() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.drill.json");
        fs::write(&file, r##"{ "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }, { "name": "u", "steps": [{ "click": "#a" }] }] }"##).unwrap();
        let loaded = load(&[file]);
        let plan = select(&loaded.suites, &Filters::default());
        let run_dir = {
            let mut writer = RunWriter::create(
                RunOptions {
                    workspace: dir.path().to_path_buf(),
                    invocation: Invocation::default(),
                    keep: 5,
                },
                &plan,
            )
            .unwrap();
            writer.suite_started(0).unwrap();
            writer.test_started(0, 0, 0).unwrap();
            writer.step_started(Phase::Test, "click", "click").unwrap();
            writer.dir().to_path_buf()
        };
        let report = read_run(&run_dir).unwrap();
        assert_eq!(report.status, Status::Interrupted);
        assert_eq!(report.suites[0].status, Status::Interrupted);
        assert_eq!(report.suites[0].tests[0].status, Status::Interrupted);
        assert_eq!(report.suites[0].tests[0].outcome, Some(Outcome::Unexpected));
        assert_eq!(report.suites[0].tests[1].status, Status::Pending);
        assert_eq!(report.totals.not_run, 1);
        let lines = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let tail: Vec<Value> = lines
            .lines()
            .rev()
            .take(4)
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let kinds: Vec<&str> = tail
            .iter()
            .rev()
            .map(|v| v["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "stepFinished",
                "testFinished",
                "suiteFinished",
                "runFinished"
            ]
        );
        assert_eq!(tail[3]["status"], "interrupted");
        let summary = &report.suites[0].tests[0].attempts[0];
        let attempt = read_attempt(&run_dir, summary).unwrap();
        assert_eq!(attempt.steps.len(), 1);
        assert_eq!(attempt.steps[0].status, Status::Interrupted);
        assert_eq!(attempt.steps[0].title, "click");
        let run_json: Value =
            serde_json::from_str(&fs::read_to_string(run_dir.join("run.json")).unwrap()).unwrap();
        assert_valid(&validator("run"), &run_json, "run.json");
    }

    #[test]
    fn incomplete_suite_without_failures_is_interrupted_not_passed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.drill.json");
        fs::write(&file, r##"{ "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }, { "name": "u", "steps": [{ "click": "#a" }] }] }"##).unwrap();
        let loaded = load(&[file]);
        let plan = select(&loaded.suites, &Filters::default());
        let mut writer = RunWriter::create(
            RunOptions {
                workspace: dir.path().to_path_buf(),
                invocation: Invocation::default(),
                keep: 5,
            },
            &plan,
        )
        .unwrap();
        writer.suite_started(0).unwrap();
        let status = attempt(&mut writer, 0, 0, 0, true);
        writer.test_finished(status, None, false).unwrap();
        writer.suite_finished(0, None).unwrap();
        assert_eq!(writer.report().suites[0].status, Status::Interrupted);
        let run_dir = writer.dir().to_path_buf();
        let report = writer.finish(false).unwrap();
        assert_eq!(report.status, Status::Interrupted);
        assert_eq!(report.totals.passed, 1);
        assert_eq!(report.totals.not_run, 1);
        assert_eq!(read_run(&run_dir).unwrap().status, Status::Interrupted);
    }

    fn dead_pid() -> u32 {
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", "exit"])
                .spawn()
        } else {
            std::process::Command::new("true").spawn()
        }
        .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn stale_running_run_reads_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("20260101-000000-000");
        fs::create_dir_all(&run_dir).unwrap();
        let attempt = serde_json::json!({
            "attempt": 0, "status": "running", "startedAt": "2026-01-01T00:00:00Z",
            "durationMs": null, "error": null, "dir": "tests/fedcba9876543210/attempt-0"
        });
        let test = serde_json::json!({
            "id": "fedcba9876543210", "name": "t", "title": "s > t", "tags": [], "mode": "run",
            "modeReason": null, "expectedStatus": "passed", "status": "running", "outcome": null,
            "durationMs": null, "attempts": [attempt]
        });
        let suite = serde_json::json!({
            "id": "0123456789abcdef", "suite": "s", "path": "s.drill.json", "status": "running",
            "startedAt": null, "durationMs": null, "app": null, "error": null, "hooks": [],
            "tests": [test]
        });
        let mut report = serde_json::json!({
            "schemaVersion": 1, "runId": "20260101-000000-000", "status": "running", "seq": 3,
            "startedAt": "2026-01-01T00:00:00Z", "endedAt": null, "durationMs": null,
            "pid": dead_pid(), "workspace": "/w",
            "invocation": { "args": [], "patterns": [], "grep": null, "grepInvert": null, "tags": [] },
            "environment": { "os": "x", "arch": "y", "auditaurVersion": "0" },
            "totals": { "tests": 0, "passed": 0, "failed": 0, "flaky": 0, "skipped": 0, "notRun": 0 },
            "suites": [suite]
        });
        assert_valid(&validator("run"), &report, "fixture");
        fs::write(run_dir.join("run.json"), report.to_string()).unwrap();
        let stale = read_run(&run_dir).unwrap();
        assert_eq!(stale.status, Status::Interrupted);
        assert_eq!(stale.suites[0].status, Status::Interrupted);
        assert_eq!(stale.suites[0].tests[0].status, Status::Interrupted);
        assert_eq!(
            stale.suites[0].tests[0].attempts[0].status,
            Status::Interrupted
        );
        assert_eq!(stale.totals.failed, 1);

        report["schemaVersion"] = serde_json::json!(RUN_SCHEMA_VERSION + 1);
        fs::write(run_dir.join("run.json"), report.to_string()).unwrap();
        assert!(read_run(&run_dir)
            .unwrap_err()
            .to_string()
            .contains("run schema"));
    }

    #[test]
    fn prune_keeps_newest_and_current() {
        let dir = tempfile::tempdir().unwrap();
        let ids = [
            "20260101-000000-000",
            "20260102-000000-000",
            "20260103-000000-000",
            "20260104-000000-000",
        ];
        for id in ids {
            fs::create_dir_all(dir.path().join(id)).unwrap();
            let report = serde_json::json!({
                "schemaVersion": 1, "runId": id, "status": "passed", "seq": 1,
                "startedAt": "2026-01-01T00:00:00Z", "endedAt": null, "durationMs": null, "pid": 0, "workspace": "/w",
                "invocation": { "args": [], "patterns": [], "grep": null, "grepInvert": null, "tags": [] },
                "environment": { "os": "x", "arch": "y", "auditaurVersion": "0" },
                "totals": { "tests": 0, "passed": 0, "failed": 0, "flaky": 0, "skipped": 0, "notRun": 0 },
                "suites": []
            });
            fs::write(dir.path().join(id).join("run.json"), report.to_string()).unwrap();
        }
        // A live run (this process) and a just-created folder without run.json are kept.
        let live = "20260100-000000-000";
        fs::create_dir_all(dir.path().join(live)).unwrap();
        let mut running: Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(ids[1]).join("run.json")).unwrap(),
        )
        .unwrap();
        running["status"] = "running".into();
        running["pid"] = std::process::id().into();
        fs::write(dir.path().join(live).join("run.json"), running.to_string()).unwrap();
        let starting = "20260100-000000-001";
        fs::create_dir_all(dir.path().join(starting)).unwrap();
        fs::create_dir_all(dir.path().join("not-a-run")).unwrap();
        let removed = prune_runs(dir.path(), 2, ids[0]).unwrap();
        assert_eq!(removed, [ids[1]]);
        assert_eq!(
            run_ids(dir.path()).unwrap(),
            [live, starting, ids[0], ids[2], ids[3]]
        );
        assert!(dir.path().join("not-a-run").is_dir());
        assert!(run_ids(&dir.path().join("missing")).unwrap().is_empty());
    }

    #[test]
    fn run_ids_are_sortable_and_recognized() {
        let id = new_run_id();
        assert!(is_run_id(&id), "{id}");
        assert!(is_run_id(&format!("{id}-02")));
        assert!(!is_run_id(&format!("{id}-2")));
        assert!(!is_run_id(&format!("{id}x")));
        // Collision suffixes sort after their base and before the next millisecond.
        let mut ids = vec![
            "20260101-000000-001",
            "20260101-000000-000-10",
            "20260101-000000-000-02",
            "20260101-000000-000",
        ];
        ids.sort();
        assert_eq!(
            ids,
            [
                "20260101-000000-000",
                "20260101-000000-000-02",
                "20260101-000000-000-10",
                "20260101-000000-001"
            ]
        );
        assert!(!is_run_id("latest.json"));
        assert!(!is_run_id("2026-01-01T00"));
    }

    #[test]
    fn test_ids_are_stable_and_distinct() {
        assert_eq!(test_id("a.drill.json", "x"), test_id("a.drill.json", "x"));
        assert_ne!(test_id("a.drill.json", "x"), test_id("a.drill.json", "y"));
        // The separator keeps `ab`+`c` and `a`+`bc` apart.
        assert_ne!(test_id("ab", "c"), test_id("a", "bc"));
        // Pinned so ids never silently change between releases.
        assert_eq!(test_id("", ""), "af63bd4c8601b7df");
        assert_eq!(test_id("a.drill.json", "x").len(), 16);
    }

    #[test]
    fn contained_paths_reject_escapes() {
        assert!(is_contained("tests/abc/attempt-0"));
        assert!(is_contained("shot.png"));
        for bad in [
            "",
            "../x",
            "a/../../x",
            "/abs",
            "./x",
            "a:b",
            "a//b",
            "...",
            "a/",
        ] {
            assert!(!is_contained(bad), "{bad}");
        }
        #[cfg(windows)]
        assert!(!is_contained(r"C:\x"));
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_waits_for_a_reader_holding_the_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        fs::write(&path, "{}").unwrap();
        // Share read only (no delete), like a naive reader; rename fails until it closes.
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(&path)
            .unwrap();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(reader);
        });
        write_json_atomic(&path, &serde_json::json!({ "ok": true })).unwrap();
        handle.join().unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("ok"));
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_outlasts_an_exclusive_scan_longer_than_a_second() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        fs::write(&path, "{}").unwrap();
        // No sharing at all, like an antivirus scan that holds the file for a while.
        let scanner = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            drop(scanner);
        });
        write_json_atomic(&path, &serde_json::json!({ "ok": true })).unwrap();
        handle.join().unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("ok"));
    }

    #[test]
    fn a_run_is_only_interrupted_if_still_running_after_its_runner_is_seen_dead() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.drill.json");
        fs::write(
            &file,
            r##"{ "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }] }"##,
        )
        .unwrap();
        let loaded = load(&[file]);
        let plan = select(&loaded.suites, &Filters::default());
        let writer = RunWriter::create(
            RunOptions {
                workspace: dir.path().to_path_buf(),
                invocation: Invocation::default(),
                keep: 5,
            },
            &plan,
        )
        .unwrap();
        let run_dir = writer.dir().to_path_buf();

        // Alive at the check, even if it exits right after: one check, no stale verdict.
        let mut checks = 0;
        let live = read_run_with(&run_dir, |_| {
            checks += 1;
            checks == 1
        })
        .unwrap();
        assert_eq!(live.status, Status::Running);
        assert_eq!(checks, 1);

        // Finished between the first read and the liveness check: the re-read wins.
        let mut writer = Some(writer);
        let done = read_run_with(&run_dir, |_| {
            writer.take().unwrap().finish(false).unwrap();
            false
        })
        .unwrap();
        assert!(done.ended_at.is_some(), "{done:?}");
        assert!(done.seq > live.seq);
    }

    #[cfg(windows)]
    #[test]
    fn a_locked_final_snapshot_is_resaved_not_refinished() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.drill.json");
        fs::write(
            &file,
            r##"{ "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }] }"##,
        )
        .unwrap();
        let loaded = load(&[file]);
        let plan = select(&loaded.suites, &Filters::default());
        let writer = RunWriter::create(
            RunOptions {
                workspace: dir.path().to_path_buf(),
                invocation: Invocation::default(),
                keep: 5,
            },
            &plan,
        )
        .unwrap();
        let run_dir = writer.dir().to_path_buf();
        let scanner = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(run_dir.join("run.json"))
            .unwrap();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(LOCKED_FILE_WAIT + Duration::from_millis(500));
            drop(scanner);
        });
        assert!(writer.finish(false).is_err());
        handle.join().unwrap();
        let lines = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let finished: Vec<Value> = lines
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|line| line["type"] == "runFinished")
            .collect();
        assert_eq!(finished.len(), 1, "{lines}");
        let snapshot: Value =
            serde_json::from_str(&fs::read_to_string(run_dir.join("run.json")).unwrap()).unwrap();
        assert_eq!(snapshot["seq"], finished[0]["seq"]);
    }

    #[test]
    fn only_lock_errors_are_retried() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(is_transient_lock(&denied));
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(!is_transient_lock(&missing));
        assert_eq!(
            is_transient_lock(&std::io::Error::from_raw_os_error(32)),
            cfg!(windows)
        );
    }

    #[test]
    fn docs_run_schema_copy_is_identical() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/public/schemas/drill-run.schema.json");
        if let Ok(copy) = fs::read_to_string(docs) {
            assert_eq!(copy.replace("\r\n", "\n"), RUN_SCHEMA.replace("\r\n", "\n"));
        }
    }
}
