use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::{
    model::{FrontendError, LogRecord, Session, SpanRecord},
    protocol::TraceSummary,
    storage::{FrontendErrorQuery, LogQuery, SpanQuery},
};
use serde::Serialize;
use std::path::Path;

use crate::output::table_cell;

pub fn sessions(db: &Path, json: bool, limit: usize) -> Result<()> {
    let store = open_validated_store(db)?;
    let sessions = store.list_sessions(Some(limit))?;
    print_json_or_table(json, &sessions, || print_sessions(&sessions))
}

pub fn logs(
    db: &Path,
    session_id: Option<String>,
    trace_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let store = open_validated_store(db)?;
    let logs = store.list_logs(&LogQuery {
        session_id,
        trace_id,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &logs, || print_logs(&logs))
}

pub fn errors(
    db: &Path,
    session_id: Option<String>,
    trace_id: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let store = open_validated_store(db)?;
    let errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id,
        trace_id,
        limit: Some(limit),
    })?;
    print_json_or_table(json, &errors, || print_errors(&errors))
}

pub fn traces(db: &Path, session_id: Option<String>, json: bool, limit: usize) -> Result<()> {
    let store = open_validated_store(db)?;
    let summaries = store.list_trace_summaries(session_id.as_deref(), Some(limit))?;
    print_json_or_table(json, &summaries, || print_traces(&summaries))
}

pub fn trace(db: &Path, session_id: Option<String>, trace_id: String, json: bool) -> Result<()> {
    let store = open_validated_store(db)?;
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
        session_id,
        trace_id: Some(trace_id.clone()),
        limit: Some(usize::MAX),
    })?;
    let detail = TraceDetail {
        trace_id,
        spans,
        logs,
        frontend_errors: errors,
    };
    print_json_or_table(json, &detail, || print_trace(&detail))
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
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceDetail {
    trace_id: String,
    spans: Vec<SpanRecord>,
    logs: Vec<LogRecord>,
    frontend_errors: Vec<FrontendError>,
}
