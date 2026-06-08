use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::{
    model::{
        FrontendError, LogRecord, Session, SpanRecord, TauriEventRecord, TauriIpcCall,
        TauriWindowState,
    },
    protocol::TraceSummary,
    storage::{
        FrontendErrorQuery, LogQuery, RelatedTelemetryQuery, TauriEventQuery, TauriIpcQuery,
        TauriWindowQuery,
    },
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

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
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let mut logs = store.list_logs(&LogQuery {
        session_id,
        trace_id,
        limit: Some(fetch_limit(since.as_ref(), limit)),
    })?;
    filter_since(&mut logs, since.as_deref(), |log| log.timestamp_unix_nanos)?;
    logs.truncate(limit);
    print_json_or_table(json, &logs, || print_logs(&logs))
}

pub fn errors(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let mut errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id,
        trace_id,
        limit: Some(fetch_limit(since.as_ref(), limit)),
    })?;
    filter_since(&mut errors, since.as_deref(), |error| {
        error.timestamp_unix_nanos
    })?;
    errors.truncate(limit);
    print_json_or_table(json, &errors, || print_errors(&errors))
}

pub fn traces(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    since: Option<String>,
    failed: bool,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let mut summaries = store.list_trace_summaries(
        session_id.as_deref(),
        Some(if since.is_some() || failed {
            usize::MAX
        } else {
            limit
        }),
    )?;
    filter_since(&mut summaries, since.as_deref(), |trace| {
        trace.start_time_unix_nanos.unwrap_or_default()
    })?;
    if failed {
        summaries.retain(is_failed_trace);
    }
    summaries.truncate(limit);
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
    let related = store.related_telemetry(&RelatedTelemetryQuery {
        session_id,
        trace_id: Some(trace_id.clone()),
        window_label: None,
        start_time_unix_nanos: None,
        end_time_unix_nanos: None,
        limit: Some(usize::MAX),
    })?;
    let detail = TraceDetail {
        trace_id,
        spans: related.spans,
        logs: related.logs,
        frontend_errors: related.frontend_errors,
        tauri_ipc_calls: related.tauri_ipc_calls,
        tauri_events: related.tauri_events,
        tauri_windows: related.tauri_windows,
    };
    print_json_or_table(json, &detail, || print_trace(&detail))
}

pub fn ipc(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    failed: bool,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let mut calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id,
        trace_id,
        limit: Some(if since.is_some() || failed {
            usize::MAX
        } else {
            limit
        }),
    })?;
    filter_since(&mut calls, since.as_deref(), |call| {
        call.timestamp_unix_nanos
    })?;
    if failed {
        calls.retain(is_failed_ipc);
    }
    calls.truncate(limit);
    print_json_or_table(json, &calls, || print_ipc(&calls))
}

pub fn events(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = open_validated_store(&db)?;
    let mut events = store.list_tauri_events(&TauriEventQuery {
        session_id,
        trace_id,
        limit: Some(fetch_limit(since.as_ref(), limit)),
    })?;
    filter_since(&mut events, since.as_deref(), |event| {
        event.timestamp_unix_nanos
    })?;
    events.truncate(limit);
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

pub(crate) fn open_validated_store(db: &Path) -> Result<SqliteStore> {
    let store = SqliteStore::open(db)?;
    store.migrate()?;
    store.validate_schema()?;
    Ok(store)
}

pub(crate) fn print_json_or_table<T: Serialize>(
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

pub(crate) fn parse_since_cutoff(value: Option<&str>) -> Result<Option<i64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let duration_nanos = parse_duration_nanos(value)?;
    Ok(Some(
        current_time_unix_nanos().saturating_sub(duration_nanos),
    ))
}

pub(crate) fn current_time_unix_nanos() -> i64 {
    let now = OffsetDateTime::now_utc();
    now.unix_timestamp()
        .saturating_mul(1_000_000_000)
        .saturating_add(i64::from(now.nanosecond()))
}

pub(crate) fn is_failed_trace(trace: &TraceSummary) -> bool {
    trace.error_count > 0
        || trace
            .status_code
            .as_deref()
            .is_some_and(|status| !status.eq_ignore_ascii_case("OK"))
}

pub(crate) fn is_failed_ipc(call: &TauriIpcCall) -> bool {
    is_failed_ipc_status(&call.status)
}

pub(crate) fn is_failed_ipc_status(status: &str) -> bool {
    !matches!(
        status,
        "OK" | "Ok" | "ok" | "SUCCESS" | "Success" | "success"
    )
}

fn fetch_limit(since: Option<&String>, limit: usize) -> usize {
    if since.is_some() {
        usize::MAX
    } else {
        limit
    }
}

fn filter_since<T>(
    items: &mut Vec<T>,
    since: Option<&str>,
    timestamp: impl Fn(&T) -> i64,
) -> Result<()> {
    if let Some(cutoff) = parse_since_cutoff(since)? {
        items.retain(|item| timestamp(item) >= cutoff);
    }
    Ok(())
}

fn parse_duration_nanos(value: &str) -> Result<i64> {
    let mut chars = value.chars();
    let unit = chars.next_back().unwrap_or_default();
    let number = chars.as_str();
    let amount: i64 = number.parse().map_err(|_| {
        anyhow::anyhow!("Invalid duration `{value}`. Use values like 30s, 10m, 2h, or 1d.")
    })?;
    let seconds = match unit {
        's' => amount,
        'm' => amount.saturating_mul(60),
        'h' => amount.saturating_mul(60 * 60),
        'd' => amount.saturating_mul(60 * 60 * 24),
        _ => {
            return Err(anyhow::anyhow!(
                "Invalid duration unit `{unit}`. Use s, m, h, or d."
            ))
        }
    };
    Ok(seconds.saturating_mul(1_000_000_000))
}

#[cfg(test)]
mod tests {
    use super::parse_duration_nanos;

    #[test]
    fn duration_parser_rejects_invalid_unicode_unit_without_panicking() {
        let error = parse_duration_nanos("5µ").unwrap_err().to_string();
        assert!(error.contains("Invalid duration unit"));
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
    println!("STATUS\tSERVICE\tSESSION\tPID\tSTARTED\tHEARTBEAT_AGE\tCHURN\tDB");
    for app in apps {
        println!(
            "{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            app.status,
            table_cell(&app.service_name, 80),
            table_cell(&app.session_id, 80),
            app.pid,
            table_cell(&app.started_at, 40),
            app.heartbeat_age_seconds
                .map(|age| age.to_string())
                .unwrap_or_else(|| "-".to_string()),
            table_cell(app.churn_hint.as_deref().unwrap_or("-"), 120),
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
    println!("TIME\tWINDOW\tEVENT\tTITLE\tVISIBLE\tFOCUSED\tSIZE");
    for window in windows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}x{}",
            window.timestamp_unix_nanos,
            table_cell(&window.window_label, 80),
            table_cell(window_event(&window), 40),
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

fn window_event(window: &TauriWindowState) -> &str {
    window
        .attributes
        .get("tauri.window.event")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("-")
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
    tauri_windows: Vec<TauriWindowState>,
}
