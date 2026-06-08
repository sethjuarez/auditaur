use anyhow::Result;
use auditaur_core::{
    model::{
        FrontendError, LogRecord, SpanRecord, TauriEventRecord, TauriIpcCall, TauriWindowState,
    },
    storage::{RelatedTelemetry, RelatedTelemetryQuery},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use crate::{commands::read, discovery, output::table_cell};

pub fn timeline(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let mut entries = load_timeline(&db, session_id, trace_id, since.as_deref(), limit)?;
    entries.truncate(limit);
    read::print_json_or_table(json, &entries, || print_timeline(&entries))
}

pub fn related(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    window_label: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let related = load_related(
        &db,
        session_id,
        trace_id,
        window_label,
        since.as_deref(),
        limit,
    )?;
    read::print_json_or_table(json, &related, || print_related(&related))
}

pub fn explain(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let entries = load_timeline(&db, session_id, trace_id.clone(), since.as_deref(), limit)?;
    let report = ExplainReport::from_timeline(trace_id, &entries);
    read::print_json_or_table(json, &report, || print_explain(&report))
}

pub fn bundle(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    _redacted: bool,
    output: Option<PathBuf>,
    limit: usize,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let store = read::open_validated_store(&db)?;
    let sessions = store.list_sessions(Some(limit))?;
    let related = related_from_store(&store, session_id, trace_id, None, since.as_deref(), limit)?;
    let mut bundle = json!({
        "schemaVersion": 1,
        "generatedAtUnixNanos": read::current_time_unix_nanos(),
        "databasePath": db,
        "redacted": true,
        "sessions": sessions,
        "logs": related.logs,
        "spans": related.spans,
        "frontendErrors": related.frontend_errors,
        "tauriIpcCalls": related.tauri_ipc_calls,
        "tauriEvents": related.tauri_events,
        "tauriWindows": related.tauri_windows,
    });
    redact_value(&mut bundle);
    let serialized = serde_json::to_string_pretty(&bundle)?;
    if let Some(output) = output {
        fs::write(output, serialized)?;
    } else {
        println!("{serialized}");
    }
    Ok(())
}

pub fn tail(
    db: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    replay: bool,
    interval_ms: u64,
    duration_seconds: Option<u64>,
    json: bool,
) -> Result<()> {
    let db = discovery::resolve_db(db.clone())?;
    let started = Instant::now();
    let mut last_seen = if replay {
        0
    } else {
        read::current_time_unix_nanos()
    };
    loop {
        let mut entries =
            load_timeline(&db, session_id.clone(), trace_id.clone(), None, usize::MAX)?;
        entries.retain(|entry| entry.timestamp_unix_nanos > last_seen);
        entries.sort_by_key(|entry| entry.timestamp_unix_nanos);
        for entry in &entries {
            last_seen = last_seen.max(entry.timestamp_unix_nanos);
            if json {
                println!("{}", serde_json::to_string(entry)?);
            } else {
                print_timeline_entry(entry);
            }
        }
        if duration_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            break;
        }
        thread::sleep(Duration::from_millis(interval_ms.max(100)));
    }
    Ok(())
}

fn load_timeline(
    db: &PathBuf,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<TimelineEntry>> {
    let related = load_related(db, session_id, trace_id, None, since, limit)?;
    Ok(timeline_entries(related, limit))
}

fn load_related(
    db: &PathBuf,
    session_id: Option<String>,
    trace_id: Option<String>,
    window_label: Option<String>,
    since: Option<&str>,
    limit: usize,
) -> Result<RelatedTelemetry> {
    let store = read::open_validated_store(db)?;
    related_from_store(&store, session_id, trace_id, window_label, since, limit)
}

pub(crate) fn related_from_store(
    store: &auditaur_collector::exporter_sqlite::SqliteStore,
    session_id: Option<String>,
    trace_id: Option<String>,
    window_label: Option<String>,
    since: Option<&str>,
    limit: usize,
) -> Result<RelatedTelemetry> {
    let start_time_unix_nanos = read::parse_since_cutoff(since)?;
    Ok(store.related_telemetry(&RelatedTelemetryQuery {
        session_id,
        trace_id,
        window_label,
        start_time_unix_nanos,
        end_time_unix_nanos: None,
        limit: Some(limit),
    })?)
}

fn timeline_entries(related: RelatedTelemetry, limit: usize) -> Vec<TimelineEntry> {
    let mut entries = Vec::new();
    entries.extend(related.logs.into_iter().map(TimelineEntry::from_log));
    entries.extend(related.spans.into_iter().map(TimelineEntry::from_span));
    entries.extend(
        related
            .frontend_errors
            .into_iter()
            .map(TimelineEntry::from_error),
    );
    entries.extend(
        related
            .tauri_ipc_calls
            .into_iter()
            .map(TimelineEntry::from_ipc),
    );
    entries.extend(
        related
            .tauri_events
            .into_iter()
            .map(TimelineEntry::from_event),
    );
    entries.extend(
        related
            .tauri_windows
            .into_iter()
            .map(TimelineEntry::from_window),
    );
    entries.sort_by_key(|entry| entry.timestamp_unix_nanos);
    if entries.len() > limit {
        entries = entries.split_off(entries.len() - limit);
    }
    entries
}

fn print_timeline(entries: &[TimelineEntry]) -> Result<()> {
    println!("TIME\tTYPE\tSTATUS\tTRACE\tSUMMARY");
    for entry in entries {
        print_timeline_entry(entry);
    }
    Ok(())
}

fn print_timeline_entry(entry: &TimelineEntry) {
    println!(
        "{}\t{}\t{}\t{}\t{}",
        entry.timestamp_unix_nanos,
        table_cell(&entry.kind, 20),
        table_cell(entry.status.as_deref().unwrap_or("-"), 20),
        table_cell(entry.trace_id.as_deref().unwrap_or("-"), 80),
        table_cell(&entry.summary, 220)
    );
}

fn print_explain(report: &ExplainReport) -> Result<()> {
    println!("Auditaur explain");
    println!("Total events: {}", report.total_events);
    println!("Errors: {}", report.error_count);
    println!("Failed IPC calls: {}", report.failed_ipc_count);
    println!("Failed spans: {}", report.failed_span_count);
    if report.findings.is_empty() {
        println!("No obvious failures found in the selected telemetry.");
    } else {
        println!("Findings:");
        for finding in &report.findings {
            println!("- {}", table_cell(finding, 240));
        }
    }
    Ok(())
}

fn print_related(related: &RelatedTelemetry) -> Result<()> {
    println!("TYPE\tCOUNT");
    println!("spans\t{}", related.spans.len());
    println!("logs\t{}", related.logs.len());
    println!("frontend_errors\t{}", related.frontend_errors.len());
    println!("tauri_ipc_calls\t{}", related.tauri_ipc_calls.len());
    println!("tauri_events\t{}", related.tauri_events.len());
    println!("tauri_windows\t{}", related.tauri_windows.len());
    Ok(())
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let key_lower = key.to_ascii_lowercase();
                if key_lower.contains("secret")
                    || key_lower.contains("password")
                    || key_lower.contains("token")
                    || key_lower.contains("authorization")
                    || key_lower.contains("cookie")
                    || key_lower.ends_with("key")
                    || key_lower.ends_with("json")
                    || key_lower == "attributes"
                {
                    *value = Value::String("[redacted]".to_string());
                } else {
                    redact_value(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimelineEntry {
    timestamp_unix_nanos: i64,
    kind: String,
    session_id: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    status: Option<String>,
    summary: String,
}

impl TimelineEntry {
    fn from_log(log: LogRecord) -> Self {
        Self {
            timestamp_unix_nanos: log.timestamp_unix_nanos,
            kind: "log".to_string(),
            session_id: log.session_id,
            trace_id: log.trace_id,
            span_id: log.span_id,
            status: log.severity_text,
            summary: log.body.unwrap_or_default(),
        }
    }

    fn from_span(span: SpanRecord) -> Self {
        Self {
            timestamp_unix_nanos: span.start_time_unix_nanos,
            kind: "span".to_string(),
            session_id: span.session_id,
            trace_id: Some(span.trace_id),
            span_id: Some(span.span_id),
            status: span.status_code,
            summary: span.name,
        }
    }

    fn from_error(error: FrontendError) -> Self {
        Self {
            timestamp_unix_nanos: error.timestamp_unix_nanos,
            kind: "frontend_error".to_string(),
            session_id: error.session_id,
            trace_id: error.trace_id,
            span_id: error.span_id,
            status: error.error_type,
            summary: error.message,
        }
    }

    fn from_ipc(call: TauriIpcCall) -> Self {
        let summary = match call.error_message {
            Some(error) => format!("{}: {error}", call.command),
            None => call.command,
        };
        Self {
            timestamp_unix_nanos: call.timestamp_unix_nanos,
            kind: "ipc".to_string(),
            session_id: call.session_id,
            trace_id: call.trace_id,
            span_id: call.span_id,
            status: Some(call.status),
            summary,
        }
    }

    fn from_event(event: TauriEventRecord) -> Self {
        Self {
            timestamp_unix_nanos: event.timestamp_unix_nanos,
            kind: "event".to_string(),
            session_id: event.session_id,
            trace_id: event.trace_id,
            span_id: event.span_id,
            status: Some(event.direction),
            summary: event.event_name,
        }
    }

    fn from_window(window: TauriWindowState) -> Self {
        let event = window
            .attributes
            .get("tauri.window.event")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        Self {
            timestamp_unix_nanos: window.timestamp_unix_nanos,
            kind: "window".to_string(),
            session_id: window.session_id,
            trace_id: None,
            span_id: None,
            status: event.or_else(|| {
                window.focused.map(|focused| {
                    if focused {
                        "focused".to_string()
                    } else {
                        "unfocused".to_string()
                    }
                })
            }),
            summary: window.window_label,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExplainReport {
    trace_id: Option<String>,
    total_events: usize,
    error_count: usize,
    failed_ipc_count: usize,
    failed_span_count: usize,
    findings: Vec<String>,
}

impl ExplainReport {
    fn from_timeline(trace_id: Option<String>, entries: &[TimelineEntry]) -> Self {
        let mut findings = Vec::new();
        let mut error_count = 0;
        let mut failed_ipc_count = 0;
        let mut failed_span_count = 0;
        for entry in entries {
            match entry.kind.as_str() {
                "frontend_error" => {
                    error_count += 1;
                    findings.push(format!("Frontend error: {}", entry.summary));
                }
                "ipc"
                    if entry
                        .status
                        .as_deref()
                        .is_some_and(read::is_failed_ipc_status) =>
                {
                    failed_ipc_count += 1;
                    findings.push(format!("Failed IPC call: {}", entry.summary));
                }
                "span"
                    if entry
                        .status
                        .as_deref()
                        .is_some_and(|status| !status.eq_ignore_ascii_case("OK")) =>
                {
                    failed_span_count += 1;
                    findings.push(format!("Failed span: {}", entry.summary));
                }
                "log" if entry.status.as_deref().is_some_and(is_error_level) => {
                    error_count += 1;
                    findings.push(format!("Error log: {}", entry.summary));
                }
                _ => {}
            }
        }
        findings.truncate(20);
        Self {
            trace_id,
            total_events: entries.len(),
            error_count,
            failed_ipc_count,
            failed_span_count,
            findings,
        }
    }
}

fn is_error_level(level: &str) -> bool {
    matches!(
        level.to_ascii_uppercase().as_str(),
        "ERROR" | "FATAL" | "CRITICAL"
    )
}
