use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{anyhow, Context, Result};
use auditaur_core::{
    redaction::redact_json,
    storage::{FrontendErrorQuery, TauriIpcQuery},
};
use serde::Serialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    commands::{debug, drive, polish, read},
    discovery::{self, DiscoveredApp, DiscoveryStatus},
};

const EXIT_PASSED: i32 = 0;
const EXIT_CHECK_FAILED: i32 = 1;
const EXIT_RUNNER_ERROR: i32 = 2;
const EXIT_APP_EXITED: i32 = 3;
const EXIT_TIMEOUT: i32 = 4;
const EXIT_CLEANUP_FAILED: i32 = 5;

#[derive(Debug)]
pub struct DrillRunOptions {
    pub app: String,
    pub require_frontend: bool,
    pub require_drive_bridge: bool,
    pub timeout_seconds: u64,
    pub interval_ms: u64,
    pub report: PathBuf,
    pub selector: Option<String>,
    pub expect_text: Option<String>,
    pub json: bool,
    pub command: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillReport {
    schema_version: u8,
    generated_at_unix_nanos: i64,
    status: DrillStatus,
    exit_code: i32,
    app: DrillApp,
    command: DrillCommandReport,
    options: DrillOptionsReport,
    phases: Vec<DrillPhase>,
    summary: DrillSummary,
    artifacts: Vec<DrillArtifact>,
    redaction: DrillRedaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillStatus {
    Passed,
    Failed,
    Error,
    TimedOut,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillApp {
    service_name: String,
    session_id: Option<String>,
    instance_id: Option<String>,
    pid: Option<u32>,
    database_path: Option<String>,
    started_at: Option<String>,
    pinned_by: Option<String>,
    preexisting_matching_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillCommandReport {
    argv: Vec<String>,
    root_pid: Option<u32>,
    exit_code: Option<i32>,
    running_at_readiness: Option<bool>,
    timed_out: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillOptionsReport {
    require_frontend: bool,
    require_drive_bridge: bool,
    timeout_seconds: u64,
    selector: Option<String>,
    expect_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillPhase {
    id: String,
    kind: String,
    status: DrillPhaseStatus,
    duration_ms: u128,
    message: String,
    result: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillPhaseStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillSummary {
    passed: usize,
    failed: usize,
    skipped: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillArtifact {
    kind: String,
    path: String,
    redacted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillRedaction {
    defaults: bool,
    extra_keys: Vec<String>,
    redacted: bool,
}

pub fn run(options: DrillRunOptions) -> Result<i32> {
    let started_at = OffsetDateTime::now_utc();
    let deadline = Instant::now() + Duration::from_secs(options.timeout_seconds);
    let interval = Duration::from_millis(options.interval_ms).max(Duration::from_millis(100));
    let preexisting = matching_sessions(&options.app)?;
    let mut report = DrillReport::new(&options, &preexisting);
    let mut exit_code = EXIT_PASSED;

    let spawn_start = Instant::now();
    let mut child = match spawn_command(&options.command) {
        Ok(child) => {
            let root_pid = child.id();
            report.command.root_pid = Some(root_pid);
            report.push_phase(DrillPhase::passed(
                "spawn",
                "process",
                spawn_start,
                format!("started wrapped command as pid {root_pid}"),
                Some(json!({ "pid": root_pid })),
            ));
            Some(child)
        }
        Err(error) => {
            exit_code = EXIT_RUNNER_ERROR;
            report.push_phase(DrillPhase::failed(
                "spawn",
                "process",
                spawn_start,
                format!("failed to start wrapped command: {error}"),
                None,
            ));
            return finish(report, &options, exit_code);
        }
    };

    let pinned = wait_for_spawned_session(
        &options.app,
        started_at,
        &preexisting,
        deadline,
        interval,
        child.as_mut(),
        &mut report,
    )?;

    if let Some(app) = pinned {
        report.pin_app(&app);
        match wait_for_debug_ready(
            &options,
            &app,
            deadline,
            interval,
            child.as_mut(),
            &mut report,
        )? {
            WaitOutcome::Ready(status) => {
                report.command.running_at_readiness = match child.as_mut() {
                    Some(child) => child.try_wait()?.map(|_| false).or(Some(true)),
                    None => None,
                };
                report.push_phase(DrillPhase::passed(
                    "debug-readiness",
                    "debugStatus",
                    Instant::now(),
                    "debug readiness reached for spawned session",
                    Some(serde_json::to_value(status)?),
                ));
                run_post_readiness_phases(&options, &app, &mut report, &mut exit_code);
            }
            WaitOutcome::AppExited(code) => {
                exit_code = EXIT_APP_EXITED;
                report.command.exit_code = code;
                report.push_phase(DrillPhase::failed(
                    "debug-readiness",
                    "debugStatus",
                    Instant::now(),
                    "wrapped command exited before debug readiness",
                    Some(json!({ "exitCode": code })),
                ));
            }
            WaitOutcome::TimedOut(last_status) => {
                exit_code = EXIT_TIMEOUT;
                report.command.timed_out = true;
                report.push_phase(DrillPhase::failed(
                    "debug-readiness",
                    "debugStatus",
                    Instant::now(),
                    format!(
                        "timed out after {}s waiting for debug readiness",
                        options.timeout_seconds
                    ),
                    last_status
                        .map(|status| serde_json::to_value(status))
                        .transpose()?,
                ));
            }
        }
    } else {
        exit_code = if Instant::now() >= deadline {
            EXIT_TIMEOUT
        } else {
            EXIT_APP_EXITED
        };
    }

    if let Some(mut child) = child {
        let cleanup = cleanup_child_tree(&mut child);
        match cleanup {
            Ok(message) => report.push_phase(DrillPhase::passed(
                "cleanup",
                "processTree",
                Instant::now(),
                message,
                Some(json!({ "rootPid": report.command.root_pid })),
            )),
            Err(error) => {
                if exit_code == EXIT_PASSED {
                    exit_code = EXIT_CLEANUP_FAILED;
                }
                report.push_phase(DrillPhase::failed(
                    "cleanup",
                    "processTree",
                    Instant::now(),
                    format!("failed to clean up process tree: {error}"),
                    Some(json!({ "rootPid": report.command.root_pid })),
                ));
            }
        }
    }

    finish(report, &options, exit_code)
}

fn run_post_readiness_phases(
    options: &DrillRunOptions,
    app: &DiscoveredApp,
    report: &mut DrillReport,
    exit_code: &mut i32,
) {
    run_phase(report, exit_code, "drive-inspect", "driveInspect", || {
        drive::inspect_json_value(drive_selector(app))
    });

    if options.require_drive_bridge || options.selector.is_some() || options.expect_text.is_some() {
        run_phase_result(
            report,
            exit_code,
            "drive-bridge-responsive",
            "drivePing",
            || {
                let value = drive::ping_json_value(drive_selector(app))?;
                let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
                let status = value
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                Ok((
                    value,
                    ok,
                    if ok {
                        "drive bridge responded to ping".to_string()
                    } else {
                        format!("drive bridge action responsiveness failed: {status}")
                    },
                ))
            },
        );
    } else {
        report.push_phase(DrillPhase::skipped(
            "drive-bridge-responsive",
            "driveEvaluate",
            "drive bridge responsiveness was not required",
        ));
    }

    if options.selector.is_some() || options.expect_text.is_some() {
        let selector = options
            .selector
            .clone()
            .unwrap_or_else(|| "body".to_string());
        let expected = options.expect_text.clone();
        run_phase(report, exit_code, "drive-text", "driveText", || {
            let value = drive::text_json_value(
                drive_selector(app),
                drive::SelectorActionOptions {
                    selector: selector.clone(),
                    target_id: Some("auditaur-bridge".to_string()),
                    test_id: Some("auditaur-drill".to_string()),
                    step_id: Some("drive-text".to_string()),
                    visible_only: true,
                    json: true,
                },
            )?;
            if let Some(expected) = &expected {
                let text = value
                    .get("payload")
                    .and_then(|payload| payload.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !text.contains(expected) {
                    return Err(anyhow!(
                        "selector `{selector}` text did not contain expected text `{expected}`"
                    ));
                }
            }
            Ok(value)
        });
    } else {
        report.push_phase(DrillPhase::skipped(
            "drive-text",
            "driveText",
            "no selector or expected text was supplied",
        ));
    }

    run_telemetry_phases(options, app, report, exit_code);
}

fn run_telemetry_phases(
    _options: &DrillRunOptions,
    app: &DiscoveredApp,
    report: &mut DrillReport,
    exit_code: &mut i32,
) {
    let db = PathBuf::from(&app.database_path);
    run_phase_result(report, exit_code, "errors", "telemetryErrors", || {
        let store = read::open_validated_store(&db)?;
        let errors = store.list_frontend_errors(&FrontendErrorQuery {
            session_id: Some(app.session_id.clone()),
            trace_id: None,
            limit: Some(200),
        })?;
        let count = errors.len();
        Ok((
            json!({ "count": count, "items": errors }),
            count == 0,
            if count == 0 {
                "no frontend errors found".to_string()
            } else {
                format!("found {count} frontend error(s)")
            },
        ))
    });

    run_phase_result(report, exit_code, "failed-ipc", "telemetryIpc", || {
        let store = read::open_validated_store(&db)?;
        let calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
            session_id: Some(app.session_id.clone()),
            trace_id: None,
            limit: Some(200),
        })?;
        let failed = calls
            .into_iter()
            .filter(read::is_failed_ipc)
            .collect::<Vec<_>>();
        let count = failed.len();
        Ok((
            json!({ "count": count, "items": failed }),
            count == 0,
            if count == 0 {
                "no failed IPC calls found".to_string()
            } else {
                format!("found {count} failed IPC call(s)")
            },
        ))
    });

    run_phase(report, exit_code, "explain", "telemetryExplain", || {
        polish::explain_json_value(
            &Some(db.clone()),
            Some(app.session_id.clone()),
            None,
            None,
            200,
        )
    });
}

fn run_phase(
    report: &mut DrillReport,
    exit_code: &mut i32,
    id: &str,
    kind: &str,
    phase: impl FnOnce() -> Result<Value>,
) {
    let started = Instant::now();
    match phase() {
        Ok(value) => report.push_phase(DrillPhase::passed(
            id,
            kind,
            started,
            format!("{id} passed"),
            Some(value),
        )),
        Err(error) => {
            if *exit_code == EXIT_PASSED {
                *exit_code = EXIT_CHECK_FAILED;
            }
            report.push_phase(DrillPhase::failed(
                id,
                kind,
                started,
                error.to_string(),
                None,
            ));
        }
    }
}

fn run_phase_result(
    report: &mut DrillReport,
    exit_code: &mut i32,
    id: &str,
    kind: &str,
    phase: impl FnOnce() -> Result<(Value, bool, String)>,
) {
    let started = Instant::now();
    match phase() {
        Ok((value, true, message)) => {
            report.push_phase(DrillPhase::passed(id, kind, started, message, Some(value)))
        }
        Ok((value, false, message)) => {
            if *exit_code == EXIT_PASSED {
                *exit_code = EXIT_CHECK_FAILED;
            }
            report.push_phase(DrillPhase::failed(id, kind, started, message, Some(value)));
        }
        Err(error) => {
            if *exit_code == EXIT_PASSED {
                *exit_code = EXIT_CHECK_FAILED;
            }
            report.push_phase(DrillPhase::failed(
                id,
                kind,
                started,
                error.to_string(),
                None,
            ));
        }
    }
}

fn wait_for_spawned_session(
    app_name: &str,
    started_at: OffsetDateTime,
    preexisting: &[DiscoveredApp],
    deadline: Instant,
    interval: Duration,
    child: Option<&mut Child>,
    report: &mut DrillReport,
) -> Result<Option<DiscoveredApp>> {
    let started = Instant::now();
    let preexisting_ids = preexisting
        .iter()
        .map(|app| app.instance_id.as_str())
        .collect::<HashSet<_>>();
    let mut child = child;
    loop {
        if let Some(app) = find_spawned_session(app_name, started_at, &preexisting_ids)? {
            report.push_phase(DrillPhase::passed(
                "spawn-owned-session",
                "discovery",
                started,
                format!("pinned spawned Auditaur session {}", app.session_id),
                Some(json!({
                    "sessionId": app.session_id,
                    "instanceId": app.instance_id,
                    "pid": app.pid,
                    "databasePath": app.database_path,
                    "pinnedBy": "newDiscoveryAfterSpawn"
                })),
            ));
            return Ok(Some(app));
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.try_wait()? {
                report.command.exit_code = status.code();
                report.push_phase(DrillPhase::failed(
                    "spawn-owned-session",
                    "discovery",
                    started,
                    "wrapped command exited before a new Auditaur session was discovered",
                    Some(json!({ "exitCode": status.code() })),
                ));
                return Ok(None);
            }
        }
        if Instant::now() >= deadline {
            report.command.timed_out = true;
            report.push_phase(DrillPhase::failed(
                "spawn-owned-session",
                "discovery",
                started,
                "timed out waiting for a new Auditaur session after spawn; preexisting sessions were ignored",
                Some(json!({
                    "preexistingMatchingSessions": preexisting.len(),
                    "preexistingMatchingSessionIds": preexisting.iter().map(|app| app.session_id.clone()).collect::<Vec<_>>(),
                    "preexistingMatchingInstanceIds": preexisting.iter().map(|app| app.instance_id.clone()).collect::<Vec<_>>()
                })),
            ));
            return Ok(None);
        }
        thread::sleep(interval);
    }
}

enum WaitOutcome {
    Ready(debug::DebugStatus),
    AppExited(Option<i32>),
    TimedOut(Option<debug::DebugStatus>),
}

fn wait_for_debug_ready(
    options: &DrillRunOptions,
    app: &DiscoveredApp,
    deadline: Instant,
    interval: Duration,
    child: Option<&mut Child>,
    _report: &mut DrillReport,
) -> Result<WaitOutcome> {
    let selector = debug::DebugSelector {
        db: Some(PathBuf::from(&app.database_path)),
        app: None,
        session_id: Some(app.session_id.clone()),
        instance_id: Some(app.instance_id.clone()),
        pid: Some(app.pid),
        latest: false,
        active: false,
        cdp_port: None,
        require_frontend: options.require_frontend,
        require_drive_bridge: options.require_drive_bridge,
    };
    let mut child = child;
    loop {
        let status = debug::snapshot(&selector)?;
        if status.ready {
            return Ok(WaitOutcome::Ready(status));
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.try_wait()? {
                return Ok(WaitOutcome::AppExited(status.code()));
            }
        }
        if Instant::now() >= deadline {
            return Ok(WaitOutcome::TimedOut(Some(status)));
        }
        thread::sleep(interval);
    }
}

fn spawn_command(command: &[String]) -> Result<Child> {
    if command.is_empty() {
        return Err(anyhow!("drill run requires a command after `--`"));
    }
    let mut child_command = Command::new(&command[0]);
    child_command
        .args(&command[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    child_command.creation_flags(0x0800_0000);
    child_command
        .spawn()
        .with_context(|| format!("failed to start `{}`", command.join(" ")))
}

fn matching_sessions(app_name: &str) -> Result<Vec<DiscoveredApp>> {
    let mut apps = discovery::list_apps()?
        .into_iter()
        .filter(|app| app_matches(app, app_name))
        .collect::<Vec<_>>();
    apps.sort_by(|left, right| right.last_heartbeat_at.cmp(&left.last_heartbeat_at));
    Ok(apps)
}

fn find_spawned_session(
    app_name: &str,
    started_at: OffsetDateTime,
    preexisting_ids: &HashSet<&str>,
) -> Result<Option<DiscoveredApp>> {
    let mut candidates = discovery::list_apps()?
        .into_iter()
        .filter(|app| app_matches(app, app_name))
        .filter(|app| app.status == DiscoveryStatus::Active)
        .filter(|app| !preexisting_ids.contains(app.instance_id.as_str()))
        .filter(|app| app_started_or_heartbeat_after(app, started_at))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.last_heartbeat_at.cmp(&left.last_heartbeat_at));
    Ok(candidates.into_iter().next())
}

fn app_started_or_heartbeat_after(app: &DiscoveredApp, started_at: OffsetDateTime) -> bool {
    let cutoff = started_at - time::Duration::seconds(2);
    parse_rfc3339(&app.started_at).is_some_and(|time| time >= cutoff)
        || parse_rfc3339(&app.last_heartbeat_at).is_some_and(|time| time >= cutoff)
}

fn app_matches(app: &DiscoveredApp, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    app.service_name.to_ascii_lowercase().contains(&needle)
        || app
            .app_identifier
            .as_deref()
            .is_some_and(|identifier| identifier.to_ascii_lowercase().contains(&needle))
        || app.session_id.to_ascii_lowercase().contains(&needle)
}

fn parse_rfc3339(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn drive_selector(app: &DiscoveredApp) -> drive::DriveAppSelector {
    drive::DriveAppSelector {
        app: None,
        session_id: Some(app.session_id.clone()),
        instance_id: Some(app.instance_id.clone()),
        pid: Some(app.pid),
        latest: false,
        active: false,
    }
}

fn cleanup_child_tree(child: &mut Child) -> Result<String> {
    if let Some(status) = child.try_wait()? {
        return Ok(format!(
            "wrapped command already exited with status {status}"
        ));
    }
    let pid = child.id();
    cleanup_process_tree(pid)?;
    let _ = child.wait();
    Ok(format!("stopped process tree rooted at pid {pid}"))
}

#[cfg(windows)]
fn cleanup_process_tree(pid: u32) -> Result<()> {
    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("failed to invoke taskkill")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "taskkill failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
fn cleanup_process_tree(pid: u32) -> Result<()> {
    let _ = Command::new("pkill")
        .args(["-TERM", "-P", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to invoke kill")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("kill failed for pid {pid}"))
    }
}

fn finish(mut report: DrillReport, options: &DrillRunOptions, exit_code: i32) -> Result<i32> {
    report.exit_code = exit_code;
    report.status = status_for_exit_code(exit_code);
    report.summary = summarize_phases(&report.phases);
    report.generated_at_unix_nanos = read::current_time_unix_nanos();
    if let Some(parent) = options
        .report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    report.artifacts.push(DrillArtifact {
        kind: "json".to_string(),
        path: options.report.to_string_lossy().to_string(),
        redacted: true,
    });
    let value = redacted_report_value(&report)?;
    fs::write(&options.report, serde_json::to_vec_pretty(&value)?)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "Auditaur drill {:?}: {} passed, {} failed, {} skipped. Report: {}",
            report.status,
            report.summary.passed,
            report.summary.failed,
            report.summary.skipped,
            options.report.display()
        );
    }
    Ok(exit_code)
}

fn redacted_report_value(report: &DrillReport) -> Result<Value> {
    let value = serde_json::to_value(report)?;
    let result = redact_json(&value, &[]);
    let mut value = result.value;
    if let Some(redaction) = value.get_mut("redaction").and_then(Value::as_object_mut) {
        redaction.insert("redacted".to_string(), Value::Bool(result.redacted));
    }
    Ok(value)
}

fn summarize_phases(phases: &[DrillPhase]) -> DrillSummary {
    let mut summary = DrillSummary::default();
    for phase in phases {
        match phase.status {
            DrillPhaseStatus::Passed => summary.passed += 1,
            DrillPhaseStatus::Failed => summary.failed += 1,
            DrillPhaseStatus::Skipped => summary.skipped += 1,
        }
    }
    summary
}

fn status_for_exit_code(exit_code: i32) -> DrillStatus {
    match exit_code {
        EXIT_PASSED => DrillStatus::Passed,
        EXIT_CHECK_FAILED | EXIT_APP_EXITED | EXIT_CLEANUP_FAILED => DrillStatus::Failed,
        EXIT_TIMEOUT => DrillStatus::TimedOut,
        _ => DrillStatus::Error,
    }
}

impl DrillReport {
    fn new(options: &DrillRunOptions, preexisting: &[DiscoveredApp]) -> Self {
        Self {
            schema_version: 1,
            generated_at_unix_nanos: read::current_time_unix_nanos(),
            status: DrillStatus::Error,
            exit_code: EXIT_RUNNER_ERROR,
            app: DrillApp {
                service_name: options.app.clone(),
                session_id: None,
                instance_id: None,
                pid: None,
                database_path: None,
                started_at: None,
                pinned_by: None,
                preexisting_matching_sessions: preexisting
                    .iter()
                    .map(|app| app.session_id.clone())
                    .collect(),
            },
            command: DrillCommandReport {
                argv: options.command.clone(),
                root_pid: None,
                exit_code: None,
                running_at_readiness: None,
                timed_out: false,
            },
            options: DrillOptionsReport {
                require_frontend: options.require_frontend,
                require_drive_bridge: options.require_drive_bridge,
                timeout_seconds: options.timeout_seconds,
                selector: options.selector.clone(),
                expect_text: options.expect_text.clone(),
            },
            phases: Vec::new(),
            summary: DrillSummary::default(),
            artifacts: Vec::new(),
            redaction: DrillRedaction {
                defaults: true,
                extra_keys: Vec::new(),
                redacted: false,
            },
        }
    }

    fn pin_app(&mut self, app: &DiscoveredApp) {
        self.app.session_id = Some(app.session_id.clone());
        self.app.instance_id = Some(app.instance_id.clone());
        self.app.pid = Some(app.pid);
        self.app.database_path = Some(app.database_path.clone());
        self.app.started_at = Some(app.started_at.clone());
        self.app.pinned_by = Some("newDiscoveryAfterSpawn".to_string());
    }

    fn push_phase(&mut self, phase: DrillPhase) {
        self.phases.push(phase);
    }
}

impl DrillPhase {
    fn passed(
        id: impl Into<String>,
        kind: impl Into<String>,
        started: Instant,
        message: impl Into<String>,
        result: Option<Value>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: DrillPhaseStatus::Passed,
            duration_ms: started.elapsed().as_millis(),
            message: message.into(),
            result,
        }
    }

    fn failed(
        id: impl Into<String>,
        kind: impl Into<String>,
        started: Instant,
        message: impl Into<String>,
        result: Option<Value>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: DrillPhaseStatus::Failed,
            duration_ms: started.elapsed().as_millis(),
            message: message.into(),
            result,
        }
    }

    fn skipped(id: impl Into<String>, kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: DrillPhaseStatus::Skipped,
            duration_ms: 0,
            message: message.into(),
            result: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_map_to_status() {
        assert_eq!(status_for_exit_code(EXIT_PASSED), DrillStatus::Passed);
        assert_eq!(status_for_exit_code(EXIT_CHECK_FAILED), DrillStatus::Failed);
        assert_eq!(status_for_exit_code(EXIT_TIMEOUT), DrillStatus::TimedOut);
        assert_eq!(status_for_exit_code(EXIT_RUNNER_ERROR), DrillStatus::Error);
    }

    #[test]
    fn summarizes_phase_statuses() {
        let phases = vec![
            DrillPhase::passed("a", "kind", Instant::now(), "ok", None),
            DrillPhase::failed("b", "kind", Instant::now(), "bad", None),
            DrillPhase::skipped("c", "kind", "skip"),
        ];
        let summary = summarize_phases(&phases);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn redacts_report_payloads() {
        let options = DrillRunOptions {
            app: "fixture".to_string(),
            require_frontend: false,
            require_drive_bridge: false,
            timeout_seconds: 1,
            interval_ms: 100,
            report: PathBuf::from("report.json"),
            selector: None,
            expect_text: None,
            json: false,
            command: vec!["fixture".to_string()],
        };
        let mut report = DrillReport::new(&options, &[]);
        report.push_phase(DrillPhase::passed(
            "secret-phase",
            "test",
            Instant::now(),
            "ok",
            Some(json!({ "token": "abc", "nested": { "api_key": "def" } })),
        ));
        let value = redacted_report_value(&report).unwrap();
        assert_eq!(
            value["phases"][0]["result"]["token"],
            Value::String("[REDACTED]".to_string())
        );
        assert_eq!(
            value["phases"][0]["result"]["nested"]["api_key"],
            Value::String("[REDACTED]".to_string())
        );
    }

    #[test]
    fn new_run_selector_accepts_started_or_heartbeat_after_spawn() {
        let started_at = OffsetDateTime::parse("2026-06-27T22:00:00Z", &Rfc3339).unwrap();
        let app = discovered_app("2026-06-27T21:59:00Z", "2026-06-27T22:00:01Z");

        assert!(app_started_or_heartbeat_after(&app, started_at));
    }

    #[test]
    fn new_run_selector_rejects_pre_spawn_discovery_times() {
        let started_at = OffsetDateTime::parse("2026-06-27T22:00:00Z", &Rfc3339).unwrap();
        let app = discovered_app("2026-06-27T21:59:00Z", "2026-06-27T21:59:30Z");

        assert!(!app_started_or_heartbeat_after(&app, started_at));
    }

    fn discovered_app(started_at: &str, last_heartbeat_at: &str) -> DiscoveredApp {
        DiscoveredApp {
            instance_id: "instance".to_string(),
            session_id: "session".to_string(),
            service_name: "fixture".to_string(),
            service_version: None,
            app_identifier: None,
            pid: 42,
            started_at: started_at.to_string(),
            last_heartbeat_at: last_heartbeat_at.to_string(),
            heartbeat_age_seconds: Some(0),
            status: DiscoveryStatus::Active,
            capabilities: Vec::new(),
            database_path: "telemetry.sqlite".to_string(),
            database_readable: true,
            schema_valid: true,
            discovery_path: "discovery.json".to_string(),
            stale_reason: None,
            superseded_by_session_id: None,
            seconds_until_next_start: None,
            churn_session_count: None,
            churn_window_seconds: None,
            churn_hint: None,
        }
    }
}
