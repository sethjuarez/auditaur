use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::{
    model::{
        FrontendError, LogRecord, Session, SpanRecord, TauriEventRecord, TauriIpcCall,
        TauriWindowState,
    },
    protocol::TraceSummary,
    storage::{
        FrontendErrorQuery, LogQuery, SpanQuery, TauriEventQuery, TauriIpcQuery, TauriWindowQuery,
    },
};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::{discovery, output::table_cell};

pub fn apps(json: bool) -> Result<()> {
    let apps = discovery::list_apps()?;
    print_json_or_table(json, &apps, || print_apps(&apps))
}

pub fn sessions(db: &Option<PathBuf>, json: bool, limit: usize) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let sessions = store.list_sessions(Some(limit))?;
    print_json_or_table(json, &sessions, || print_sessions(&sessions))
}

pub fn logs(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let logs = store.list_logs(&LogQuery {
        session_id,
        trace_id,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &logs, || print_logs(&logs))
}

pub fn errors(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id,
        trace_id,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &errors, || print_errors(&errors))
}

pub fn traces(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let summaries = store.list_trace_summaries(session_id.as_deref(), Some(limit))?;
    print_json_or_table(json, &summaries, || print_traces(&summaries))
}

pub fn trace(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: String,
    json: bool,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let spans = store.list_spans(&SpanQuery {
        session_id: session_id.clone(),
        trace_id: Some(trace_id.clone()),
        limit: Some(usize::MAX),
    })?;
    let logs = store.list_logs(&LogQuery {
        session_id: session_id.clone(),
        trace_id: Some(trace_id.clone()),
        limit: Some(usize::MAX),
    })?;
    let errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id: session_id.clone(),
        trace_id: Some(trace_id.clone()),
        limit: Some(usize::MAX),
    })?;
    let ipc_calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id: session_id.clone(),
        trace_id: Some(trace_id.clone()),
        limit: Some(usize::MAX),
    })?;
    let events = store.list_tauri_events(&TauriEventQuery {
        session_id,
        trace_id: Some(trace_id.clone()),
        limit: Some(usize::MAX),
    })?;
    let detail = TraceDetail {
        trace_id,
        spans,
        logs,
        frontend_errors: errors,
        tauri_ipc_calls: ipc_calls,
        tauri_events: events,
    };
    print_json_or_table(json, &detail, || print_trace(&detail))
}

pub fn ipc(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id,
        trace_id,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &calls, || print_ipc(&calls))
}

pub fn events(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let events = store.list_tauri_events(&TauriEventQuery {
        session_id,
        trace_id,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &events, || print_events(&events))
}

pub fn windows(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let windows = store.list_tauri_windows(&TauriWindowQuery {
        session_id,
        latest_only: true,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &windows, || print_windows(&windows))
}

fn open_validated_store(db: &Path) -> Result<SqliteStore> {
    let store = SqliteStore::open(db)?;
    store.validate_schema()?;
    Ok(store)
}

fn print_json_or_table<T: Serialize>(
    json: bool,
    value: &T,
    human: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    } else {
        human()
    }
}

fn print_sessions(sessions: &[Session]) -> Result<()> {
    println!("SESSION\tSERVICE\tSTARTED\tENDED");
    for session in sessions {
        println!(
            "{}\t{}\t{}\t{}",
            table_cell(&session.id, 80),
            table_cell(&session.service_name, 80),
            table_cell(&session.started_at, 40),
            table_cell(session.ended_at.as_deref().unwrap_or("-"), 40)
        );
    }
    Ok(())
}

fn print_apps(apps: &[discovery::DiscoveredApp]) -> Result<()> {
    println!("STATUS\tSERVICE\tSESSION\tPID\tHEARTBEAT_AGE\tDB");
    for app in apps {
        println!(
            "{:?}\t{}\t{}\t{}\t{}\t{}",
            app.status,
            table_cell(&app.service_name, 80),
            table_cell(&app.session_id, 80),
            app.pid,
            app.heartbeat_age_seconds
                .map(|age| age.to_string())
                .unwrap_or_else(|| "-".to_string()),
            table_cell(&app.database_path, 160)
        );
    }
    Ok(())
}

fn print_logs(logs: &[LogRecord]) -> Result<()> {
    println!("TIME\tLEVEL\tSOURCE\tTRACE\tBODY");
    for log in logs {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            log.timestamp_unix_nanos,
            table_cell(log.severity_text.as_deref().unwrap_or("-"), 16),
            log.source.as_str(),
            table_cell(log.trace_id.as_deref().unwrap_or("-"), 80),
            table_cell(log.body.as_deref().unwrap_or(""), 200)
        );
    }
    Ok(())
}

fn print_errors(errors: &[FrontendError]) -> Result<()> {
    println!("TIME\tTRACE\tWINDOW\tMESSAGE");
    for error in errors {
        println!(
            "{}\t{}\t{}\t{}",
            error.timestamp_unix_nanos,
            table_cell(error.trace_id.as_deref().unwrap_or("-"), 80),
            table_cell(error.window_label.as_deref().unwrap_or("-"), 80),
            table_cell(&error.message, 200)
        );
    }
    Ok(())
}

fn print_traces(traces: &[TraceSummary]) -> Result<()> {
    println!("TRACE\tROOT\tDURATION_NS\tSTATUS\tSPANS\tERRORS");
    for trace in traces {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            table_cell(&trace.trace_id, 80),
            table_cell(trace.root_span_name.as_deref().unwrap_or("-"), 120),
            trace.duration_unix_nanos.unwrap_or_default(),
            table_cell(trace.status_code.as_deref().unwrap_or("-"), 20),
            trace.span_count,
            trace.error_count
        );
    }
    Ok(())
}

fn print_trace(trace: &TraceDetail) -> Result<()> {
    println!("Trace {}", trace.trace_id);
    println!("Spans: {}", trace.spans.len());
    for span in &trace.spans {
        println!(
            "span\t{}\t{}\t{}",
            table_cell(&span.span_id, 80),
            table_cell(span.parent_span_id.as_deref().unwrap_or("-"), 80),
            table_cell(&span.name, 160)
        );
    }
    println!("Logs: {}", trace.logs.len());
    println!("Frontend errors: {}", trace.frontend_errors.len());
    println!("Tauri IPC calls: {}", trace.tauri_ipc_calls.len());
    println!("Tauri events: {}", trace.tauri_events.len());
    Ok(())
}

fn print_ipc(calls: &[TauriIpcCall]) -> Result<()> {
    println!("TIME\tSTATUS\tCOMMAND\tTRACE\tERROR");
    for call in calls {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            call.timestamp_unix_nanos,
            table_cell(&call.status, 20),
            table_cell(&call.command, 120),
            table_cell(call.trace_id.as_deref().unwrap_or("-"), 80),
            table_cell(call.error_message.as_deref().unwrap_or("-"), 160)
        );
    }
    Ok(())
}

fn print_events(events: &[TauriEventRecord]) -> Result<()> {
    println!("TIME\tDIRECTION\tEVENT\tTRACE\tTARGET");
    for event in events {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            event.timestamp_unix_nanos,
            table_cell(&event.direction, 20),
            table_cell(&event.event_name, 120),
            table_cell(event.trace_id.as_deref().unwrap_or("-"), 80),
            table_cell(event.target.as_deref().unwrap_or("-"), 80)
        );
    }
    Ok(())
}

fn print_windows(windows: &[TauriWindowState]) -> Result<()> {
    println!("TIME\tWINDOW\tTITLE\tVISIBLE\tFOCUSED\tSIZE");
    for window in windows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}x{}",
            window.timestamp_unix_nanos,
            table_cell(&window.window_label, 80),
            table_cell(window.title.as_deref().unwrap_or("-"), 120),
            window
                .visible
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            window
                .focused
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            window.width.unwrap_or_default(),
            window.height.unwrap_or_default()
        );
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceDetail {
    trace_id: String,
    spans: Vec<SpanRecord>,
    logs: Vec<LogRecord>,
    frontend_errors: Vec<FrontendError>,
    tauri_ipc_calls: Vec<TauriIpcCall>,
    tauri_events: Vec<TauriEventRecord>,
}
