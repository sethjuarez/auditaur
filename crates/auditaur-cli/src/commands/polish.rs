use anyhow::{anyhow, Context, Result};
use auditaur_core::{
    model::{
        FrontendError, LogRecord, SpanRecord, TauriEventRecord, TauriIpcCall, TauriWindowState,
        TelemetrySource,
    },
    storage::{
        FrontendErrorQuery, RelatedTelemetry, RelatedTelemetryQuery, TauriEventQuery, TauriIpcQuery,
    },
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{commands::read, discovery, output::table_cell};

pub fn timeline(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    anchor: Option<String>,
    window: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let focus = load_focused_related(
        &db,
        session_id,
        trace_id,
        None,
        anchor,
        window.as_deref(),
        since.as_deref(),
        limit,
    )?;
    let mut entries = timeline_entries(focus.related, limit);
    entries.truncate(limit);
    if let Some(anchor) = focus.anchor {
        let report = TimelineReport { anchor, entries };
        read::print_json_or_table(json, &report, || {
            print_anchor(&report.anchor);
            print_timeline(&report.entries)
        })
    } else {
        read::print_json_or_table(json, &entries, || print_timeline(&entries))
    }
}

pub fn related(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    run_id: Option<String>,
    window_label: Option<String>,
    anchor: Option<String>,
    anchor_window: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let trace_id = resolve_related_trace(&db, session_id.as_deref(), trace_id, run_id)?;
    let focus = load_focused_related(
        &db,
        session_id,
        trace_id,
        window_label,
        anchor,
        anchor_window.as_deref(),
        since.as_deref(),
        limit,
    )?;
    if let Some(anchor) = focus.anchor {
        let report = RelatedReport {
            anchor,
            related: focus.related,
        };
        read::print_json_or_table(json, &report, || {
            print_anchor(&report.anchor);
            print_related(&report.related)
        })
    } else {
        read::print_json_or_table(json, &focus.related, || print_related(&focus.related))
    }
}

fn resolve_related_trace(
    db: &PathBuf,
    session_id: Option<&str>,
    trace_id: Option<String>,
    run_id: Option<String>,
) -> Result<Option<String>> {
    if trace_id.is_some() || run_id.is_none() {
        return Ok(trace_id);
    }
    let run_id = run_id.expect("checked above");
    let store = read::open_validated_store(db)?;
    crate::commands::agent::find_run_trace_id(&store, session_id, &run_id)?
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("No agent run `{run_id}` found. Try `auditaur agent-runs`."))
}

pub fn explain(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    anchor: Option<String>,
    window: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let focus = load_focused_related(
        &db,
        session_id,
        trace_id.clone(),
        None,
        anchor,
        window.as_deref(),
        since.as_deref(),
        limit,
    )?;
    let entries = timeline_entries(focus.related.clone(), limit);
    let report = ExplainReport::from_related(trace_id, focus.anchor, &focus.related, &entries);
    read::print_json_or_table(json, &report, || print_explain(&report))
}

pub(crate) fn explain_json_value(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    anchor: Option<String>,
    window: Option<String>,
    since: Option<String>,
    limit: usize,
) -> Result<Value> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let focus = load_focused_related(
        &db,
        session_id,
        trace_id.clone(),
        None,
        anchor,
        window.as_deref(),
        since.as_deref(),
        limit,
    )?;
    let entries = timeline_entries(focus.related.clone(), limit);
    let report = ExplainReport::from_related(trace_id, focus.anchor, &focus.related, &entries);
    serde_json::to_value(report).map_err(Into::into)
}

pub fn bundle(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    since: Option<String>,
    _redacted: bool,
    output: Option<PathBuf>,
    limit: usize,
) -> Result<()> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let store = read::open_validated_store(&db)?;
    let sessions = store.list_sessions(Some(limit))?;
    let related = related_from_store(
        &store,
        session_id,
        trace_id,
        None,
        since.as_deref(),
        None,
        limit,
    )?;
    let mut bundle = json!({
        "schemaVersion": 1,
        "generatedAtUnixNanos": read::current_time_unix_nanos(),
        "databasePath": db,
        "redacted": true,
        "sessions": sessions,
        "logs": related.logs,
        "spans": related.spans,
        "spanEvents": related.span_events,
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
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    signal: Option<String>,
    replay: bool,
    interval_ms: u64,
    duration_seconds: Option<u64>,
    json: bool,
) -> Result<()> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let started = Instant::now();
    let mut last_seen = if replay {
        0
    } else {
        read::current_time_unix_nanos()
    };
    loop {
        let mut entries =
            load_timeline(&db, session_id.clone(), trace_id.clone(), None, usize::MAX)?;
        if let Some(signal) = signal.as_deref() {
            apply_signal_filter(&mut entries, signal)?;
        }
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

pub fn diagnose(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    session_id: Option<String>,
    trace_id: Option<String>,
    anchor: Option<String>,
    window: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    let db = discovery::resolve_db(db)?;
    let focus = load_focused_related(
        &db,
        session_id.clone(),
        trace_id.clone(),
        None,
        anchor,
        window.as_deref(),
        since.as_deref(),
        limit,
    )?;
    let entries = timeline_entries(focus.related.clone(), limit);
    let explain = ExplainReport::from_related(trace_id, focus.anchor, &focus.related, &entries);
    let failure_entries = entries
        .iter()
        .filter(|entry| is_failure_timeline_entry(entry))
        .take(20)
        .cloned()
        .collect::<Vec<_>>();
    let suggestions = suggested_commands(
        session_file,
        Some(&db),
        session_id.as_deref(),
        explain.anchor.as_ref(),
    );
    let report = DiagnoseReport {
        total_events: explain.total_events,
        finding_count: explain.findings.len(),
        error_count: explain.error_count,
        failed_ipc_count: explain.failed_ipc_count,
        failed_span_count: explain.failed_span_count,
        anchor: explain.anchor,
        findings: explain.findings,
        failure_entries,
        suggested_commands: suggestions,
    };
    read::print_json_or_table(json, &report, || print_diagnose(&report))
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

fn load_focused_related(
    db: &PathBuf,
    session_id: Option<String>,
    trace_id: Option<String>,
    window_label: Option<String>,
    anchor: Option<String>,
    anchor_window: Option<&str>,
    since: Option<&str>,
    limit: usize,
) -> Result<FocusedRelated> {
    let store = read::open_validated_store(db)?;
    let mut trace_id = trace_id;
    let mut start_time_unix_nanos = read::parse_since_cutoff(since)?;
    let mut end_time_unix_nanos = None;
    let mut anchor_metadata = None;

    if let Some(anchor) = anchor {
        let resolved = resolve_anchor(&store, session_id.as_deref(), &anchor, anchor_window)?;
        if trace_id.is_none() && resolved.start_time_unix_nanos.is_none() {
            trace_id = resolved.trace_id.clone();
        }
        if let Some(start) = resolved.start_time_unix_nanos {
            start_time_unix_nanos =
                Some(start_time_unix_nanos.map_or(start, |since| since.max(start)));
        }
        end_time_unix_nanos = resolved.end_time_unix_nanos;
        anchor_metadata = Some(resolved.metadata);
    }

    let related = related_from_store_with_bounds(
        &store,
        session_id,
        trace_id,
        window_label,
        start_time_unix_nanos,
        end_time_unix_nanos,
        limit,
    )?;
    Ok(FocusedRelated {
        anchor: anchor_metadata,
        related,
    })
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
    related_from_store(
        &store,
        session_id,
        trace_id,
        window_label,
        since,
        None,
        limit,
    )
}

pub(crate) fn related_from_store(
    store: &auditaur_collector::exporter_sqlite::SqliteStore,
    session_id: Option<String>,
    trace_id: Option<String>,
    window_label: Option<String>,
    since: Option<&str>,
    end_time_unix_nanos: Option<i64>,
    limit: usize,
) -> Result<RelatedTelemetry> {
    let start_time_unix_nanos = read::parse_since_cutoff(since)?;
    related_from_store_with_bounds(
        store,
        session_id,
        trace_id,
        window_label,
        start_time_unix_nanos,
        end_time_unix_nanos,
        limit,
    )
}

fn related_from_store_with_bounds(
    store: &auditaur_collector::exporter_sqlite::SqliteStore,
    session_id: Option<String>,
    trace_id: Option<String>,
    window_label: Option<String>,
    start_time_unix_nanos: Option<i64>,
    end_time_unix_nanos: Option<i64>,
    limit: usize,
) -> Result<RelatedTelemetry> {
    Ok(store.related_telemetry(&RelatedTelemetryQuery {
        session_id,
        trace_id,
        window_label,
        start_time_unix_nanos,
        end_time_unix_nanos,
        limit: Some(limit),
    })?)
}

fn timeline_entries(related: RelatedTelemetry, limit: usize) -> Vec<TimelineEntry> {
    let mut entries = Vec::new();
    entries.extend(related.logs.into_iter().map(TimelineEntry::from_log));
    entries.extend(related.spans.into_iter().map(TimelineEntry::from_span));
    entries.extend(
        related
            .span_events
            .into_iter()
            .map(TimelineEntry::from_span_event),
    );
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

fn resolve_anchor(
    store: &auditaur_collector::exporter_sqlite::SqliteStore,
    session_id: Option<&str>,
    raw: &str,
    window: Option<&str>,
) -> Result<ResolvedAnchor> {
    let (kind, value) = raw
        .split_once(':')
        .ok_or_else(|| anyhow!("Invalid anchor `{raw}`. Use <kind>:<value>."))?;
    let window_unix_nanos = parse_anchor_window(window)?;
    let window_label = format_duration(window_unix_nanos);
    let session = session_id.map(str::to_string);
    let mut metadata = AnchorMetadata {
        raw: raw.to_string(),
        kind: kind.to_string(),
        value: value.to_string(),
        timestamp_unix_nanos: None,
        trace_id: None,
        window_unix_nanos: Some(window_unix_nanos),
        window: Some(window_label),
    };

    match kind {
            "trace" => {
                metadata.trace_id = Some(value.to_string());
                Ok(ResolvedAnchor {
                    metadata,
                    trace_id: Some(value.to_string()),
                    start_time_unix_nanos: None,
                    end_time_unix_nanos: None,
                })
            }
            "ipc" => {
                let calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
                    session_id: session,
                    trace_id: None,
                    limit: Some(usize::MAX),
                })?;
                let call = calls
                    .iter()
                    .filter(|call| value == "latest" || call.command == value)
                    .max_by_key(|call| call.timestamp_unix_nanos)
                    .ok_or_else(|| anyhow!("No IPC anchor `{raw}` found in selected telemetry."))?;
                metadata.timestamp_unix_nanos = Some(call.timestamp_unix_nanos);
                metadata.trace_id = call.trace_id.clone();
                Ok(anchor_window_result(
                    metadata,
                    call.trace_id.clone(),
                    call.timestamp_unix_nanos,
                    window_unix_nanos,
                ))
            }
            "error" => {
                let errors = store.list_frontend_errors(&FrontendErrorQuery {
                    session_id: session,
                    trace_id: None,
                    limit: Some(usize::MAX),
                })?;
                let error = errors
                    .iter()
                    .filter(|error| value == "latest" || error.message.contains(value))
                    .max_by_key(|error| error.timestamp_unix_nanos)
                    .ok_or_else(|| anyhow!("No error anchor `{raw}` found in selected telemetry."))?;
                metadata.timestamp_unix_nanos = Some(error.timestamp_unix_nanos);
                metadata.trace_id = error.trace_id.clone();
                Ok(anchor_window_result(
                    metadata,
                    error.trace_id.clone(),
                    error.timestamp_unix_nanos,
                    window_unix_nanos,
                ))
            }
            "event" | "checkpoint" => {
                let events = store.list_tauri_events(&TauriEventQuery {
                    session_id: session,
                    trace_id: None,
                    limit: Some(usize::MAX),
                })?;
                let event = events
                    .iter()
                    .filter(|event| value == "latest" || event.event_name == value)
                    .max_by_key(|event| event.timestamp_unix_nanos)
                    .ok_or_else(|| anyhow!("No {kind} anchor `{raw}` found in selected telemetry."))?;
                metadata.timestamp_unix_nanos = Some(event.timestamp_unix_nanos);
                metadata.trace_id = event.trace_id.clone();
                Ok(anchor_window_result(
                    metadata,
                    event.trace_id.clone(),
                    event.timestamp_unix_nanos,
                    window_unix_nanos,
                ))
            }
            "time" => {
                let timestamp = parse_anchor_time(value)?;
                metadata.timestamp_unix_nanos = Some(timestamp);
                Ok(anchor_window_result(metadata, None, timestamp, window_unix_nanos))
            }
            _ => Err(anyhow!(
                "Unsupported anchor kind `{kind}`. Supported kinds: trace, ipc, error, event, checkpoint, time."
            )),
        }
}

fn anchor_window_result(
    metadata: AnchorMetadata,
    trace_id: Option<String>,
    timestamp_unix_nanos: i64,
    window_unix_nanos: i64,
) -> ResolvedAnchor {
    let half_window = window_unix_nanos / 2;
    ResolvedAnchor {
        metadata,
        trace_id,
        start_time_unix_nanos: Some(timestamp_unix_nanos.saturating_sub(half_window)),
        end_time_unix_nanos: Some(timestamp_unix_nanos.saturating_add(half_window)),
    }
}

fn parse_anchor_window(value: Option<&str>) -> Result<i64> {
    value
        .map(read::parse_duration_nanos)
        .unwrap_or_else(|| read::parse_duration_nanos("10s"))
}

fn parse_anchor_time(value: &str) -> Result<i64> {
    if let Ok(timestamp) = value.parse::<i64>() {
        return Ok(timestamp);
    }
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .with_context(|| format!("Invalid time anchor `{value}`. Use RFC3339 or unix nanos."))?;
    Ok(parsed
        .unix_timestamp()
        .saturating_mul(1_000_000_000)
        .saturating_add(i64::from(parsed.nanosecond())))
}

fn format_duration(nanos: i64) -> String {
    if nanos % 3_600_000_000_000 == 0 {
        format!("{}h", nanos / 3_600_000_000_000)
    } else if nanos % 60_000_000_000 == 0 {
        format!("{}m", nanos / 60_000_000_000)
    } else if nanos % 1_000_000_000 == 0 {
        format!("{}s", nanos / 1_000_000_000)
    } else {
        format!("{nanos}ns")
    }
}

fn print_anchor(anchor: &AnchorMetadata) {
    println!(
        "Anchor: {} window={}",
        anchor.raw,
        anchor.window.as_deref().unwrap_or("-")
    );
}

fn apply_signal_filter(entries: &mut Vec<TimelineEntry>, signal: &str) -> Result<()> {
    match signal {
        "failures" => {
            entries.retain(is_failure_timeline_entry);
            Ok(())
        }
        _ => Err(anyhow!(
            "Unsupported signal `{signal}`. Supported signals: failures."
        )),
    }
}

fn is_failure_timeline_entry(entry: &TimelineEntry) -> bool {
    match entry.kind.as_str() {
        "frontend_error" => true,
        "ipc" => entry
            .status
            .as_deref()
            .is_some_and(read::is_failed_ipc_status),
        "span" => entry
            .status
            .as_deref()
            .is_some_and(|status| !status.eq_ignore_ascii_case("OK")),
        "log" => entry.status.as_deref().is_some_and(is_error_level),
        _ => false,
    }
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
    if let Some(anchor) = &report.anchor {
        print_anchor(anchor);
    }
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

fn print_diagnose(report: &DiagnoseReport) -> Result<()> {
    println!("Auditaur diagnose");
    if let Some(anchor) = &report.anchor {
        print_anchor(anchor);
    }
    println!("Total events: {}", report.total_events);
    println!("Findings: {}", report.finding_count);
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
    if !report.failure_entries.is_empty() {
        println!("Failure signals:");
        for entry in &report.failure_entries {
            print_timeline_entry(entry);
        }
    }
    if !report.suggested_commands.is_empty() {
        println!("Suggested next commands:");
        for command in &report.suggested_commands {
            println!("- {command}");
        }
    }
    Ok(())
}

fn print_related(related: &RelatedTelemetry) -> Result<()> {
    println!("TYPE\tCOUNT");
    println!("spans\t{}", related.spans.len());
    println!("span_events\t{}", related.span_events.len());
    println!("logs\t{}", related.logs.len());
    println!("frontend_errors\t{}", related.frontend_errors.len());
    println!("tauri_ipc_calls\t{}", related.tauri_ipc_calls.len());
    println!("tauri_events\t{}", related.tauri_events.len());
    println!("tauri_windows\t{}", related.tauri_windows.len());
    Ok(())
}

fn suggested_commands(
    session_file: &Option<PathBuf>,
    db: Option<&PathBuf>,
    session_id: Option<&str>,
    anchor: Option<&AnchorMetadata>,
) -> Vec<String> {
    let selector = if let Some(session_file) = session_file {
        format!(
            "--session-file {}",
            quote_arg(&session_file.display().to_string())
        )
    } else if let (Some(db), Some(session_id)) = (db, session_id) {
        format!(
            "--db {} --session {session_id}",
            quote_arg(&db.display().to_string())
        )
    } else if let Some(db) = db {
        format!("--db {}", quote_arg(&db.display().to_string()))
    } else if let Some(session_id) = session_id {
        format!("--session {session_id}")
    } else {
        String::new()
    };
    let mut commands = vec![
        command_with_selector("auditaur diagnose", &selector),
        command_with_selector(
            "auditaur tail",
            &format!("{selector} --signal failures --replay --duration-seconds 30"),
        ),
    ];
    if let Some(anchor) = anchor {
        commands.push(command_with_selector(
            "auditaur related",
            &format!(
                "{selector} --anchor {} --anchor-window {}",
                anchor.raw,
                anchor.window.as_deref().unwrap_or("10s")
            ),
        ));
    }
    commands
}

fn command_with_selector(command: &str, args: &str) -> String {
    let args = args.trim();
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{command} {args}")
    }
}

fn quote_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
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
struct FocusedRelated {
    anchor: Option<AnchorMetadata>,
    related: RelatedTelemetry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimelineReport {
    anchor: AnchorMetadata,
    entries: Vec<TimelineEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelatedReport {
    anchor: AnchorMetadata,
    related: RelatedTelemetry,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnchorMetadata {
    raw: String,
    kind: String,
    value: String,
    timestamp_unix_nanos: Option<i64>,
    trace_id: Option<String>,
    window_unix_nanos: Option<i64>,
    window: Option<String>,
}

#[derive(Debug)]
struct ResolvedAnchor {
    metadata: AnchorMetadata,
    trace_id: Option<String>,
    start_time_unix_nanos: Option<i64>,
    end_time_unix_nanos: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnoseReport {
    total_events: usize,
    finding_count: usize,
    error_count: usize,
    failed_ipc_count: usize,
    failed_span_count: usize,
    anchor: Option<AnchorMetadata>,
    findings: Vec<String>,
    failure_entries: Vec<TimelineEntry>,
    suggested_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
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

    fn from_span_event(event: auditaur_core::model::SpanEventRecord) -> Self {
        Self {
            timestamp_unix_nanos: event.timestamp_unix_nanos,
            kind: "span_event".to_string(),
            session_id: event.session_id,
            trace_id: Some(event.trace_id),
            span_id: Some(event.span_id),
            status: None,
            summary: event.name,
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
    anchor: Option<AnchorMetadata>,
    total_events: usize,
    error_count: usize,
    failed_ipc_count: usize,
    failed_span_count: usize,
    findings: Vec<String>,
}

impl ExplainReport {
    fn from_related(
        trace_id: Option<String>,
        anchor: Option<AnchorMetadata>,
        related: &RelatedTelemetry,
        entries: &[TimelineEntry],
    ) -> Self {
        let mut report = Self::from_timeline(trace_id, anchor, entries);
        let mut continuation_findings = Self::missing_backend_continuation_findings(related);
        continuation_findings.extend(report.findings);
        report.findings = continuation_findings;
        report.findings.truncate(20);
        report
    }

    fn from_timeline(
        trace_id: Option<String>,
        anchor: Option<AnchorMetadata>,
        entries: &[TimelineEntry],
    ) -> Self {
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
        Self {
            trace_id,
            anchor,
            total_events: entries.len(),
            error_count,
            failed_ipc_count,
            failed_span_count,
            findings,
        }
    }

    fn missing_backend_continuation_findings(related: &RelatedTelemetry) -> Vec<String> {
        let mut findings = Vec::new();
        let mut reported = HashSet::new();
        for call in &related.tauri_ipc_calls {
            if call.command.contains(':') || call.command.contains('|') {
                continue;
            }
            let Some(trace_id) = call.trace_id.as_deref() else {
                continue;
            };
            let Some(frontend_span_id) = call.span_id.as_deref() else {
                continue;
            };
            let has_frontend_invoke_span = related.spans.iter().any(|span| {
                span.trace_id == trace_id
                    && span.span_id == frontend_span_id
                    && span.name == format!("tauri.invoke {}", call.command)
            });
            if !has_frontend_invoke_span {
                continue;
            }
            let has_backend_child_span = related.spans.iter().any(|span| {
                span.trace_id == trace_id
                    && span.source == TelemetrySource::Backend
                    && span.parent_span_id.as_deref() == Some(frontend_span_id)
            });
            if has_backend_child_span {
                continue;
            }
            if reported.insert((trace_id.to_string(), call.command.clone())) {
                findings.push(format!(
                    "Missing backend trace continuation for tauri.invoke {}; add #[tauri_plugin_auditaur::auditaur_command] or #[tauri_plugin_auditaur::instrument_ipc] to the Tauri command.",
                    call.command
                ));
            }
        }
        findings
    }
}

fn is_error_level(level: &str) -> bool {
    matches!(
        level.to_ascii_uppercase().as_str(),
        "ERROR" | "FATAL" | "CRITICAL"
    )
}
