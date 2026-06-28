use std::{
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{anyhow, Context, Result};
use auditaur_core::{
    drive_bridge::{
        DriveBridgeStatus, DRIVE_BRIDGE_DIR, DRIVE_BRIDGE_STALE_FILE_NANOS,
        DRIVE_BRIDGE_STATUS_FILE,
    },
    model::TelemetrySource,
    storage::{
        FrontendErrorQuery, LogQuery, SpanEventQuery, SpanQuery, TauriEventQuery, TauriIpcQuery,
        TauriWindowQuery,
    },
};
use serde::Serialize;

use crate::{
    commands::read,
    discovery::{self, DiscoveredApp, DiscoveryStatus},
    output::table_cell,
};

#[derive(Debug, Clone)]
pub struct DebugSelector {
    pub db: Option<PathBuf>,
    pub app: Option<String>,
    pub session_id: Option<String>,
    pub instance_id: Option<String>,
    pub pid: Option<u32>,
    pub latest: bool,
    pub active: bool,
    pub cdp_port: Option<u16>,
    pub require_frontend: bool,
    pub require_drive_bridge: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugStatus {
    pub schema_version: u8,
    pub generated_at_unix_nanos: i64,
    pub ready: bool,
    pub app: Option<DiscoveredApp>,
    pub database_path: Option<String>,
    pub session_id: Option<String>,
    pub stages: Vec<DebugStage>,
    pub telemetry: DebugTelemetryCounts,
    pub cdp: Option<DebugCdpStatus>,
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugStage {
    pub name: String,
    pub status: DebugStageStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DebugStageStatus {
    Ok,
    Waiting,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugTelemetryCounts {
    pub sessions: usize,
    pub logs: usize,
    pub spans: usize,
    pub span_events: usize,
    pub frontend_errors: usize,
    pub ipc: usize,
    pub events: usize,
    pub windows: usize,
    pub frontend_records: usize,
    pub backend_records: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugCdpStatus {
    pub port: u16,
    pub ok: bool,
    pub target_count: Option<usize>,
    pub browser: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugRunResult {
    status: DebugStatus,
    process: DebugProcessStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugProcessStatus {
    command: Vec<String>,
    pid: Option<u32>,
    exit_code: Option<i32>,
    running: bool,
}

pub fn status(selector: DebugSelector, json: bool) -> Result<()> {
    let status = snapshot(&selector)?;
    read::print_json_or_table(json, &status, || print_status(&status))
}

pub fn watch(
    selector: DebugSelector,
    interval_ms: u64,
    timeout_seconds: Option<u64>,
    until_ready: bool,
    json: bool,
) -> Result<()> {
    let started = Instant::now();
    let interval = Duration::from_millis(interval_ms).max(Duration::from_millis(100));
    loop {
        let status = snapshot(&selector)?;
        if json {
            println!("{}", serde_json::to_string(&status)?);
        } else {
            print_status(&status)?;
            println!();
        }
        if until_ready && status.ready {
            return Ok(());
        }
        if !until_ready {
            return Ok(());
        }
        if timeout_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            return Err(anyhow!(
                "Timed out after {}s waiting for Auditaur debug readiness.",
                timeout_seconds.unwrap()
            ));
        }
        thread::sleep(interval);
    }
}

pub fn run(
    selector: DebugSelector,
    interval_ms: u64,
    timeout_seconds: Option<u64>,
    json: bool,
    command: Vec<String>,
) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!(
            "`auditaur debug run` requires a command after `--`."
        ));
    }
    let mut child_command = Command::new(&command[0]);
    child_command.args(&command[1..]);
    if json {
        child_command.stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        child_command.creation_flags(0x0800_0000);
    } else {
        child_command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
    let mut child = child_command
        .spawn()
        .with_context(|| format!("failed to start `{}`", command.join(" ")))?;
    let pid = child.id();
    let started = Instant::now();
    let interval = Duration::from_millis(interval_ms).max(Duration::from_millis(100));
    let mut final_status;
    loop {
        final_status = match snapshot(&selector) {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.context("debug readiness snapshot failed after starting command"));
            }
        };
        if json {
            println!("{}", serde_json::to_string(&final_status)?);
        } else {
            print_status(&final_status)?;
            println!();
        }
        if final_status.ready {
            break;
        }
        if let Some(status) = child.try_wait()? {
            let result = DebugRunResult {
                status: final_status,
                process: DebugProcessStatus {
                    command,
                    pid: Some(pid),
                    exit_code: status.code(),
                    running: false,
                },
            };
            if json {
                println!("{}", serde_json::to_string(&result)?);
            }
            return Err(anyhow!(
                "debug command exited before Auditaur became ready with status {status}"
            ));
        }
        if timeout_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!(
                "Timed out after {}s waiting for Auditaur debug readiness.",
                timeout_seconds.unwrap()
            ));
        }
        thread::sleep(interval);
    }

    let child_status = child.try_wait()?;
    let process = DebugProcessStatus {
        command,
        pid: Some(pid),
        exit_code: child_status.and_then(|status| status.code()),
        running: child_status.is_none(),
    };
    let result = DebugRunResult {
        status: final_status,
        process,
    };
    if json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("Auditaur debug session ready.");
    }
    Ok(())
}

pub(crate) fn snapshot(selector: &DebugSelector) -> Result<DebugStatus> {
    let mut stages = Vec::new();
    let mut hints = Vec::new();
    let app = if selector.db.is_some() {
        stages.push(stage(
            "app_discovery",
            DebugStageStatus::Skipped,
            "using explicit --db; app discovery is not required",
        ));
        stages.push(stage(
            "heartbeat",
            DebugStageStatus::Skipped,
            "using explicit --db; app heartbeat is not required",
        ));
        None
    } else {
        match select_app(selector)? {
            Some(app) => {
                stages.push(stage(
                    "app_discovery",
                    DebugStageStatus::Ok,
                    format!(
                        "discovered {} pid={} session={}",
                        app.service_name, app.pid, app.session_id
                    ),
                ));
                Some(app)
            }
            None => {
                stages.push(stage(
                    "app_discovery",
                    DebugStageStatus::Waiting,
                    "waiting for an Auditaur discovery file",
                ));
                hints.push(
                    "Start the app or use `auditaur debug run -- <command>` to wrap startup."
                        .to_string(),
                );
                None
            }
        }
    };

    let db = selector
        .db
        .clone()
        .or_else(|| app.as_ref().map(|app| PathBuf::from(&app.database_path)));
    let db_for_drive_bridge = db.clone();
    if let Some(app) = &app {
        if app.status == DiscoveryStatus::Active {
            stages.push(stage(
                "heartbeat",
                DebugStageStatus::Ok,
                format!(
                    "heartbeat is fresh{}",
                    app.heartbeat_age_seconds
                        .map(|age| format!(" ({age}s old)"))
                        .unwrap_or_default()
                ),
            ));
        } else {
            stages.push(stage(
                "heartbeat",
                DebugStageStatus::Waiting,
                app.stale_reason
                    .clone()
                    .unwrap_or_else(|| "heartbeat is stale".to_string()),
            ));
        }
    }

    let (database_path, telemetry, session_id) = match db {
        Some(db) => load_database_status(&db, selector, &mut stages, &mut hints)?,
        None => {
            stages.push(stage(
                "telemetry_database",
                DebugStageStatus::Waiting,
                "waiting for a telemetry database path",
            ));
            (None, DebugTelemetryCounts::default(), None)
        }
    };

    push_drive_bridge_stage(
        db_for_drive_bridge.as_deref(),
        selector.require_drive_bridge,
        &mut stages,
        &mut hints,
    );

    if let Some(cdp_port) = selector.cdp_port {
        let cdp = cdp_status(cdp_port);
        stages.push(stage(
            "cdp_endpoint",
            if cdp.ok {
                DebugStageStatus::Ok
            } else {
                DebugStageStatus::Waiting
            },
            cdp.message.clone(),
        ));
        let ready = readiness(
            &stages,
            selector.require_frontend,
            selector.require_drive_bridge,
        );
        Ok(DebugStatus {
            schema_version: 1,
            generated_at_unix_nanos: read::current_time_unix_nanos(),
            ready,
            app,
            database_path,
            session_id,
            stages,
            telemetry,
            cdp: Some(cdp),
            hints,
        })
    } else {
        stages.push(stage(
            "cdp_endpoint",
            DebugStageStatus::Skipped,
            "pass --cdp-port to include WebView/CDP readiness",
        ));
        let ready = readiness(
            &stages,
            selector.require_frontend,
            selector.require_drive_bridge,
        );
        Ok(DebugStatus {
            schema_version: 1,
            generated_at_unix_nanos: read::current_time_unix_nanos(),
            ready,
            app,
            database_path,
            session_id,
            stages,
            telemetry,
            cdp: None,
            hints,
        })
    }
}

fn push_drive_bridge_stage(
    db: Option<&Path>,
    required: bool,
    stages: &mut Vec<DebugStage>,
    hints: &mut Vec<String>,
) {
    let Some(db) = db else {
        stages.push(stage(
            "drive_bridge",
            if required {
                DebugStageStatus::Waiting
            } else {
                DebugStageStatus::Skipped
            },
            "waiting for a telemetry database path before checking drive bridge readiness",
        ));
        return;
    };
    let Some(data_dir) = db.parent() else {
        stages.push(stage(
            "drive_bridge",
            DebugStageStatus::Error,
            format!(
                "telemetry database path has no parent directory: {}",
                db.display()
            ),
        ));
        return;
    };

    let status_path = data_dir
        .join(DRIVE_BRIDGE_DIR)
        .join(DRIVE_BRIDGE_STATUS_FILE);
    if !status_path.is_file() {
        stages.push(stage(
            "drive_bridge",
            if required {
                DebugStageStatus::Waiting
            } else {
                DebugStageStatus::Skipped
            },
            "drive bridge status file has not been written; enable initAuditaur({ driveBridge: true }) to drive the WebView",
        ));
        if required {
            hints.push("Enable `initAuditaur({ driveBridge: true })` in exactly one debug/test WebView for this Auditaur session.".to_string());
        }
        return;
    }

    let status = match std::fs::read_to_string(&status_path)
        .with_context(|| {
            format!(
                "failed to read drive bridge status {}",
                status_path.display()
            )
        })
        .and_then(|contents| {
            serde_json::from_str::<DriveBridgeStatus>(&contents).with_context(|| {
                format!(
                    "failed to parse drive bridge status {}",
                    status_path.display()
                )
            })
        }) {
        Ok(status) => status,
        Err(error) => {
            stages.push(stage(
                "drive_bridge",
                DebugStageStatus::Error,
                format!("drive bridge status is unreadable: {error}"),
            ));
            return;
        }
    };

    let heartbeat_age_nanos = read::current_time_unix_nanos() - status.last_heartbeat_unix_nanos;
    let heartbeat_fresh = heartbeat_age_nanos <= DRIVE_BRIDGE_STALE_FILE_NANOS;
    let actionable = status.active && heartbeat_fresh;
    let stage_status = if actionable {
        DebugStageStatus::Ok
    } else if required {
        DebugStageStatus::Waiting
    } else {
        DebugStageStatus::Skipped
    };
    let message = if !status.active {
        "drive bridge is registered but inactive".to_string()
    } else if !heartbeat_fresh {
        "drive bridge heartbeat is stale".to_string()
    } else {
        format!(
            "drive bridge is active{}",
            status
                .window_label
                .as_ref()
                .map(|label| format!(" for window `{label}`"))
                .unwrap_or_default()
        )
    };
    stages.push(stage("drive_bridge", stage_status, message));
}

fn load_database_status(
    db: &Path,
    selector: &DebugSelector,
    stages: &mut Vec<DebugStage>,
    hints: &mut Vec<String>,
) -> Result<(Option<String>, DebugTelemetryCounts, Option<String>)> {
    if !db.is_file() {
        stages.push(stage(
            "telemetry_database",
            DebugStageStatus::Waiting,
            format!("waiting for telemetry database: {}", db.display()),
        ));
        return Ok((
            Some(db.to_string_lossy().to_string()),
            DebugTelemetryCounts::default(),
            None,
        ));
    }

    let store = match read::open_validated_store(db) {
        Ok(store) => {
            stages.push(stage(
                "telemetry_database",
                DebugStageStatus::Ok,
                format!("database is readable and schema-valid: {}", db.display()),
            ));
            store
        }
        Err(error) => {
            stages.push(stage(
                "telemetry_database",
                DebugStageStatus::Error,
                format!("database could not be opened or validated: {error}"),
            ));
            return Ok((
                Some(db.to_string_lossy().to_string()),
                DebugTelemetryCounts::default(),
                None,
            ));
        }
    };

    let sessions = store.list_sessions(Some(50))?;
    let session_id = selector
        .session_id
        .clone()
        .or_else(|| sessions.first().map(|session| session.id.clone()));
    if let Some(session_id) = &session_id {
        stages.push(stage(
            "session",
            DebugStageStatus::Ok,
            format!("session is queryable: {session_id}"),
        ));
    } else {
        stages.push(stage(
            "session",
            DebugStageStatus::Waiting,
            "waiting for the first session row",
        ));
    }

    let telemetry = telemetry_counts(&store, session_id.clone())?;
    if telemetry.windows > 0 {
        stages.push(stage(
            "window",
            DebugStageStatus::Ok,
            format!("{} window record(s) captured", telemetry.windows),
        ));
    } else {
        stages.push(stage(
            "window",
            DebugStageStatus::Waiting,
            "waiting for Tauri window telemetry",
        ));
    }

    if telemetry.backend_records > 0 {
        stages.push(stage(
            "backend_telemetry",
            DebugStageStatus::Ok,
            format!(
                "{} backend/plugin record(s) captured",
                telemetry.backend_records
            ),
        ));
    } else {
        stages.push(stage(
            "backend_telemetry",
            DebugStageStatus::Waiting,
            "waiting for backend/plugin telemetry rows",
        ));
    }

    if telemetry.frontend_records > 0 {
        stages.push(stage(
            "frontend_telemetry",
            DebugStageStatus::Ok,
            format!("{} frontend record(s) captured", telemetry.frontend_records),
        ));
    } else {
        stages.push(stage(
            "frontend_telemetry",
            if selector.require_frontend {
                DebugStageStatus::Waiting
            } else {
                DebugStageStatus::Skipped
            },
            "no frontend telemetry rows observed yet",
        ));
        hints.push(
            "If frontend telemetry is expected, click a webview action and inspect app UI/export errors."
                .to_string(),
        );
    }

    Ok((
        Some(db.to_string_lossy().to_string()),
        DebugTelemetryCounts {
            sessions: sessions.len(),
            ..telemetry
        },
        session_id,
    ))
}

fn telemetry_counts(
    store: &auditaur_collector::exporter_sqlite::SqliteStore,
    session_id: Option<String>,
) -> Result<DebugTelemetryCounts> {
    let logs = store.list_logs(&LogQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let spans = store.list_spans(&SpanQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let span_events = store.list_span_events(&SpanEventQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let frontend_errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let ipc = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let events = store.list_tauri_events(&TauriEventQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let windows = store.list_tauri_windows(&TauriWindowQuery {
        session_id,
        latest_only: false,
        limit: Some(usize::MAX),
    })?;
    let frontend_logs = logs
        .iter()
        .filter(|log| log.source == TelemetrySource::Frontend)
        .count();
    let backend_logs = logs.len().saturating_sub(frontend_logs);
    let frontend_spans = spans
        .iter()
        .filter(|span| span.source == TelemetrySource::Frontend)
        .count();
    let backend_spans = spans.len().saturating_sub(frontend_spans);
    Ok(DebugTelemetryCounts {
        sessions: 0,
        logs: logs.len(),
        spans: spans.len(),
        span_events: span_events.len(),
        frontend_errors: frontend_errors.len(),
        ipc: ipc.len(),
        events: events.len(),
        windows: windows.len(),
        frontend_records: frontend_logs
            + frontend_spans
            + frontend_errors.len()
            + ipc.len()
            + events.len(),
        backend_records: backend_logs + backend_spans + span_events.len() + windows.len(),
    })
}

fn select_app(selector: &DebugSelector) -> Result<Option<DiscoveredApp>> {
    let mut candidates: Vec<_> = discovery::list_apps()?
        .into_iter()
        .filter(|candidate| {
            selector
                .app
                .as_deref()
                .is_none_or(|needle| app_matches(candidate, needle))
        })
        .filter(|candidate| {
            selector
                .session_id
                .as_deref()
                .is_none_or(|needle| candidate.session_id.contains(needle))
        })
        .filter(|candidate| {
            selector
                .instance_id
                .as_deref()
                .is_none_or(|needle| candidate.instance_id.contains(needle))
        })
        .filter(|candidate| selector.pid.is_none_or(|pid| candidate.pid == pid))
        .filter(|candidate| !selector.active || candidate.status == DiscoveryStatus::Active)
        .collect();
    candidates.sort_by(|left, right| {
        let left_active = left.status == DiscoveryStatus::Active;
        let right_active = right.status == DiscoveryStatus::Active;
        right_active
            .cmp(&left_active)
            .then_with(|| right.last_heartbeat_at.cmp(&left.last_heartbeat_at))
    });
    if selector.latest {
        return Ok(candidates.into_iter().next());
    }
    let active_count = candidates
        .iter()
        .filter(|candidate| candidate.status == DiscoveryStatus::Active)
        .count();
    match candidates.as_slice() {
        [] => Ok(None),
        [candidate] => Ok(Some(candidate.clone())),
        _ if active_count == 1 => Ok(candidates
            .into_iter()
            .find(|candidate| candidate.status == DiscoveryStatus::Active)),
        _ => Err(anyhow!(
            "Multiple Auditaur apps matched debug selector. Pass --session-id, --instance-id, --pid, --latest, or --active."
        )),
    }
}

fn app_matches(candidate: &DiscoveredApp, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    candidate
        .service_name
        .to_ascii_lowercase()
        .contains(&needle)
        || candidate
            .app_identifier
            .as_deref()
            .is_some_and(|identifier| identifier.to_ascii_lowercase().contains(&needle))
        || candidate.session_id.to_ascii_lowercase().contains(&needle)
}

fn cdp_status(port: u16) -> DebugCdpStatus {
    match get_cdp_json(port, "/json/version")
        .and_then(|version| get_cdp_json(port, "/json/list").map(|targets| (version, targets)))
    {
        Ok((version, targets)) => {
            let target_count = targets.as_array().map(Vec::len);
            let browser = version
                .get("Browser")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            DebugCdpStatus {
                port,
                ok: true,
                target_count,
                browser,
                message: format!(
                    "CDP endpoint is reachable on port {port} with {} target(s)",
                    target_count.unwrap_or_default()
                ),
            }
        }
        Err(error) => DebugCdpStatus {
            port,
            ok: false,
            target_count: None,
            browser: None,
            message: format!("waiting for CDP endpoint on port {port}: {error}"),
        },
    }
}

fn get_cdp_json(port: u16, path: &str) -> Result<serde_json::Value> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse()?,
        Duration::from_millis(500),
    )?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        response.push(byte[0]);
    }
    let headers = String::from_utf8(response)?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or_else(|| anyhow!("CDP response did not include Content-Length"))?;
    let mut body = vec![0_u8; content_length];
    stream.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

fn readiness(stages: &[DebugStage], require_frontend: bool, require_drive_bridge: bool) -> bool {
    let required = [
        "app_discovery",
        "heartbeat",
        "telemetry_database",
        "session",
        "window",
        "backend_telemetry",
        "cdp_endpoint",
    ];
    required.iter().all(|name| {
        stages
            .iter()
            .find(|stage| stage.name == *name)
            .is_some_and(|stage| {
                stage.status == DebugStageStatus::Ok || stage.status == DebugStageStatus::Skipped
            })
    }) && (!require_frontend
        || stages
            .iter()
            .find(|stage| stage.name == "frontend_telemetry")
            .is_some_and(|stage| stage.status == DebugStageStatus::Ok))
        && (!require_drive_bridge
            || stages
                .iter()
                .find(|stage| stage.name == "drive_bridge")
                .is_some_and(|stage| stage.status == DebugStageStatus::Ok))
}

fn stage(
    name: impl Into<String>,
    status: DebugStageStatus,
    message: impl Into<String>,
) -> DebugStage {
    DebugStage {
        name: name.into(),
        status,
        message: message.into(),
    }
}

fn print_status(status: &DebugStatus) -> Result<()> {
    let ok = status
        .stages
        .iter()
        .filter(|stage| stage.status == DebugStageStatus::Ok)
        .count();
    let waiting = status
        .stages
        .iter()
        .filter(|stage| stage.status == DebugStageStatus::Waiting)
        .count();
    let errors = status
        .stages
        .iter()
        .filter(|stage| stage.status == DebugStageStatus::Error)
        .count();
    println!(
        "Auditaur debug: {} (ok={} waiting={} errors={})",
        if status.ready { "ready" } else { "waiting" },
        ok,
        waiting,
        errors
    );
    for stage in &status.stages {
        println!(
            "{}\t{:?}\t{}",
            table_cell(&stage.name, 24),
            stage.status,
            table_cell(&stage.message, 96)
        );
    }
    println!(
        "telemetry\tsessions={} logs={} spans={} span_events={} frontend_errors={} ipc={} events={} windows={}",
        status.telemetry.sessions,
        status.telemetry.logs,
        status.telemetry.spans,
        status.telemetry.span_events,
        status.telemetry.frontend_errors,
        status.telemetry.ipc,
        status.telemetry.events,
        status.telemetry.windows
    );
    for hint in &status.hints {
        println!("hint\t{}", table_cell(hint, 120));
    }
    Ok(())
}
