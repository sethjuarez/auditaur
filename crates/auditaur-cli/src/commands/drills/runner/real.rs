//! Drives real Tauri apps through the Auditaur drive bridge.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::storage::{RelatedTelemetry, RelatedTelemetryQuery};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::{
    expect::{judge, Inspection, Verdict},
    snapshot::{
        compare_images, decide, line_diff, normalize_aria, Comparison, Decision, Image,
        SnapshotTarget, Tolerance,
    },
    telemetry::{self, Judgement},
    AppDriver, HookResult, Host, RunWriter, StepCtx, StepResult,
};
use crate::{
    commands::{
        debug::{self, DebugStageStatus},
        drill,
        drills::{
            model::{
                DrillSuite, Expectation, Locator, Matcher, ScreenshotMatch, Step, StepKind,
                TelemetryExpectation, ValueExpectation, WaitState,
            },
            run::ErrorInfo,
        },
        drive::{BridgeReply, BridgeSession, DriveAppSelector},
        read, workflow,
    },
    discovery::DiscoveredApp,
    process_tree::ProcessTree,
};

const LAUNCH_POLL: Duration = Duration::from_millis(500);
const WAIT_POLL: Duration = Duration::from_millis(100);
/// Long enough for one bridge round trip, even when the expect budget is nearly spent.
const MIN_INSPECT_TIMEOUT: Duration = Duration::from_millis(250);
/// Bounded so failure evidence never hangs a run.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
/// The per-attempt telemetry slice is best effort; don't stall the run on a gone app.
const SLICE_FLUSH_TIMEOUT: Duration = Duration::from_secs(1);
const RELOAD_POLL_TIMEOUT: Duration = Duration::from_secs(2);
/// How much of the app's output a launch failure quotes; the full log stays in the run.
const LAUNCH_OUTPUT_SHOWN: usize = 2_000;
/// How much of the app's output failure evidence keeps; the full log stays in the run.
const EVIDENCE_OUTPUT_KEPT: usize = 64 * 1024;

/// How each suite gets its app.
#[derive(Debug, Clone)]
pub enum LaunchSource {
    /// Spawn this command (from `-- <command>`).
    Command(Vec<String>),
    /// Spawn the `start` command from this config, with fresh ports each launch.
    Config(PathBuf),
    /// Drive an already running app.
    Attach,
}

pub struct RealHost {
    pub app: String,
    pub source: LaunchSource,
    pub require_frontend: bool,
    pub ready_timeout: Duration,
    pub workspace_root: PathBuf,
    /// Set by Ctrl+C or `cancel.request`; stops the readiness wait early.
    pub cancel: Arc<AtomicBool>,
    /// Where each launch's stdout/stderr go, as `app-<n>.log` (the run directory).
    pub log_dir: Option<PathBuf>,
    /// Launches so far, numbering the logs.
    pub launches: usize,
    /// The log of the launch that just failed, for [`Host::capture_launch_failure`].
    pub failed_launch_log: Option<PathBuf>,
}

impl Host for RealHost {
    fn launch(
        &mut self,
        suite: &DrillSuite,
        env: &[(String, String)],
        progress: &mut dyn FnMut(&str),
    ) -> Result<Box<dyn AppDriver>> {
        self.failed_launch_log = None;
        let (command, mut environment) = match &self.source {
            LaunchSource::Attach => {
                if !env.is_empty() {
                    eprintln!(
                        "warning: suite `{}` sets environment, but an attached app was started without it",
                        suite.suite
                    );
                }
                return self.attach();
            }
            LaunchSource::Command(command) => (command.clone(), Vec::new()),
            LaunchSource::Config(path) => {
                let config = workflow::load_launch_config(path)?
                    .ok_or_else(|| anyhow!("`{}` no longer exists", path.display()))?;
                let command = config
                    .command
                    .ok_or_else(|| anyhow!("`{}` has no `start` command", path.display()))?;
                (command, config.environment)
            }
        };
        // Fresh port variables from the config win over the suite's.
        environment.splice(0..0, env.iter().cloned());
        self.spawn(&command, &environment, progress)
    }

    fn capture_launch_failure(&mut self, writer: &mut RunWriter) {
        if let Some(log) = self.failed_launch_log.take() {
            let name = log.file_stem().unwrap_or_default().to_string_lossy();
            let file = format!("launch-{name}-output.log");
            let _ = attach_output(writer, &log, "app output", &file);
        }
    }

    fn run_hook(
        &mut self,
        hook: &Value,
        teardown: bool,
        env: &[(String, String)],
    ) -> Result<HookResult> {
        let run = drill::run_hook_json(&self.workspace_root, hook, teardown, env)?;
        Ok(HookResult {
            passed: run.passed,
            timed_out: run.timed_out,
            report: run.report,
        })
    }
}

impl RealHost {
    fn check_cancel(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("cancelled while waiting for the app"));
        }
        Ok(())
    }

    fn attach(&self) -> Result<Box<dyn AppDriver>> {
        let selector = DriveAppSelector {
            app: Some(self.app.clone()),
            session_id: None,
            instance_id: None,
            pid: None,
            latest: false,
            active: true,
        };
        let session = BridgeSession::connect(&selector).map_err(|error| {
            anyhow!(
                "{error:#}\nStart the app first, or pass `-- <command>` (or a config `start` command) so the drill launches it."
            )
        })?;
        check_drill_support(&session)?;
        Ok(Box::new(BridgeDriver::new(session, None, None)))
    }

    fn spawn(
        &mut self,
        command: &[String],
        environment: &[(String, String)],
        progress: &mut dyn FnMut(&str),
    ) -> Result<Box<dyn AppDriver>> {
        self.launches += 1;
        let log = self
            .log_dir
            .as_ref()
            .map(|dir| dir.join(format!("app-{}.log", self.launches)));
        let preexisting = drill::matching_sessions(&self.app)?;
        let preexisting_ids: HashSet<&str> = preexisting
            .iter()
            .map(|app| app.instance_id.as_str())
            .collect();
        let started_at = OffsetDateTime::now_utc();
        let cancel = Arc::clone(&self.cancel);
        let cancelled = move || cancel.load(Ordering::SeqCst);
        let mut child = drill::spawn_command(command, environment, log.as_deref(), &cancelled)?;
        let deadline = Instant::now() + self.ready_timeout;
        let mut output = LaunchOutput::new(log.as_deref(), progress);
        let ready = self.wait_until_ready(
            &mut child,
            started_at,
            &preexisting_ids,
            deadline,
            &mut output,
        );
        let session = ready.and_then(|app| {
            let session = BridgeSession::connect(&drill::drive_selector(&app))?;
            check_drill_support(&session)?;
            Ok(session)
        });
        match session {
            Ok(session) => Ok(Box::new(BridgeDriver::new(session, Some(child), log))),
            Err(error) => {
                let _ = drill::cleanup_child_tree(&mut child);
                let hint = if Instant::now() >= deadline {
                    output.building_hint()
                } else {
                    String::new()
                };
                match log {
                    Some(log) => {
                        let tail = output_tail(&log);
                        let error = anyhow!("{error:#}{hint}\n{tail}");
                        self.failed_launch_log = Some(log);
                        Err(error)
                    }
                    None => Err(error),
                }
            }
        }
    }

    fn wait_until_ready(
        &self,
        child: &mut Child,
        started_at: OffsetDateTime,
        preexisting_ids: &HashSet<&str>,
        deadline: Instant,
        output: &mut LaunchOutput<'_>,
    ) -> Result<DiscoveredApp> {
        let timeout = self.ready_timeout.as_secs();
        let app = loop {
            output.poll();
            if let Some(app) = drill::find_spawned_session(&self.app, started_at, preexisting_ids)?
            {
                break app;
            }
            if let Some(status) = child.try_wait()? {
                return Err(anyhow!(
                    "app command exited ({status}) before a new `{}` Auditaur session appeared",
                    self.app
                ));
            }
            self.check_cancel()?;
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "timed out after {timeout}s waiting for a new `{}` Auditaur session",
                    self.app
                ));
            }
            thread::sleep(LAUNCH_POLL);
        };
        let selector = debug::DebugSelector {
            db: Some(PathBuf::from(&app.database_path)),
            app: None,
            session_id: Some(app.session_id.clone()),
            instance_id: Some(app.instance_id.clone()),
            pid: Some(app.pid),
            latest: false,
            active: false,
            cdp_port: None,
            require_frontend: self.require_frontend,
            require_drive_bridge: true,
        };
        loop {
            output.poll();
            let status = debug::snapshot(&selector)?;
            if status.ready {
                return Ok(app);
            }
            if let Some(exit) = child.try_wait()? {
                return Err(anyhow!("app exited ({exit}) before it was ready"));
            }
            self.check_cancel()?;
            if Instant::now() >= deadline {
                let waiting = status
                    .stages
                    .iter()
                    .filter(|stage| {
                        !matches!(
                            stage.status,
                            DebugStageStatus::Ok | DebugStageStatus::Skipped
                        )
                    })
                    .map(|stage| format!("{}: {}", stage.name, stage.message))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(anyhow!(
                    "timed out after {timeout}s waiting for the app to be ready ({waiting})"
                ));
            }
            thread::sleep(LAUNCH_POLL);
        }
    }
}

pub struct BridgeDriver {
    session: BridgeSession,
    /// The spawned app process; `None` when attached to a running app.
    child: Option<ProcessTree>,
    /// Where the spawned app's output goes, in the run directory.
    log: Option<PathBuf>,
    /// The app's telemetry store, opened on first use.
    store: Option<SqliteStore>,
    /// Cleared once the bridge says it has no `flush` action.
    can_flush: bool,
    /// Set once the bridge is known to honor `within` (older ones ignore it and search the page).
    within_supported: bool,
}

impl BridgeDriver {
    fn new(session: BridgeSession, child: Option<ProcessTree>, log: Option<PathBuf>) -> Self {
        Self {
            session,
            child,
            log,
            store: None,
            can_flush: true,
            within_supported: false,
        }
    }

    /// Before the first step that scopes a locator with `within`, make sure the bridge honors
    /// it: an older one ignores the key and would silently match across the whole page.
    fn require_within(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepOutcome {
        if self.within_supported || !step.kind.uses_within() {
            return Ok(());
        }
        let reply = self.call(
            ctx,
            "exists",
            Some(within_probe_selector()),
            None,
            Vec::new(),
        );
        let ignored = match reply {
            Ok(reply) => bridge_ignores_within(&reply.payload),
            // Couldn't ask; let the step run and ask again next time.
            Err(_) => return Ok(()),
        };
        if ignored {
            return Err(failed(within_upgrade_message()));
        }
        self.within_supported = true;
        Ok(())
    }
}

/// An `exists` probe for `html` inside a container that can't exist: a bridge that honors
/// `within` finds nothing, one that ignores it finds the page.
pub fn within_probe_selector() -> String {
    bridge_selector(&Locator::Within {
        locator: Box::new(Locator::Css("html".to_string())),
        within: Box::new(Locator::Css(MISSING_PROBE.to_string())),
    })
}

pub fn bridge_ignores_within(probe_payload: &Value) -> bool {
    probe_payload.get("exists") == Some(&json!(true))
}

pub fn within_upgrade_message() -> String {
    format!("this app's @auditaur/api ignores `within`; upgrade it to {MIN_WITHIN_API_VERSION} or newer to scope locators")
}

impl Drop for BridgeDriver {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl AppDriver for BridgeDriver {
    fn app_info(&self) -> Value {
        let mut info = self.session.app_info();
        info["launched"] = json!(self.child.is_some());
        if let Some(name) = self.log.as_deref().and_then(Path::file_name) {
            info["appLog"] = json!(name.to_string_lossy());
        }
        info
    }

    fn run_step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepResult {
        if let Err(failure) = self.require_within(step, ctx) {
            return failure;
        }
        match self.step(step, ctx) {
            Ok(()) => StepResult::Passed,
            Err(failure) => failure,
        }
    }

    fn capture_failure(&mut self, ctx: &mut StepCtx<'_>) {
        let mut evidence = Evidence::default();
        let exited = self.exited();
        match &exited {
            Some(reason) => evidence.skip("bridge", format!("{reason}; nothing to capture")),
            None => self.capture(ctx, "failure", &mut evidence, true),
        }
        if let Some(log) = self.log.clone() {
            let file = ctx.artifact("failure-app-output.log");
            let saved = attach_output(ctx.writer, &log, "failure app output", &file);
            evidence.note("app output", saved);
        } else {
            evidence.skip(
                "app output",
                "no app log: the drill did not launch the app".to_string(),
            );
        }
        let manifest = json!({
            "captured": evidence.captured,
            "missing": evidence.missing,
            "appExited": exited,
            "appLog": self.log.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy()),
        });
        if let Ok(bytes) = serde_json::to_vec_pretty(&manifest) {
            let file = ctx.artifact("failure-evidence.json");
            let _ = ctx
                .writer
                .attach("failure evidence", "application/json", &file, &bytes);
        }
    }

    fn capture_step(&mut self, ctx: &mut StepCtx<'_>) {
        self.capture(ctx, "trace", &mut Evidence::default(), false);
    }

    fn telemetry(&mut self, since_unix_nanos: i64, live: bool) -> Option<Value> {
        if live {
            self.flush(None, None, SLICE_FLUSH_TIMEOUT);
        }
        let related = self.recorded_since(since_unix_nanos).ok()?;
        serde_json::to_value(related).ok()
    }

    fn exited(&mut self) -> Option<String> {
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(format!("app process exited ({status})"));
            }
        }
        // The launcher (`tauri dev`, `npm start`) can outlive the app it started, and an attached
        // app has no launcher at all: either way the bridge is stranded and every later step
        // would only time out. If the check itself fails, don't claim the app is gone.
        let pid = self.session.pid();
        if workflow::process_exists(pid).unwrap_or(true) {
            return None;
        }
        const HINT: &str = "something relaunched it, such as a `tauri dev` rebuild. Drill a built app, or avoid source edits while drills run";
        Some(match self.session.successor() {
            Some(next) => format!(
                "it restarted mid-run (pid {pid} → {}, session {} → {}); {HINT}",
                next.pid,
                self.session.session_id(),
                next.session_id
            ),
            None if self.child.is_some() => format!(
                "the app (pid {pid}) is gone but its launch command is still running; {HINT}"
            ),
            None => format!("the app (pid {pid}) is gone"),
        })
    }

    fn bridge(
        &mut self,
        action: &str,
        value: Option<String>,
        timeout: Duration,
    ) -> std::result::Result<Value, String> {
        let reply = self
            .session
            .request(action, None, value, Vec::new(), false, None, None, timeout)
            .map_err(|error| format!("{error:#}"))?;
        if reply.ok {
            return Ok(reply.payload);
        }
        let error = reply
            .error
            .unwrap_or_else(|| format!("bridge refused `{action}`"));
        if error.contains("unsupported drive bridge action") {
            return Err(format!(
                "{error}\n`dialog` steps need @auditaur/api {MIN_DIALOG_API_VERSION} or newer in the app"
            ));
        }
        Err(error)
    }

    fn close(&mut self) -> Result<()> {
        // Keep the handle until cleanup succeeds so `Drop` can retry.
        if let Some(child) = self.child.as_mut() {
            drill::cleanup_child_tree(child)?;
            self.child = None;
        }
        Ok(())
    }
}

type StepOutcome = std::result::Result<(), StepResult>;

fn failed(message: impl Into<String>) -> StepResult {
    StepResult::Failed(ErrorInfo::new(message))
}

fn timed_out(message: impl Into<String>) -> StepResult {
    StepResult::TimedOut(ErrorInfo::new(message))
}

/// Bridge selectors carry the whole locator as JSON; the bridge resolves it strictly.
pub(crate) const LOCATOR_PREFIX: &str = "locator:";
/// The bridge's error when an action's target is missing (or not visible).
const SELECTOR_NOT_FOUND: &str = "selector not found";
/// The bridge's replies when an action's target isn't actionable yet; `act` waits for them to
/// clear, like Playwright's "enabled" and "editable" checks.
const ELEMENT_DISABLED: &str = "element is disabled";
const ELEMENT_NOT_EDITABLE: &str = "element is not editable";
/// The bridge's replies while a select value has no option yet, or only a disabled one.
const OPTION_NOT_FOUND: &str = "option not found";
const OPTION_DISABLED: &str = "option is disabled";

pub(crate) fn bridge_selector(locator: &Locator) -> String {
    format!("{LOCATOR_PREFIX}{}", locator_spec(locator))
}

fn locator_spec(locator: &Locator) -> Value {
    fn put<T: serde::Serialize>(spec: &mut Value, key: &str, value: Option<T>) {
        if let Some(value) = value {
            spec[key] = json!(value);
        }
    }
    let (kind, value, name, exact, nth) = match locator {
        Locator::Within { locator, within } => {
            let mut spec = locator_spec(locator);
            spec["within"] = locator_spec(within);
            return spec;
        }
        Locator::Css(css) => ("css", css, None, false, None),
        Locator::CssNth { css, nth } => ("css", css, None, false, Some(*nth)),
        Locator::Role {
            role,
            name,
            exact,
            nth,
        } => ("role", role, name.as_ref(), *exact, *nth),
        Locator::Text { text, exact, nth } => ("text", text, None, *exact, *nth),
        Locator::TestId { id, nth } => ("testId", id, None, false, *nth),
        Locator::Label { label, exact, nth } => ("label", label, None, *exact, *nth),
        Locator::Placeholder {
            placeholder,
            exact,
            nth,
        } => ("placeholder", placeholder, None, *exact, *nth),
    };
    let mut spec = json!({ "kind": kind, "value": value });
    put(&mut spec, "name", name);
    put(&mut spec, "exact", exact.then_some(true));
    put(&mut spec, "nth", nth);
    spec
}

/// Name the target in an error unless the bridge already did.
fn about(target: &Locator, error: String) -> String {
    let target = target.to_string();
    if error.contains(&target) {
        error
    } else {
        format!("`{target}`: {error}")
    }
}

/// How much of the log a launch reads to find the app's latest output line.
const PROGRESS_TAIL: usize = 4_096;
/// Progress lines are a glance, not a log.
const PROGRESS_LINE_MAX: usize = 200;
/// Lines a build tool prints while it works (cargo's status verbs), so a readiness timeout
/// can say the app was still building rather than hung.
const BUILD_ACTIVITY: &[&str] = &[
    "Compiling ",
    "Building ",
    "Checking ",
    "Blocking waiting for file lock",
    "Downloading ",
    "Downloaded ",
    "Updating ",
    "Locking ",
];

/// Follows a launching app's output: reports each new latest line as step progress, and
/// remembers it to explain a readiness timeout.
struct LaunchOutput<'a> {
    log: Option<&'a Path>,
    progress: &'a mut dyn FnMut(&str),
    last: Option<String>,
}

impl<'a> LaunchOutput<'a> {
    fn new(log: Option<&'a Path>, progress: &'a mut dyn FnMut(&str)) -> Self {
        Self {
            log,
            progress,
            last: None,
        }
    }

    fn poll(&mut self) {
        let Some(log) = self.log else { return };
        let Ok((text, _)) = read_tail(log, PROGRESS_TAIL) else {
            return;
        };
        let Some(line) = last_line(&text) else { return };
        if self.last.as_deref() != Some(line.as_str()) {
            (self.progress)(&line);
            self.last = Some(line);
        }
    }

    /// Why a readiness timeout may only mean a slow build, or nothing.
    fn building_hint(&mut self) -> String {
        self.poll();
        match &self.last {
            Some(line) if is_build_activity(line) => format!(
                "\nthe app was still building (`{line}`); the first launch after a dependency change can take longer, so raise `--timeout-seconds` or `readiness.timeoutSeconds` in .auditaur/config.json"
            ),
            _ => String::new(),
        }
    }
}

/// The last non-empty line of some output, without terminal escapes, cut to a glance.
fn last_line(text: &str) -> Option<String> {
    let line = text
        .split(['\n', '\r'])
        .map(|line| strip_ansi(line).trim().to_string())
        .rfind(|line| !line.is_empty())?;
    Some(match line.char_indices().nth(PROGRESS_LINE_MAX) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    })
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // CSI sequences (`ESC [ … final`) are what build tools color with; drop any other
        // escape's next char too.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

fn is_build_activity(line: &str) -> bool {
    BUILD_ACTIVITY.iter().any(|verb| line.starts_with(verb))
}

/// The end of a launch log, for a launch failure message.
fn output_tail(log: &Path) -> String {
    let (text, cut) = match read_tail(log, LAUNCH_OUTPUT_SHOWN) {
        Ok(tail) => tail,
        Err(error) => return format!("could not read app output `{}`: {error}", log.display()),
    };
    let text = text.trim();
    if text.is_empty() && !cut {
        return format!("the app printed nothing (`{}`)", log.display());
    }
    let ellipsis = if cut { "…" } else { "" };
    format!("app output (`{}`):\n{ellipsis}{text}", log.display())
}

/// The last `max_chars` characters of a log, reading at most the bytes they can take, and
/// whether earlier output was left out.
fn read_tail(log: &Path, max_chars: usize) -> std::io::Result<(String, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(log)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub((max_chars * 4) as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    // The app may still be writing; read no further than the window.
    file.take(len - start).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let from = text
        .char_indices()
        .rev()
        .nth(max_chars - 1)
        .map_or(0, |(at, _)| at);
    Ok((text[from..].to_string(), start > 0 || from > 0))
}

/// Save the end of the app's output as an attachment of the running step.
fn attach_output(writer: &mut RunWriter, log: &Path, name: &str, file: &str) -> Result<()> {
    let (text, cut) = read_tail(log, EVIDENCE_OUTPUT_KEPT)?;
    let log_name = log.file_name().unwrap_or_default().to_string_lossy();
    let head = if cut {
        format!("… earlier output is in {log_name}\n")
    } else {
        String::new()
    };
    let body = format!("{head}{text}");
    writer
        .attach(name, "text/plain", file, body.as_bytes())
        .map(drop)
}

/// What failure capture saved, and why anything is missing.
#[derive(Default)]
struct Evidence {
    captured: Vec<String>,
    missing: serde_json::Map<String, Value>,
}

impl Evidence {
    fn note<T>(&mut self, name: &str, result: Result<T>) {
        match result {
            Ok(_) => self.captured.push(name.to_string()),
            Err(error) => self.skip(name, format!("{error:#}")),
        }
    }

    fn skip(&mut self, name: &str, reason: String) {
        self.missing.insert(name.to_string(), json!(reason));
    }
}

/// First `@auditaur/api` release whose drive bridge understands `locator:` selectors and the
/// drill actions (`dblclick`, `inspect`), and reports a missing element without throwing once
/// bundled (0.7.0 threw, so every auto-wait failed at once).
const MIN_DRILL_API_VERSION: &str = "0.7.1";
/// First `@auditaur/api` release with `withDrillDialogs` and the `dialogs.*` bridge actions.
const MIN_DIALOG_API_VERSION: &str = "0.11.0";
/// First `@auditaur/api` release with the `clickButton` bridge action (right and middle clicks).
const MIN_BUTTON_CLICK_API_VERSION: &str = "0.12.0";
/// First `@auditaur/api` release with the `dragTo` bridge action.
const MIN_DRAG_API_VERSION: &str = "0.13.0";
/// First `@auditaur/api` release with the `paste` bridge action.
const MIN_PASTE_API_VERSION: &str = "0.14.0";
/// First `@auditaur/api` release with `within` locators and the `clearStorage` bridge action.
const MIN_WITHIN_API_VERSION: &str = "0.15.0";
const MIN_CLEAR_STORAGE_API_VERSION: &str = MIN_WITHIN_API_VERSION;

/// Matches nothing, so the probe exercises the bridge's not-found reply.
const MISSING_PROBE: &str = "[data-auditaur-drill-probe-missing]";

/// Fail the launch, not step 1, when the app's bridge predates drill locators. Only a known
/// stale-bridge reply fails; anything else is left for the steps to report.
fn check_drill_support(session: &BridgeSession) -> Result<()> {
    let probe = bridge_selector(&Locator::Css(MISSING_PROBE.to_string()));
    let reply = session.request(
        "exists",
        Some(probe),
        None,
        Vec::new(),
        false,
        None,
        None,
        CAPTURE_TIMEOUT,
    );
    let error = match reply {
        Ok(reply) => reply.error,
        Err(error) => Some(format!("{error:#}")),
    };
    match error {
        Some(error) if is_stale_bridge(&error) => Err(anyhow!(explain(error))),
        _ => Ok(()),
    }
}

/// Old bridges reject `locator:` selectors and newer actions (`dblclick`, `inspect`).
/// Received values in failure messages stay readable.
fn clip_value(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 500 {
        format!("{}…", text.chars().take(500).collect::<String>())
    } else {
        text
    }
}

fn is_stale_bridge(error: &str) -> bool {
    (error.contains("is not a valid selector") && error.contains(LOCATOR_PREFIX))
        || error.contains("unsupported drive bridge action")
        || error.contains("'selectorNotFoundMessage' before initialization")
}

fn explain(error: String) -> String {
    if is_stale_bridge(&error) {
        format!(
            "{error}\nThis app's @auditaur/api is too old for `auditaur drill test`; upgrade it to {MIN_DRILL_API_VERSION} or newer to drive this app with drills."
        )
    } else {
        error
    }
}

/// Like [`explain`], but names the release an action newer than drill support needs.
fn explain_action(action: &str, error: String) -> String {
    let needs = match action {
        "clickButton" => Some(("`click` with `button`", MIN_BUTTON_CLICK_API_VERSION)),
        "dragTo" => Some(("`drag`", MIN_DRAG_API_VERSION)),
        "paste" => Some(("`paste`", MIN_PASTE_API_VERSION)),
        "clearStorage" => Some(("`clearStorage`", MIN_CLEAR_STORAGE_API_VERSION)),
        _ => None,
    };
    match needs {
        Some((step, version)) if error.contains("unsupported drive bridge action") => {
            format!("{error}\n{step} needs @auditaur/api {version} or newer in the app")
        }
        _ => explain(error),
    }
}

impl BridgeDriver {
    /// Attach a screenshot, DOM snapshot, and (with `aria`) aria snapshot to the current step,
    /// named `<kind> screenshot`, `<kind> dom`, and `<kind> aria`. Best effort; `evidence`
    /// records what was saved and why anything wasn't.
    fn capture(&mut self, ctx: &mut StepCtx<'_>, kind: &str, evidence: &mut Evidence, aria: bool) {
        let index = ctx.step_index;
        let step_id = Some(format!("step-{index}"));
        let shot = self
            .session
            .screenshot_png(None, ctx.test_id.clone(), step_id, CAPTURE_TIMEOUT)
            .and_then(|png| {
                let file = ctx.artifact(&format!("{kind}.png"));
                ctx.writer
                    .attach(&format!("{kind} screenshot"), "image/png", &file, &png)
            });
        evidence.note("screenshot", shot);
        let dom = self.payload(ctx, "snapshot").and_then(|payload| {
            let file = ctx.artifact(&format!("{kind}-dom.json"));
            let bytes = serde_json::to_vec_pretty(&payload)?;
            ctx.writer
                .attach(&format!("{kind} dom"), "application/json", &file, &bytes)
        });
        evidence.note("dom", dom);
        if aria {
            let aria = self.payload(ctx, "ariaSnapshot").and_then(|payload| {
                let text = payload
                    .get("snapshot")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("bridge `ariaSnapshot` reply had no `snapshot`"))?;
                let file = ctx.artifact(&format!("{kind}-aria.yml"));
                ctx.writer
                    .attach(&format!("{kind} aria"), "text/yaml", &file, text.as_bytes())
            });
            evidence.note("aria", aria);
        }
    }

    /// The payload of a page-wide bridge request made for evidence.
    fn payload(&self, ctx: &StepCtx<'_>, action: &str) -> Result<Value> {
        let reply = self.request(ctx, action, None, None, CAPTURE_TIMEOUT)?;
        if reply.ok {
            Ok(reply.payload)
        } else {
            Err(anyhow!(reply
                .error
                .unwrap_or_else(|| format!("bridge refused `{action}`"))))
        }
    }

    /// Wait for a human (or agent) through the gate queue; see [`drill::run_test_gate`].
    fn gate(&mut self, ctx: &mut StepCtx<'_>, gate: &Value) -> StepOutcome {
        let target = self.session.gate_target();
        let cancel = ctx.cancel.clone();
        let child = &mut self.child;
        let mut stop = || {
            if cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::SeqCst))
            {
                return Some("interrupted".to_string());
            }
            match child.as_deref_mut().map(Child::try_wait) {
                Some(Ok(Some(status))) => Some(format!("app process exited ({status})")),
                _ => None,
            }
        };
        let deadline = ctx.deadline;
        let writer = &mut *ctx.writer;
        let mut waiting = |request: Value| writer.gate_waiting(request);
        let outcome = drill::run_test_gate(gate, &target, deadline, &mut stop, &mut waiting);
        let (response, result) = match outcome {
            Ok(drill::TestGateOutcome::Passed(report)) => (report, Ok(())),
            Ok(drill::TestGateOutcome::Skipped(report)) => (report, Err(StepResult::Skipped)),
            Ok(drill::TestGateOutcome::Failed(message, report)) => (report, Err(failed(message))),
            Ok(drill::TestGateOutcome::TimedOut(message)) => {
                (json!({ "timedOut": true }), Err(timed_out(message)))
            }
            Ok(drill::TestGateOutcome::Stopped(reason)) => {
                (json!({ "stopped": reason }), Err(failed(reason)))
            }
            Err(error) => return Err(failed(format!("{error:#}"))),
        };
        if let Err(error) = ctx.writer.gate_resolved(response) {
            return Err(failed(format!("could not record gate result: {error:#}")));
        }
        result
    }

    fn step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepOutcome {
        match &step.kind {
            StepKind::Click {
                target,
                button: None,
            } => self.act(ctx, "click", target, None, Vec::new()),
            StepKind::Click {
                target,
                button: Some(button),
            } => self.act(
                ctx,
                "clickButton",
                target,
                Some(button.as_str().to_string()),
                Vec::new(),
            ),
            StepKind::Dblclick(target) => self.act(ctx, "dblclick", target, None, Vec::new()),
            StepKind::Hover(target) => self.act(ctx, "hover", target, None, Vec::new()),
            StepKind::Drag { source, to } => {
                // Wait for the drop target first so a timeout names the element that held it up.
                self.poll(ctx, to, true, true)
                    .map_err(|error| match error {
                        StepResult::TimedOut(_) => {
                            timed_out(format!("drag: timed out waiting for `{to}` to be visible"))
                        }
                        other => other,
                    })?;
                self.act(ctx, "dragTo", source, Some(bridge_selector(to)), Vec::new())
            }
            StepKind::Check(target) => self.act(ctx, "check", target, None, Vec::new()),
            StepKind::Uncheck(target) => self.act(ctx, "uncheck", target, None, Vec::new()),
            StepKind::Fill { target, value } => {
                self.act(ctx, "fill", target, Some(value.clone()), Vec::new())
            }
            StepKind::Type { target, value } => {
                self.act(ctx, "type", target, Some(value.clone()), Vec::new())
            }
            StepKind::Select { target, values } => self.act(
                ctx,
                "select",
                target,
                values.first().cloned(),
                values.clone(),
            ),
            StepKind::Press {
                key,
                target: Some(target),
            } => self.act(ctx, "press", target, Some(key.clone()), Vec::new()),
            StepKind::Press { key, target: None } => self
                .call(ctx, "press", None, Some(key.clone()), Vec::new())
                .map(drop),
            StepKind::Paste(paste) => {
                let payload = super::paste::payload(paste, ctx.env).map_err(failed)?;
                match &paste.target {
                    Some(target) => self.act(ctx, "paste", target, Some(payload), Vec::new()),
                    None => self
                        .call(ctx, "paste", None, Some(payload), Vec::new())
                        .map(drop),
                }
            }
            StepKind::WaitFor { target, state } => self.wait_for(ctx, target, *state),
            StepKind::Evaluate(expression) => self
                .call(ctx, "evaluate", None, Some(expression.clone()), Vec::new())
                .map(drop),
            StepKind::Reload => self.reload(ctx),
            StepKind::ClearStorage => {
                self.call(ctx, "clearStorage", None, None, Vec::new())?;
                self.reload(ctx)
            }
            StepKind::Sleep(ms) => {
                let wait = Duration::from_millis(*ms);
                if wait > ctx.remaining() {
                    thread::sleep(ctx.remaining());
                    return Err(timed_out(format!(
                        "sleep of {ms}ms exceeds the remaining time"
                    )));
                }
                thread::sleep(wait);
                Ok(())
            }
            StepKind::Screenshot { name, target } => self.screenshot(ctx, name, target.as_ref()),
            StepKind::Gate(gate) => self.gate(ctx, gate),
            StepKind::Expect(expectation) => match &expectation.matcher {
                Matcher::ToMatchAriaSnapshot(_) => self.expect_aria(ctx, expectation),
                Matcher::ToHaveScreenshot(options) => {
                    self.expect_screenshot(ctx, expectation, options)
                }
                _ => self.expect(ctx, expectation),
            },
            StepKind::Telemetry(expectation) => self.expect_telemetry(ctx, step, expectation),
            StepKind::ExpectValue(expectation) => self.expect_value(ctx, step, expectation),
            // The runner runs commands through the host; they never reach the app.
            StepKind::Run(_) => Err(failed("`run` steps are run by the host")),
            StepKind::Dialog(_) => Err(failed("`dialog` steps are queued by the runner")),
        }
    }

    /// Re-evaluate the expression until its result matches, like Playwright's `expect.poll`.
    /// A throwing expression counts as a miss and is retried.
    fn expect_value(
        &self,
        ctx: &StepCtx<'_>,
        step: &Step,
        expectation: &ValueExpectation,
    ) -> StepOutcome {
        let started = Instant::now();
        let give_up = (started + ctx.expect_timeout).min(ctx.deadline);
        // What the expression last produced (or threw), kept apart from bridge failures: the
        // final poll often only has a sliver of time left, and its timeout says nothing.
        let mut observed: Option<String> = None;
        let mut bridge_error: Option<String> = None;
        let mut first = true;
        loop {
            let now = Instant::now();
            if now >= give_up && !first {
                break;
            }
            let budget = give_up.saturating_duration_since(now);
            let wait = if first {
                budget.max(MIN_INSPECT_TIMEOUT)
            } else {
                budget
            }
            .min(ctx.remaining());
            first = false;
            if wait.is_zero() {
                break;
            }
            let reply = self.request(
                ctx,
                "evaluate",
                None,
                Some(expectation.expression.clone()),
                wait,
            );
            match reply {
                Ok(reply) if reply.ok => {
                    let Some(value) = reply.payload.get("value") else {
                        return Err(failed("bridge `evaluate` reply had no `value`"));
                    };
                    if expectation.matcher.matches(value) != expectation.not {
                        return Ok(());
                    }
                    observed = Some(clip_value(value));
                    bridge_error = None;
                }
                Ok(reply) => {
                    let error = reply
                        .error
                        .unwrap_or_else(|| "bridge returned ok=false".to_string());
                    observed = Some(format!("threw: {error}"));
                    bridge_error = None;
                }
                // A poll cut short by the deadline says nothing once a value has been seen.
                Err(error) if observed.is_none() || wait >= MIN_INSPECT_TIMEOUT => {
                    bridge_error = Some(format!("evaluate failed: {error:#}"));
                }
                Err(_) => {}
            }
            thread::sleep(WAIT_POLL.min(give_up.saturating_duration_since(Instant::now())));
        }
        let ms = started.elapsed().as_millis();
        let not = if expectation.not { "not " } else { "" };
        let expected = format!("{not}{}", expectation.matcher);
        let (actual, after) = value_received(observed, bridge_error);
        let after = after
            .map(|error| format!("\nthen: {error}"))
            .unwrap_or_default();
        let info = ErrorInfo {
            message: format!(
                "{} failed after {ms}ms\nexpected: {expected}\nreceived: {actual}{after}",
                step.title
            ),
            expected: Some(Value::String(expected)),
            actual: Some(Value::String(actual)),
            ..ErrorInfo::default()
        };
        if ctx.expired() && give_up < started + ctx.expect_timeout {
            Err(StepResult::TimedOut(info))
        } else {
            Err(StepResult::Failed(info))
        }
    }

    /// Ask the frontend to push its queued telemetry now so assertions see it promptly.
    /// Older bridges lack `flush`; then assertions just poll until the batch lands.
    fn flush(&mut self, test_id: Option<String>, step_id: Option<String>, timeout: Duration) {
        if !self.can_flush || timeout.is_zero() {
            return;
        }
        let reply = self.session.request(
            "flush",
            None,
            None,
            Vec::new(),
            false,
            test_id,
            step_id,
            timeout,
        );
        if let Ok(reply) = reply {
            if reply
                .error
                .is_some_and(|error| error.contains("unsupported drive bridge action"))
            {
                self.can_flush = false;
            }
        }
    }

    /// Everything this session recorded since `since` (Unix nanos), minus the harness's own logs.
    fn recorded_since(&mut self, since: i64) -> Result<RelatedTelemetry> {
        if self.store.is_none() {
            self.store = Some(read::open_validated_store(Path::new(
                self.session.db_path(),
            ))?);
        }
        let store = self.store.as_ref().expect("opened above");
        let related = store.related_telemetry(&RelatedTelemetryQuery {
            session_id: Some(self.session.session_id().to_string()),
            trace_id: None,
            window_label: None,
            start_time_unix_nanos: Some(since),
            end_time_unix_nanos: None,
            limit: Some(usize::MAX),
        })?;
        Ok(telemetry::scrub(related))
    }

    /// Re-read telemetry until the assertion holds, can no longer hold, or time runs out.
    fn expect_telemetry(
        &mut self,
        ctx: &StepCtx<'_>,
        step: &Step,
        expectation: &TelemetryExpectation,
    ) -> StepOutcome {
        let started = Instant::now();
        let give_up = (started + ctx.expect_timeout).min(ctx.deadline);
        let mut last = None;
        let mut first = true;
        loop {
            if Instant::now() >= give_up && !first {
                break;
            }
            first = false;
            let budget = give_up
                .saturating_duration_since(Instant::now())
                .max(MIN_INSPECT_TIMEOUT)
                .min(CAPTURE_TIMEOUT)
                .min(ctx.remaining());
            self.flush(
                ctx.test_id.clone(),
                Some(format!("step-{}", ctx.step_index)),
                budget,
            );
            let related = self
                .recorded_since(ctx.since_unix_nanos)
                .map_err(|error| failed(format!("reading recorded telemetry failed: {error:#}")))?;
            let verdict = telemetry::judge(expectation, &related);
            match verdict.judgement {
                Judgement::Pass => return Ok(()),
                Judgement::Failed => {
                    last = Some(verdict);
                    break;
                }
                Judgement::Pending => last = Some(verdict),
            }
            thread::sleep(WAIT_POLL.min(give_up.saturating_duration_since(Instant::now())));
        }
        let ms = started.elapsed().as_millis();
        let headline = format!("{} failed after {ms}ms", step.title);
        let info = match &last {
            Some(verdict) => ErrorInfo {
                message: format!(
                    "{headline}\nexpected: {}\nreceived: {}",
                    verdict.expected, verdict.actual
                ),
                expected: Some(Value::String(verdict.expected.clone())),
                actual: Some(Value::String(verdict.actual.clone())),
                ..ErrorInfo::default()
            },
            None => ErrorInfo::new(format!("{headline}: no time left to check")),
        };
        let decided = last
            .as_ref()
            .is_some_and(|verdict| verdict.judgement == Judgement::Failed);
        if !decided && ctx.expired() && give_up < started + ctx.expect_timeout {
            Err(StepResult::TimedOut(info))
        } else {
            Err(StepResult::Failed(info))
        }
    }

    /// Send one bridge request tagged with the current test and step.
    fn request(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        selector: Option<String>,
        value: Option<String>,
        timeout: Duration,
    ) -> Result<BridgeReply> {
        self.request_full(ctx, action, selector, value, Vec::new(), false, timeout)
    }

    #[allow(clippy::too_many_arguments)]
    fn request_full(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        selector: Option<String>,
        value: Option<String>,
        values: Vec<String>,
        visible_only: bool,
        timeout: Duration,
    ) -> Result<BridgeReply> {
        self.session.request(
            action,
            selector,
            value,
            values,
            visible_only,
            ctx.test_id.clone(),
            Some(format!("step-{}", ctx.step_index)),
            timeout,
        )
    }

    /// A request that must succeed before the step deadline.
    fn call(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        selector: Option<String>,
        value: Option<String>,
        values: Vec<String>,
    ) -> std::result::Result<BridgeReply, StepResult> {
        if ctx.expired() {
            return Err(timed_out(format!("{action}: step timed out")));
        }
        let reply = self.request_full(ctx, action, selector, value, values, false, ctx.remaining());
        match reply {
            Ok(reply) if reply.ok => Ok(reply),
            Ok(reply) => Err(failed(explain_action(
                action,
                format!(
                    "{action} failed: {}",
                    reply
                        .error
                        .unwrap_or_else(|| "bridge returned ok=false".to_string())
                ),
            ))),
            Err(_) if ctx.expired() => Err(timed_out(format!("{action}: step timed out"))),
            Err(error) => Err(failed(explain_action(
                action,
                format!("{action} failed: {error:#}"),
            ))),
        }
    }

    /// Re-inspect the target until the matcher passes or the expect timeout runs out.
    fn expect(&self, ctx: &StepCtx<'_>, expectation: &Expectation) -> StepOutcome {
        let target = &expectation.target;
        let started = Instant::now();
        let give_up = (started + ctx.expect_timeout).min(ctx.deadline);
        let selector = bridge_selector(target);
        let mut last = None;
        let mut first = true;
        loop {
            let now = Instant::now();
            if now >= give_up && !first {
                break;
            }
            // The first check always gets a full round trip; later ones stop at `give_up`.
            let budget = give_up.saturating_duration_since(now);
            let wait = if first {
                budget.max(MIN_INSPECT_TIMEOUT)
            } else {
                budget
            }
            .min(ctx.remaining());
            first = false;
            if wait.is_zero() {
                break;
            }
            let reply = self.request(ctx, "inspect", Some(selector.clone()), None, wait);
            let inspection = match reply {
                Ok(reply) if reply.ok => serde_json::from_value::<Inspection>(reply.payload)
                    .map_err(|error| {
                        failed(format!("bridge `inspect` reply was malformed: {error}"))
                    })?,
                Ok(reply) => {
                    let error = reply
                        .error
                        .unwrap_or_else(|| "bridge returned ok=false".to_string());
                    return Err(failed(explain(about(target, error))));
                }
                Err(_) if Instant::now() >= give_up => break,
                Err(error) => return Err(failed(explain(about(target, format!("{error:#}"))))),
            };
            match judge(&expectation.matcher, expectation.not, &inspection) {
                Verdict::Pass => return Ok(()),
                Verdict::Fatal(message) => return Err(failed(about(target, message))),
                Verdict::Retry { expected, actual } => last = Some((expected, actual)),
            }
            thread::sleep(WAIT_POLL.min(give_up.saturating_duration_since(Instant::now())));
        }
        let ms = started.elapsed().as_millis();
        let not = if expectation.not { "not." } else { "" };
        let headline = format!(
            "expect(`{target}`).{not}{}() failed after {ms}ms",
            expectation.matcher.key()
        );
        let info = match last {
            Some((expected, actual)) => ErrorInfo {
                message: format!("{headline}\nexpected: {expected}\nreceived: {actual}"),
                expected: Some(Value::String(expected)),
                actual: Some(Value::String(actual)),
                ..ErrorInfo::default()
            },
            None => ErrorInfo::new(format!("{headline}: no time left to check")),
        };
        // A step or test deadline that cut the assertion short is a timeout.
        if ctx.expired() && give_up < started + ctx.expect_timeout {
            Err(StepResult::TimedOut(info))
        } else {
            Err(StepResult::Failed(info))
        }
    }

    /// Auto-wait for the target to be visible, then act on it (like Playwright actionability).
    /// The action itself also requires visibility; if the target vanished in between, wait again.
    fn act(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        target: &Locator,
        value: Option<String>,
        values: Vec<String>,
    ) -> StepOutcome {
        // Name the step as written, not the bridge action.
        let shown = match (action, value.as_deref()) {
            ("clickButton", Some(button)) => format!("{button}-click"),
            ("dragTo", _) => "drag".to_string(),
            _ => action.to_string(),
        };
        loop {
            self.poll(ctx, target, true, true)
                .map_err(|error| match error {
                    StepResult::TimedOut(_) => timed_out(format!(
                        "{shown}: timed out waiting for `{target}` to be visible"
                    )),
                    other => other,
                })?;
            if ctx.expired() {
                return Err(timed_out(format!("{shown}: step timed out")));
            }
            let reply = self.request_full(
                ctx,
                action,
                Some(bridge_selector(target)),
                value.clone(),
                values.clone(),
                true,
                ctx.remaining(),
            );
            match reply {
                Ok(reply) if reply.ok => return Ok(()),
                Ok(reply) if reply.error.as_deref() == Some(SELECTOR_NOT_FOUND) => {
                    thread::sleep(WAIT_POLL.min(ctx.remaining()));
                }
                Ok(reply)
                    if matches!(
                        reply.error.as_deref(),
                        Some(
                            ELEMENT_DISABLED
                                | ELEMENT_NOT_EDITABLE
                                | OPTION_NOT_FOUND
                                | OPTION_DISABLED
                        )
                    ) =>
                {
                    let options = || {
                        let requested = if values.is_empty() {
                            value.iter().cloned().collect()
                        } else {
                            values.clone()
                        };
                        requested
                            .iter()
                            .map(|value| format!("`{value}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    let waited_for = match reply.error.as_deref() {
                        Some(ELEMENT_DISABLED) => format!("`{target}` to be enabled"),
                        Some(ELEMENT_NOT_EDITABLE) => format!("`{target}` to be editable"),
                        Some(OPTION_NOT_FOUND) => {
                            format!("`{target}` to have option(s) {}", options())
                        }
                        _ => format!("option(s) {} of `{target}` to be enabled", options()),
                    };
                    // Time out here, not in the next visibility poll, so the message names
                    // the condition that actually held the step up.
                    thread::sleep(WAIT_POLL.min(ctx.remaining()));
                    if ctx.expired() {
                        return Err(timed_out(format!(
                            "{shown}: timed out waiting for {waited_for}"
                        )));
                    }
                }
                Ok(reply) => {
                    return Err(failed(explain_action(
                        action,
                        format!(
                            "{shown} `{target}` failed: {}",
                            reply
                                .error
                                .unwrap_or_else(|| "bridge returned ok=false".to_string())
                        ),
                    )))
                }
                Err(_) if ctx.expired() => {
                    return Err(timed_out(format!("{shown}: step timed out")))
                }
                Err(error) => {
                    return Err(failed(explain_action(
                        action,
                        format!("{shown} `{target}` failed: {error:#}"),
                    )))
                }
            }
        }
    }

    fn wait_for(&self, ctx: &StepCtx<'_>, target: &Locator, state: WaitState) -> StepOutcome {
        let (visible_only, want) = match state {
            WaitState::Attached => (false, true),
            WaitState::Detached => (false, false),
            WaitState::Visible => (true, true),
            WaitState::Hidden => (true, false),
        };
        self.poll(ctx, target, visible_only, want)
            .map_err(|error| match error {
                StepResult::TimedOut(_) => timed_out(format!(
                    "timed out waiting for `{target}` to be {}",
                    state.as_str()
                )),
                other => other,
            })
    }

    /// Poll `exists` until it equals `want` or the step deadline passes.
    fn poll(
        &self,
        ctx: &StepCtx<'_>,
        target: &Locator,
        visible_only: bool,
        want: bool,
    ) -> StepOutcome {
        self.poll_until(ctx, target, visible_only, want, ctx.deadline)
    }

    /// Poll `exists` until it equals `want` or `until` (capped by the step deadline) passes;
    /// running out is reported as `TimedOut`.
    fn poll_until(
        &self,
        ctx: &StepCtx<'_>,
        target: &Locator,
        visible_only: bool,
        want: bool,
        until: Instant,
    ) -> StepOutcome {
        let selector = bridge_selector(target);
        let until = until.min(ctx.deadline);
        let left = || until.saturating_duration_since(Instant::now());
        loop {
            if Instant::now() >= until {
                return Err(timed_out("wait: step timed out"));
            }
            let reply = self.request_full(
                ctx,
                "exists",
                Some(selector.clone()),
                None,
                Vec::new(),
                visible_only,
                left(),
            );
            match reply {
                Ok(reply) => {
                    // A missing element comes back as `exists: false` plus an
                    // informational error; only a reply without `exists` is fatal.
                    let Some(exists) = reply.payload.get("exists").and_then(Value::as_bool) else {
                        let error = reply
                            .error
                            .unwrap_or_else(|| "bridge reply had no `exists` field".to_string());
                        return Err(failed(explain(about(target, error))));
                    };
                    if exists == want {
                        return Ok(());
                    }
                }
                Err(_) if Instant::now() >= until => return Err(timed_out("wait: step timed out")),
                Err(error) => return Err(failed(explain(about(target, format!("{error:#}"))))),
            }
            thread::sleep(WAIT_POLL.min(left()));
        }
    }

    fn reload(&self, ctx: &StepCtx<'_>) -> StepOutcome {
        self.call(
            ctx,
            "evaluate",
            None,
            Some("setTimeout(() => location.reload(), 0), true".to_string()),
            Vec::new(),
        )?;
        thread::sleep(Duration::from_millis(300).min(ctx.remaining()));
        // The bridge comes back with the new page; wait until it has fully loaded.
        loop {
            if ctx.expired() {
                return Err(timed_out("reload: step timed out"));
            }
            let reply = self.request(
                ctx,
                "evaluate",
                None,
                Some("document.readyState".to_string()),
                ctx.remaining().min(RELOAD_POLL_TIMEOUT),
            );
            if let Ok(reply) = reply {
                if reply.ok && reply.payload.get("value") == Some(&json!("complete")) {
                    return Ok(());
                }
            }
            thread::sleep(WAIT_POLL.min(ctx.remaining()));
        }
    }

    /// Capture the target's ARIA snapshot until it matches the baseline, or settles when a
    /// baseline will be written, or the expect timeout runs out.
    fn expect_aria(&self, ctx: &mut StepCtx<'_>, expectation: &Expectation) -> StepOutcome {
        let snapshot = snapshot_of(ctx)?;
        let target = &expectation.target;
        let baseline = snapshot
            .read()
            .map_err(|error| failed(format!("{error:#}")))?
            .map(|bytes| normalize_aria(&String::from_utf8_lossy(&bytes)));
        let started = Instant::now();
        let give_up = (started + ctx.expect_timeout).min(ctx.deadline);
        let selector = bridge_selector(target);
        let mut last: Option<String> = None;
        let mut stable = false;
        let mut first = true;
        loop {
            let now = Instant::now();
            if now >= give_up && !first {
                break;
            }
            let budget = give_up.saturating_duration_since(now);
            let wait = if first {
                budget.max(MIN_INSPECT_TIMEOUT)
            } else {
                budget
            }
            .min(ctx.remaining());
            first = false;
            if wait.is_zero() {
                break;
            }
            match self.request(ctx, "ariaSnapshot", Some(selector.clone()), None, wait) {
                Ok(reply) if reply.ok => {
                    let Some(text) = reply.payload.get("snapshot").and_then(Value::as_str) else {
                        return Err(failed("bridge `ariaSnapshot` reply had no `snapshot`"));
                    };
                    let actual = normalize_aria(text);
                    let comparison = aria_comparison(baseline.as_deref(), &actual);
                    if decide(comparison, snapshot.update) == Decision::Pass {
                        return Ok(());
                    }
                    stable = last.as_deref() == Some(actual.as_str());
                    last = Some(actual);
                    // A mismatch may still turn into a match; only writes need a settled value.
                    if stable && decide(comparison, snapshot.update) != Decision::Fail {
                        break;
                    }
                }
                Ok(reply) if reply.error.as_deref() == Some(SELECTOR_NOT_FOUND) => {
                    last = None;
                    stable = false;
                }
                Ok(reply) => {
                    let error = reply
                        .error
                        .unwrap_or_else(|| "bridge returned ok=false".to_string());
                    return Err(failed(explain(about(target, error))));
                }
                Err(_) if Instant::now() >= give_up => break,
                Err(error) => return Err(failed(explain(about(target, format!("{error:#}"))))),
            }
            thread::sleep(WAIT_POLL.min(give_up.saturating_duration_since(Instant::now())));
        }
        let headline = snapshot_headline(expectation, started);
        let Some(actual) = last else {
            let info = ErrorInfo::new(format!("{headline}: `{target}` was not found"));
            return Err(cut_short(ctx, started, give_up, info));
        };
        let comparison = aria_comparison(baseline.as_deref(), &actual);
        let detail = match &baseline {
            Some(expected) => line_diff(expected, &actual),
            None => String::new(),
        };
        self.settle_snapshot(
            ctx,
            &snapshot,
            comparison,
            stable,
            Captured {
                headline,
                actual: actual.clone().into_bytes(),
                expected: baseline.map(String::into_bytes),
                diff: None,
                extension: "aria.yml",
                content_type: "text/yaml",
                detail,
                received: Value::String(actual),
            },
            (started, give_up),
        )
    }

    /// Capture stabilized screenshots of the target until one matches the baseline or two in a
    /// row are identical, then compare the last one (Playwright's `toHaveScreenshot`).
    fn expect_screenshot(
        &self,
        ctx: &mut StepCtx<'_>,
        expectation: &Expectation,
        options: &ScreenshotMatch,
    ) -> StepOutcome {
        let snapshot = snapshot_of(ctx)?;
        let target = &expectation.target;
        let started = Instant::now();
        let give_up = (started + ctx.expect_timeout).min(ctx.deadline);
        self.poll_until(ctx, target, true, true, give_up)
            .map_err(|error| match error {
                StepResult::TimedOut(_) => {
                    let info = ErrorInfo::new(format!(
                        "{}: `{target}` never became visible",
                        snapshot_headline(expectation, started)
                    ));
                    cut_short(ctx, started, give_up, info)
                }
                other => other,
            })?;
        let baseline_png = snapshot
            .read()
            .map_err(|error| failed(format!("{error:#}")))?;
        let baseline = baseline_png
            .as_deref()
            .map(Image::decode)
            .transpose()
            .map_err(|error| {
                failed(format!(
                    "snapshot `{}` is not a valid PNG: {error:#}",
                    snapshot.display
                ))
            })?;
        let tolerance = Tolerance {
            threshold: options.threshold,
            max_diff_pixels: options.max_diff_pixels,
            max_diff_pixel_ratio: options.max_diff_pixel_ratio,
        };
        let masks: Vec<String> = options.mask.iter().map(bridge_selector).collect();

        let selector = bridge_selector(target);
        let mut last: Option<(Vec<u8>, Image)> = None;
        let mut stable = false;
        let mut first = true;
        loop {
            let now = Instant::now();
            if now >= give_up && !first {
                break;
            }
            let budget = give_up.saturating_duration_since(now);
            let wait = if first {
                budget.max(CAPTURE_TIMEOUT)
            } else {
                budget
            }
            .min(ctx.remaining());
            first = false;
            if wait.is_zero() {
                break;
            }
            let shot = self.session.screenshot(
                Some(selector.clone()),
                Some("expect".to_string()),
                masks.clone(),
                ctx.test_id.clone(),
                Some(format!("step-{}", ctx.step_index)),
                wait,
            );
            let shot = match shot {
                Ok(shot) => shot,
                Err(error) if format!("{error:#}").contains(SELECTOR_NOT_FOUND) => {
                    last = None;
                    stable = false;
                    thread::sleep(WAIT_POLL.min(give_up.saturating_duration_since(Instant::now())));
                    continue;
                }
                Err(_) if Instant::now() >= give_up => break,
                Err(error) => return Err(failed(explain(about(target, format!("{error:#}"))))),
            };
            if shot
                .payload
                .get("screenshotBackend")
                .and_then(Value::as_str)
                == Some(DOM_SUMMARY_BACKEND)
            {
                let reason = shot
                    .payload
                    .get("nativeScreenshotError")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(failed(format!(
                    "toHaveScreenshot needs a native screenshot, but capturing one failed: {reason}"
                )));
            }
            if !masks.is_empty() && shot.payload.get("masked").is_none() {
                return Err(failed(
                    "this app's @auditaur/api cannot mask screenshots; upgrade it to use `mask`",
                ));
            }
            let image = Image::decode(&shot.png)
                .map_err(|error| failed(format!("bridge screenshot was not a PNG: {error:#}")))?;
            if let Some(expected) = &baseline {
                let comparison = if compare_images(expected, &image, tolerance).passed {
                    Comparison::Match
                } else {
                    Comparison::Mismatch
                };
                if decide(comparison, snapshot.update) == Decision::Pass {
                    return Ok(());
                }
            }
            stable = last
                .as_ref()
                .is_some_and(|(_, previous)| *previous == image);
            last = Some((shot.png, image));
            if stable {
                break;
            }
            thread::sleep(WAIT_POLL.min(give_up.saturating_duration_since(Instant::now())));
        }
        let headline = snapshot_headline(expectation, started);
        let Some((png, image)) = last else {
            let info = ErrorInfo::new(format!("{headline}: `{target}` was not found"));
            return Err(cut_short(ctx, started, give_up, info));
        };
        let (comparison, diff) = match &baseline {
            Some(expected) => {
                let diff = compare_images(expected, &image, tolerance);
                let comparison = if diff.passed {
                    Comparison::Match
                } else {
                    Comparison::Mismatch
                };
                (comparison, Some(diff))
            }
            None => (Comparison::Missing, None),
        };
        let received = match &diff {
            Some(diff) => json!({
                "diffPixels": diff.diff_pixels,
                "totalPixels": diff.total_pixels,
                "size": [image.width, image.height],
            }),
            None => json!({ "size": [image.width, image.height] }),
        };
        self.settle_snapshot(
            ctx,
            &snapshot,
            comparison,
            stable,
            Captured {
                headline,
                actual: png,
                expected: baseline_png,
                diff: diff.as_ref().and_then(|diff| diff.diff.encode().ok()),
                extension: "png",
                content_type: "image/png",
                detail: diff.as_ref().map(|diff| diff.summary()).unwrap_or_default(),
                received,
            },
            (started, give_up),
        )
    }

    /// Apply the update mode to a final comparison: pass, write the baseline, or fail with
    /// expected/actual/diff attachments.
    fn settle_snapshot(
        &self,
        ctx: &mut StepCtx<'_>,
        snapshot: &SnapshotTarget,
        comparison: Comparison,
        stable: bool,
        captured: Captured,
        (started, give_up): (Instant, Instant),
    ) -> StepOutcome {
        let decision = decide(comparison, snapshot.update);
        let writes = matches!(decision, Decision::Update | Decision::WriteMissing);
        let expected_info = json!({ "snapshot": snapshot.display });
        let attach = |ctx: &mut StepCtx<'_>, label: &str, bytes: &[u8], extension: &str, mime| {
            let file = ctx.artifact(&format!(
                "{}-{label}.{extension}",
                file_safe(&snapshot.name)
            ));
            let _ = ctx.writer.attach(label, mime, &file, bytes);
        };
        if writes && !stable {
            // Never record a baseline that was still changing.
            attach(
                ctx,
                "actual",
                &captured.actual,
                captured.extension,
                captured.content_type,
            );
            if comparison == Comparison::Match {
                return Ok(());
            }
            let info = ErrorInfo {
                message: format!(
                    "{}\nthe {} never stabilized, so `{}` was not written",
                    captured.headline,
                    snapshot_noun(snapshot),
                    snapshot.display
                ),
                // `stable: false` tells reviewers (the snapshots canvas) not to accept this actual.
                expected: Some(json!({ "snapshot": snapshot.display, "stable": false })),
                actual: Some(captured.received),
                ..ErrorInfo::default()
            };
            return Err(cut_short(ctx, started, give_up, info));
        }
        match decision {
            Decision::Pass => Ok(()),
            Decision::Update => snapshot
                .write(&captured.actual)
                .map_err(|error| failed(format!("{error:#}"))),
            Decision::WriteMissing | Decision::Fail if comparison == Comparison::Missing => {
                attach(
                    ctx,
                    "actual",
                    &captured.actual,
                    captured.extension,
                    captured.content_type,
                );
                let note = if decision == Decision::WriteMissing {
                    snapshot
                        .write(&captured.actual)
                        .map_err(|error| failed(format!("{error:#}")))?;
                    ctx.wrote_baseline = true;
                    "wrote the actual one; review it and re-run"
                } else {
                    "run with `--update-snapshots` to create it"
                };
                Err(StepResult::Failed(ErrorInfo {
                    message: format!(
                        "{}\nno baseline at `{}`; {note}",
                        captured.headline, snapshot.display
                    ),
                    expected: Some(expected_info),
                    actual: Some(captured.received),
                    ..ErrorInfo::default()
                }))
            }
            Decision::WriteMissing | Decision::Fail => {
                if let Some(expected) = &captured.expected {
                    attach(
                        ctx,
                        "expected",
                        expected,
                        captured.extension,
                        captured.content_type,
                    );
                }
                attach(
                    ctx,
                    "actual",
                    &captured.actual,
                    captured.extension,
                    captured.content_type,
                );
                if let Some(diff) = &captured.diff {
                    attach(ctx, "diff", diff, "png", "image/png");
                }
                let info = ErrorInfo {
                    message: format!(
                        "{}\n{} does not match `{}`:\n{}",
                        captured.headline,
                        snapshot_noun(snapshot),
                        snapshot.display,
                        captured.detail
                    ),
                    expected: Some(expected_info),
                    actual: Some(captured.received),
                    ..ErrorInfo::default()
                };
                Err(cut_short(ctx, started, give_up, info))
            }
        }
    }

    fn screenshot(
        &self,
        ctx: &mut StepCtx<'_>,
        name: &str,
        target: Option<&Locator>,
    ) -> StepOutcome {
        if let Some(target) = target {
            self.poll(ctx, target, true, true)
                .map_err(|error| match error {
                    StepResult::TimedOut(_) => timed_out(format!(
                        "screenshot: timed out waiting for `{target}` to be visible"
                    )),
                    other => other,
                })?;
        }
        let selector = target.map(bridge_selector);
        if ctx.expired() {
            return Err(timed_out("screenshot: step timed out"));
        }
        let png = self
            .session
            .screenshot_png(
                selector,
                ctx.test_id.clone(),
                Some(format!("step-{}", ctx.step_index)),
                ctx.remaining(),
            )
            .map_err(|error| failed(format!("{error:#}")))?;
        let file = ctx.artifact(&format!("{}.png", file_safe(name)));
        ctx.writer
            .attach(name, "image/png", &file, &png)
            .map_err(|error| failed(format!("could not save screenshot: {error:#}")))?;
        Ok(())
    }
}

/// The bridge's text-only fallback when native capture fails; useless for pixel comparison.
const DOM_SUMMARY_BACKEND: &str = "bridge_dom_summary_canvas";

/// A final snapshot capture and what to report about it.
struct Captured {
    headline: String,
    /// Bytes to write as the baseline.
    actual: Vec<u8>,
    expected: Option<Vec<u8>>,
    /// A PNG highlighting differing pixels (screenshots only).
    diff: Option<Vec<u8>>,
    extension: &'static str,
    content_type: &'static str,
    /// Why it mismatched: a line diff or a pixel summary.
    detail: String,
    received: Value,
}

fn snapshot_of(ctx: &StepCtx<'_>) -> std::result::Result<SnapshotTarget, StepResult> {
    ctx.snapshot
        .clone()
        .ok_or_else(|| failed("internal error: snapshot step without a snapshot target"))
}

fn aria_comparison(baseline: Option<&str>, actual: &str) -> Comparison {
    match baseline {
        None => Comparison::Missing,
        Some(expected) if expected == actual => Comparison::Match,
        Some(_) => Comparison::Mismatch,
    }
}

fn snapshot_headline(expectation: &Expectation, started: Instant) -> String {
    format!(
        "expect(`{}`).{}() failed after {}ms",
        expectation.target,
        expectation.matcher.key(),
        started.elapsed().as_millis()
    )
}

fn snapshot_noun(snapshot: &SnapshotTarget) -> &'static str {
    match snapshot.kind {
        super::snapshot::SnapshotKind::Aria => "ARIA snapshot",
        super::snapshot::SnapshotKind::Screenshot => "screenshot",
    }
}

/// A step or test deadline that cut the assertion short is a timeout, not a failure.
fn cut_short(ctx: &StepCtx<'_>, started: Instant, give_up: Instant, info: ErrorInfo) -> StepResult {
    if ctx.expired() && give_up < started + ctx.expect_timeout {
        StepResult::TimedOut(info)
    } else {
        StepResult::Failed(info)
    }
}

fn file_safe(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if safe.is_empty() {
        "screenshot".to_string()
    } else {
        safe
    }
}

/// What a failed xpectValue reports: the last value the expression produced (or what it
/// threw), not the last poll's bridge error, which is usually just a sliver-of-time timeout.
/// A bridge error after the last value comes back separately: the app may have stopped answering.
fn value_received(
    observed: Option<String>,
    bridge_error: Option<String>,
) -> (String, Option<String>) {
    match (observed, bridge_error) {
        (Some(value), after) => (value, after),
        (None, Some(error)) => (error, None),
        (None, None) => ("no time left to evaluate".to_string(), None),
    }
}

#[cfg(test)]
mod value_tests {
    use super::value_received;

    #[test]
    fn reports_the_last_observed_value_over_a_trailing_bridge_error() {
        let (actual, after) = value_received(
            Some("3".to_string()),
            Some("evaluate failed: Timed out after 114ms".to_string()),
        );
        assert_eq!(actual, "3");
        assert_eq!(
            after.as_deref(),
            Some("evaluate failed: Timed out after 114ms")
        );
    }

    #[test]
    fn falls_back_to_the_bridge_error_when_nothing_was_observed() {
        let (actual, after) = value_received(None, Some("evaluate failed: gone".to_string()));
        assert_eq!(actual, "evaluate failed: gone");
        assert_eq!(after, None);
        assert_eq!(value_received(None, None).0, "no time left to evaluate");
        assert_eq!(
            value_received(Some("threw: x".to_string()), None),
            ("threw: x".to_string(), None)
        );
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn launch_failure_quotes_the_app_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let command: Vec<String> = if cfg!(windows) {
            ["cmd", "/c", "echo panicked at src/main.rs & exit 101"]
        } else {
            ["sh", "-c", "echo panicked at src/main.rs; exit 101"]
        }
        .map(String::from)
        .to_vec();
        let mut host = RealHost {
            app: "auditaur-launch-log-test-no-such-app".into(),
            source: LaunchSource::Command(command.clone()),
            require_frontend: false,
            ready_timeout: Duration::from_secs(30),
            workspace_root: dir.path().to_path_buf(),
            cancel: Arc::new(AtomicBool::new(false)),
            log_dir: Some(dir.path().to_path_buf()),
            launches: 0,
            failed_launch_log: None,
        };

        let Err(error) = host.spawn(&command, &[], &mut |_| {}) else {
            panic!("the app exits before it is ready");
        };

        let message = format!("{error:#}");
        assert!(message.contains("exited"), "{message}");
        assert!(message.contains("panicked at src/main.rs"), "{message}");
        assert!(message.contains("app-1.log"), "{message}");
    }

    #[test]
    fn a_readiness_timeout_while_building_says_so() {
        let dir = tempfile::TempDir::new().unwrap();
        let command: Vec<String> = if cfg!(windows) {
            [
                "cmd",
                "/c",
                "echo    Compiling app v0.1.0 & ping -n 10 127.0.0.1 >nul",
            ]
        } else {
            ["sh", "-c", "echo '   Compiling app v0.1.0'; sleep 10"]
        }
        .map(String::from)
        .to_vec();
        let mut host = RealHost {
            app: "auditaur-launch-log-test-no-such-app".into(),
            source: LaunchSource::Command(command.clone()),
            require_frontend: false,
            ready_timeout: Duration::from_secs(2),
            workspace_root: dir.path().to_path_buf(),
            cancel: Arc::new(AtomicBool::new(false)),
            log_dir: Some(dir.path().to_path_buf()),
            launches: 0,
            failed_launch_log: None,
        };
        let mut seen = Vec::new();
        let Err(error) = host.spawn(&command, &[], &mut |line| seen.push(line.to_string())) else {
            panic!("no session ever appears");
        };
        let message = format!("{error:#}");
        assert!(message.contains("timed out after 2s"), "{message}");
        assert!(
            message.contains("still building (`Compiling app v0.1.0`)"),
            "{message}"
        );
        assert_eq!(seen, ["Compiling app v0.1.0"]);
    }

    #[test]
    fn launch_output_reports_new_lines_and_explains_a_slow_build() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("app-1.log");
        let mut seen = Vec::new();
        let mut progress = |line: &str| seen.push(line.to_string());
        let mut output = LaunchOutput::new(Some(&log), &mut progress);
        output.poll(); // no log yet
        fs::write(&log, "   \x1b[1m\x1b[32mCompiling\x1b[0m tauri v2.0.0\n").unwrap();
        output.poll();
        output.poll(); // unchanged: not reported again
        fs::write(
            &log,
            "Compiling tauri v2.0.0\r   Building [=====>  ] 410/520: app\r",
        )
        .unwrap();
        output.poll();
        let hint = output.building_hint();
        assert!(
            hint.contains("still building (`Building [=====>  ] 410/520: app`)"),
            "{hint}"
        );
        assert!(hint.contains("readiness.timeoutSeconds"), "{hint}");
        fs::write(&log, "Compiling app\nthread 'main' panicked\n").unwrap();
        assert_eq!(output.building_hint(), "");
        drop(output);
        assert_eq!(
            seen,
            [
                "Compiling tauri v2.0.0",
                "Building [=====>  ] 410/520: app",
                "thread 'main' panicked"
            ]
        );
    }

    #[test]
    fn last_line_cuts_long_lines() {
        let line = last_line(&"é".repeat(PROGRESS_LINE_MAX + 5)).unwrap();
        assert_eq!(line.chars().count(), PROGRESS_LINE_MAX + 1);
        assert!(line.ends_with('…'));
        assert_eq!(last_line(" \n\r "), None);
    }

    #[test]
    fn output_tail_keeps_the_end() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("app-1.log");
        fs::write(&log, format!("{}END", "x".repeat(LAUNCH_OUTPUT_SHOWN))).unwrap();
        let tail = output_tail(&log);
        assert!(tail.contains("…") && tail.ends_with("END"), "{tail}");
        fs::write(&log, "  \n").unwrap();
        assert!(output_tail(&log).contains("printed nothing"));
    }

    #[test]
    fn read_tail_reads_only_the_end_and_keeps_whole_characters() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("app-1.log");
        fs::write(&log, "short").unwrap();
        assert_eq!(read_tail(&log, 10).unwrap(), ("short".to_string(), false));
        // Multi-byte characters straddling the read window decode cleanly.
        fs::write(&log, format!("{}tail", "€".repeat(100))).unwrap();
        let (text, cut) = read_tail(&log, 6).unwrap();
        assert_eq!(text, "€€tail");
        assert!(cut);
        assert_eq!(read_tail(&log, 104).unwrap().1, false);
    }

    #[test]
    fn a_failed_launch_remembers_its_log_until_the_next_launch() {
        let dir = tempfile::TempDir::new().unwrap();
        let command: Vec<String> = if cfg!(windows) {
            ["cmd", "/c", "exit 3"]
        } else {
            ["sh", "-c", "exit 3"]
        }
        .map(String::from)
        .to_vec();
        let mut host = RealHost {
            app: "auditaur-launch-log-test-no-such-app".into(),
            source: LaunchSource::Command(command.clone()),
            require_frontend: false,
            ready_timeout: Duration::from_secs(30),
            workspace_root: dir.path().to_path_buf(),
            cancel: Arc::new(AtomicBool::new(false)),
            log_dir: Some(dir.path().to_path_buf()),
            launches: 0,
            failed_launch_log: None,
        };
        assert!(host.spawn(&command, &[], &mut |_| {}).is_err());
        assert_eq!(host.failed_launch_log, Some(dir.path().join("app-1.log")));
        // A config error before spawning must not reuse the old log.
        host.source = LaunchSource::Config(dir.path().join("missing.json"));
        let suite = crate::commands::drills::model::parse_str(
            Path::new("s.drill.json"),
            r##"{ "tests": [{ "name": "t", "steps": [{ "click": "#a" }] }] }"##,
        )
        .suite
        .unwrap();
        assert!(host.launch(&suite, &[], &mut |_| {}).is_err());
        assert_eq!(host.failed_launch_log, None);
    }
}

#[cfg(test)]
mod selector_tests {
    use super::*;

    fn spec(locator: Locator) -> Value {
        let selector = bridge_selector(&locator);
        let json = selector.strip_prefix(LOCATOR_PREFIX).expect("prefix");
        serde_json::from_str(json).expect("json")
    }

    #[test]
    fn css_omits_defaults() {
        assert_eq!(
            spec(Locator::Css("#a".into())),
            json!({"kind": "css", "value": "#a"})
        );
        assert_eq!(
            spec(Locator::CssNth {
                css: "li".into(),
                nth: -1
            }),
            json!({"kind": "css", "value": "li", "nth": -1})
        );
    }

    #[test]
    fn role_carries_name_exact_nth() {
        let locator = Locator::Role {
            role: "button".into(),
            name: Some("Save \"it\"".into()),
            exact: true,
            nth: Some(2),
        };
        assert_eq!(
            spec(locator),
            json!({"kind": "role", "value": "button", "name": "Save \"it\"", "exact": true, "nth": 2})
        );
    }

    #[test]
    fn within_nests_the_container_spec() {
        let parsed = crate::commands::drills::model::parse_locator_value(
            &json!({ "role": "button", "name": "Delete", "within": { "css": "li", "nth": 1, "within": "#list" } }),
        )
        .unwrap();
        assert_eq!(
            parsed.to_string(),
            "#list >> li >> nth=1 >> role=button[name=\"Delete\"i]"
        );
        assert_eq!(
            spec(parsed),
            json!({
                "kind": "role", "value": "button", "name": "Delete",
                "within": { "kind": "css", "value": "li", "nth": 1, "within": { "kind": "css", "value": "#list" } }
            })
        );
    }

    #[test]
    fn detects_within_only_in_locators() {
        use crate::commands::drills::model::{parse_locator_value, StepKind};
        let scoped = parse_locator_value(&json!({ "css": "button", "within": "#panel" })).unwrap();
        let plain = Locator::Css("button".into());
        assert!(StepKind::Drag {
            source: plain.clone(),
            to: scoped.clone()
        }
        .uses_within());
        assert!(StepKind::Press {
            key: "Enter".into(),
            target: Some(scoped)
        }
        .uses_within());
        assert!(!StepKind::Fill {
            target: plain,
            value: "Within { not a locator }".into()
        }
        .uses_within());
        assert!(!StepKind::Evaluate("Within {".into()).uses_within());
        assert!(bridge_ignores_within(&json!({ "exists": true })));
        assert!(!bridge_ignores_within(&json!({ "exists": false })));
    }

    #[test]
    fn every_kind_maps() {
        let text = Locator::Text {
            text: "Hi".into(),
            exact: false,
            nth: None,
        };
        assert_eq!(spec(text), json!({"kind": "text", "value": "Hi"}));
        let test_id = Locator::TestId {
            id: "t".into(),
            nth: Some(0),
        };
        assert_eq!(
            spec(test_id),
            json!({"kind": "testId", "value": "t", "nth": 0})
        );
        let label = Locator::Label {
            label: "Email".into(),
            exact: true,
            nth: None,
        };
        assert_eq!(
            spec(label),
            json!({"kind": "label", "value": "Email", "exact": true})
        );
        let placeholder = Locator::Placeholder {
            placeholder: "Search".into(),
            exact: false,
            nth: None,
        };
        assert_eq!(
            spec(placeholder),
            json!({"kind": "placeholder", "value": "Search"})
        );
    }

    #[test]
    fn explains_old_bridges() {
        let error = "click failed: 'locator:{}' is not a valid selector".to_string();
        assert!(explain(error).contains("upgrade it to 0.7.1 or newer"));
        assert!(explain(
            "ReferenceError: Cannot access 'selectorNotFoundMessage' before initialization".into()
        )
        .contains("upgrade it"));
        assert!(explain("unsupported drive bridge action: inspect".into()).contains("upgrade it"));
        assert_eq!(explain("selector not found".into()), "selector not found");
    }
}
