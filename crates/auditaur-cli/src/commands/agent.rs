use anyhow::{anyhow, Result};
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::{
    model::{FrontendError, LogRecord, SpanRecord, TauriEventRecord, TauriIpcCall},
    storage::{RelatedTelemetry, RelatedTelemetryQuery, SpanQuery},
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use crate::{
    commands::read,
    discovery::{self, DiscoveryStatus},
    output::table_cell,
};

pub fn guide(json: bool) -> Result<()> {
    let guide = AgentGuide::default();
    read::print_json_or_table(json, &guide, || {
        println!("{}", guide.title);
        println!("{}", guide.summary);
        println!();
        println!("Core readiness: {}", guide.core_readiness);
        println!("Frontend readiness: {}", guide.frontend_readiness);
        println!();
        println!("Workflows:");
        for workflow in &guide.workflows {
            println!("- {}: {}", workflow.name, workflow.when_to_use);
            for command in &workflow.commands {
                println!("    {command}");
            }
        }
        println!();
        println!("Rules:");
        for rule in &guide.rules {
            println!("- {rule}");
        }
        println!();
        println!("Docs: {}", guide.docs);
        Ok(())
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentGuide {
    title: String,
    summary: String,
    docs: String,
    core_readiness: String,
    frontend_readiness: String,
    workflows: Vec<AgentWorkflow>,
    rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentWorkflow {
    name: String,
    when_to_use: String,
    commands: Vec<String>,
}

impl Default for AgentGuide {
    fn default() -> Self {
        Self {
            title: "Auditaur Agent Debugging Guide".to_string(),
            summary: "Use Auditaur as the first diagnostic surface for Tauri apps. Auditaur observes the app; it does not replace the app's normal dev command.".to_string(),
            docs: "docs/getting-started/agent-guide.mdx".to_string(),
            core_readiness: "Default readiness means app/session discovery, telemetry database, session row, Tauri window telemetry, and backend/plugin telemetry are ready when available.".to_string(),
            frontend_readiness: "Use --require-frontend only when frontend telemetry is required; Tauri WebViews may not emit frontend telemetry until a user interaction or app path runs.".to_string(),
            workflows: vec![
                AgentWorkflow {
                    name: "No-config observe".to_string(),
                    when_to_use: "Start a dev app under observation when .auditaur/config.json is not present.".to_string(),
                    commands: vec![
                        "auditaur observe --app <app-name> -- <dev command>".to_string(),
                        "auditaur observe --app <app-name> --require-frontend -- <dev command>".to_string(),
                        "auditaur observe --app <app-name> --port web -- <dev command using {{port:web}}>".to_string(),
                        "auditaur observe --app <app-name> --port-env web=VITE_PORT -- <dev command>".to_string(),
                    ],
                },
                AgentWorkflow {
                    name: "Configured loop".to_string(),
                    when_to_use: "Use when the repo has .auditaur/config.json.".to_string(),
                    commands: vec![
                        "auditaur start".to_string(),
                        "auditaur drill".to_string(),
                        "auditaur inspect".to_string(),
                        "auditaur stop".to_string(),
                    ],
                },
                AgentWorkflow {
                    name: "Attach to running app".to_string(),
                    when_to_use: "Use when the developer, IDE, or another terminal already owns app startup.".to_string(),
                    commands: vec![
                        "auditaur debug --app <app-name> --active --json watch --until-ready".to_string(),
                        "auditaur debug --app <app-name> --active --require-drive-bridge --json watch --until-ready".to_string(),
                    ],
                },
                AgentWorkflow {
                    name: "Pinned follow-up".to_string(),
                    when_to_use: "Use after observe/start writes .auditaur/session.json; prefer --session-file over --latest or copied selectors.".to_string(),
                    commands: vec![
                        "auditaur debug --db <databasePath> --session-id <sessionId> --instance-id <instanceId> --pid <pid> status".to_string(),
                        "auditaur tail --session-file .auditaur/session.json --replay".to_string(),
                        "auditaur logs --session-file .auditaur/session.json".to_string(),
                        "auditaur ipc --session-file .auditaur/session.json --failed".to_string(),
                        "auditaur explain --session-file .auditaur/session.json".to_string(),
                        "auditaur drive --session-id <sessionId> --instance-id <instanceId> --pid <pid> inspect".to_string(),
                        "auditaur stop --session-file .auditaur/session.json".to_string(),
                    ],
                },
            ],
            rules: vec![
                "Preserve the app's normal startup command.".to_string(),
                "For concurrent dev app runs, prefer observe named ports and wire them through {{port:name}} placeholders or --port-env.".to_string(),
                "Do not rely on --latest when stale sessions may exist; use the session file selectors.".to_string(),
                "Use --require-drive-bridge only when selector actions must be ready.".to_string(),
                "If a manual approval is required, use a drill human gate instead of synthesizing trust-sensitive state.".to_string(),
            ],
        }
    }
}

pub fn runs(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    app: Option<String>,
    session_id: Option<String>,
    since: Option<String>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let (db, session_id) =
        resolve_agent_read_selectors(db, session_file, app.as_deref(), session_id)?;
    let store = read::open_validated_store(&db)?;
    let cutoff = read::parse_since_cutoff(since.as_deref())?;
    let spans = store.list_spans(&SpanQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(if since.is_some() {
            usize::MAX
        } else {
            limit.saturating_mul(20).max(limit)
        }),
    })?;

    let mut grouped: BTreeMap<String, Vec<SpanRecord>> = BTreeMap::new();
    for span in spans {
        if cutoff.is_some_and(|cutoff| span.start_time_unix_nanos < cutoff) {
            continue;
        }

        if let Some(run_id) = agentive_run_id(&span.attributes) {
            grouped.entry(run_id).or_default().push(span);
        }
    }

    let mut summaries = Vec::new();
    let mut seen_traces = HashSet::new();
    for (run_id, spans) in grouped {
        let Some(trace_id) = latest_trace_id(&spans) else {
            continue;
        };
        if !seen_traces.insert((run_id.clone(), trace_id.clone())) {
            continue;
        }
        let related = store.related_telemetry(&RelatedTelemetryQuery {
            session_id: session_id.clone(),
            trace_id: Some(trace_id),
            window_label: None,
            start_time_unix_nanos: None,
            end_time_unix_nanos: None,
            limit: Some(usize::MAX),
        })?;
        summaries.push(AgentRunSummary::from_related(run_id, &related));
    }
    summaries.sort_by(|left, right| {
        right
            .start_time_unix_nanos
            .cmp(&left.start_time_unix_nanos)
            .then_with(|| left.run_id.cmp(&right.run_id))
    });
    summaries.truncate(limit);
    read::print_json_or_table(json, &summaries, || print_agent_runs(&summaries))
}

pub fn run(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    app: Option<String>,
    session_id: Option<String>,
    run_id: String,
    json: bool,
) -> Result<()> {
    let (db, session_id) =
        resolve_agent_read_selectors(db, session_file, app.as_deref(), session_id)?;
    let store = read::open_validated_store(&db)?;
    let trace_id = find_run_trace_id(&store, session_id.as_deref(), &run_id)?
        .ok_or_else(|| anyhow!("No agent run `{run_id}` found. Try `auditaur agent-runs`."))?;
    let related = store.related_telemetry(&RelatedTelemetryQuery {
        session_id,
        trace_id: Some(trace_id),
        window_label: None,
        start_time_unix_nanos: None,
        end_time_unix_nanos: None,
        limit: Some(usize::MAX),
    })?;
    let detail = AgentRunDetail::from_related(run_id, related);
    read::print_json_or_table(json, &detail, || print_agent_run(&detail))
}

pub(crate) fn find_run_trace_id(
    store: &SqliteStore,
    session_id: Option<&str>,
    run_id: &str,
) -> Result<Option<String>> {
    let spans = store.list_spans(&SpanQuery {
        session_id: session_id.map(ToString::to_string),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    Ok(spans
        .into_iter()
        .filter(|span| agentive_run_id(&span.attributes).as_deref() == Some(run_id))
        .max_by_key(|span| span.start_time_unix_nanos)
        .map(|span| span.trace_id))
}

fn resolve_agent_db(db: &Option<PathBuf>, app: Option<&str>) -> Result<PathBuf> {
    if db.is_some() || app.is_none() {
        return discovery::resolve_db(db.clone());
    }

    let app = app.expect("checked above").to_ascii_lowercase();
    let mut matches: Vec<_> = discovery::list_apps()?
        .into_iter()
        .filter(|candidate| candidate.database_readable && candidate.schema_valid)
        .filter(|candidate| {
            candidate.service_name.to_ascii_lowercase().contains(&app)
                || candidate
                    .app_identifier
                    .as_deref()
                    .is_some_and(|identifier| identifier.to_ascii_lowercase().contains(&app))
                || candidate.session_id.to_ascii_lowercase().contains(&app)
        })
        .collect();
    matches.sort_by(|left, right| {
        let left_active = left.status == DiscoveryStatus::Active;
        let right_active = right.status == DiscoveryStatus::Active;
        right_active
            .cmp(&left_active)
            .then_with(|| right.last_heartbeat_at.cmp(&left.last_heartbeat_at))
    });
    matches
        .first()
        .map(|app| PathBuf::from(&app.database_path))
        .ok_or_else(|| anyhow!("No discoverable Auditaur app matched `{app}`."))
}

fn resolve_agent_read_selectors(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    app: Option<&str>,
    session_id: Option<String>,
) -> Result<(PathBuf, Option<String>)> {
    let (db, session_id) = read::resolve_read_selectors(db, session_file, session_id)?;
    if session_file.is_some() {
        if let Some(app) = app {
            return Err(anyhow!(
                "`--app {app}` cannot be combined with `--session-file`; the session file already pins the database and session"
            ));
        }
        return discovery::resolve_db(db).map(|db| (db, session_id));
    }
    resolve_agent_db(&db, app).map(|db| (db, session_id))
}

fn latest_trace_id(spans: &[SpanRecord]) -> Option<String> {
    spans
        .iter()
        .max_by_key(|span| span.start_time_unix_nanos)
        .map(|span| span.trace_id.clone())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunSummary {
    run_id: String,
    trace_id: String,
    session_id: String,
    root_command: Option<String>,
    root_span: Option<String>,
    status: String,
    start_time_unix_nanos: i64,
    duration_unix_nanos: Option<i64>,
    provider: Option<String>,
    model: Option<String>,
    model_call_count: usize,
    tool_call_count: usize,
    agent_event_count: usize,
    log_count: usize,
    error_count: usize,
    final_summary: Option<String>,
}

impl AgentRunSummary {
    fn from_related(run_id: String, related: &RelatedTelemetry) -> Self {
        let agent_spans = agent_spans_for_run(&run_id, &related.spans);
        let trace_id = agent_spans
            .first()
            .map(|span| span.trace_id.clone())
            .or_else(|| related.spans.first().map(|span| span.trace_id.clone()))
            .unwrap_or_default();
        let session_id = agent_spans
            .first()
            .map(|span| span.session_id.clone())
            .or_else(|| related.spans.first().map(|span| span.session_id.clone()))
            .unwrap_or_default();
        let root_span = root_span(&related.spans, &trace_id);
        let root_command = root_command(&related.tauri_ipc_calls, &trace_id);
        let model_calls = model_calls(&run_id, &related.spans);
        let tool_calls = tool_calls(&run_id, &related.spans);
        let events = agent_events(&run_id, related);
        let error_count = related.frontend_errors.len()
            + related
                .logs
                .iter()
                .filter(|log| log.severity_text.as_deref().is_some_and(is_error_level))
                .count()
            + related
                .tauri_ipc_calls
                .iter()
                .filter(|call| read::is_failed_ipc_status(&call.status))
                .count()
            + related
                .spans
                .iter()
                .filter(|span| span.status_code.as_deref().is_some_and(is_error_status))
                .count();
        let (start_time_unix_nanos, duration_unix_nanos) = span_duration(&related.spans);
        let first_model = model_calls.first();
        Self {
            run_id,
            trace_id,
            session_id,
            root_command,
            root_span: root_span.map(|span| span.name.clone()),
            status: if error_count > 0 { "ERROR" } else { "OK" }.to_string(),
            start_time_unix_nanos,
            duration_unix_nanos,
            provider: first_model.and_then(|call| call.provider.clone()),
            model: first_model.and_then(|call| call.model.clone()),
            model_call_count: model_calls.len(),
            tool_call_count: tool_calls.len(),
            agent_event_count: events.len(),
            log_count: related.logs.len(),
            error_count,
            final_summary: final_summary(&events, related),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunDetail {
    run: AgentRunSummary,
    model_calls: Vec<AgentModelCall>,
    tool_calls: Vec<AgentToolCall>,
    agent_events: Vec<AgentEvent>,
    logs: Vec<LogRecord>,
    frontend_errors: Vec<FrontendError>,
    tauri_ipc_calls: Vec<TauriIpcCall>,
    tauri_events: Vec<TauriEventRecord>,
}

impl AgentRunDetail {
    fn from_related(run_id: String, related: RelatedTelemetry) -> Self {
        let run = AgentRunSummary::from_related(run_id.clone(), &related);
        Self {
            model_calls: model_calls(&run_id, &related.spans),
            tool_calls: tool_calls(&run_id, &related.spans),
            agent_events: agent_events(&run_id, &related),
            logs: related.logs,
            frontend_errors: related.frontend_errors,
            tauri_ipc_calls: related.tauri_ipc_calls,
            tauri_events: related.tauri_events,
            run,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentModelCall {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    start_time_unix_nanos: i64,
    iteration: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    status: Option<String>,
    duration_unix_nanos: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentToolCall {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    start_time_unix_nanos: i64,
    iteration: Option<String>,
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    status: Option<String>,
    duration_unix_nanos: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentEvent {
    timestamp_unix_nanos: i64,
    source: String,
    name: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    summary: Option<String>,
    attributes: Option<Value>,
}

fn print_agent_runs(runs: &[AgentRunSummary]) -> Result<()> {
    println!(
        "RUN\tTRACE\tSTATUS\tDURATION_NS\tMODELS\tTOOLS\tEVENTS\tPROVIDER\tMODEL\tROOT\tFINAL"
    );
    for run in runs {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            table_cell(&run.run_id, 80),
            table_cell(&run.trace_id, 80),
            run.status,
            run.duration_unix_nanos.unwrap_or_default(),
            run.model_call_count,
            run.tool_call_count,
            run.agent_event_count,
            table_cell(run.provider.as_deref().unwrap_or("-"), 40),
            table_cell(run.model.as_deref().unwrap_or("-"), 60),
            table_cell(
                run.root_command
                    .as_deref()
                    .or(run.root_span.as_deref())
                    .unwrap_or("-"),
                120
            ),
            table_cell(run.final_summary.as_deref().unwrap_or("-"), 160)
        );
    }
    Ok(())
}

fn print_agent_run(detail: &AgentRunDetail) -> Result<()> {
    println!("Agent run {}", detail.run.run_id);
    println!("Trace: {}", detail.run.trace_id);
    println!("Status: {}", detail.run.status);
    println!(
        "Root: {}",
        detail
            .run
            .root_command
            .as_deref()
            .or(detail.run.root_span.as_deref())
            .unwrap_or("-")
    );
    println!(
        "Model calls: {}  Tool calls: {}  Agent events: {}  Logs: {}  Errors: {}",
        detail.run.model_call_count,
        detail.run.tool_call_count,
        detail.run.agent_event_count,
        detail.run.log_count,
        detail.run.error_count
    );
    if let Some(summary) = &detail.run.final_summary {
        println!("Final: {}", table_cell(summary, 240));
    }
    println!("MODEL_CALL\tITERATION\tPROVIDER\tMODEL\tSTATUS\tDURATION_NS\tTOKENS");
    for call in &detail.model_calls {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            table_cell(&call.span_id, 80),
            table_cell(call.iteration.as_deref().unwrap_or("-"), 20),
            table_cell(call.provider.as_deref().unwrap_or("-"), 40),
            table_cell(call.model.as_deref().unwrap_or("-"), 60),
            table_cell(call.status.as_deref().unwrap_or("-"), 20),
            call.duration_unix_nanos.unwrap_or_default(),
            token_summary(call.input_tokens, call.output_tokens, call.total_tokens)
        );
    }
    println!("TOOL_CALL\tITERATION\tTOOL\tCALL_ID\tSTATUS\tDURATION_NS");
    for call in &detail.tool_calls {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            table_cell(&call.span_id, 80),
            table_cell(call.iteration.as_deref().unwrap_or("-"), 20),
            table_cell(call.tool_name.as_deref().unwrap_or("-"), 80),
            table_cell(call.tool_call_id.as_deref().unwrap_or("-"), 80),
            table_cell(call.status.as_deref().unwrap_or("-"), 20),
            call.duration_unix_nanos.unwrap_or_default()
        );
    }
    println!("AGENT_EVENT\tSOURCE\tSUMMARY");
    for event in &detail.agent_events {
        println!(
            "{}\t{}\t{}",
            table_cell(&event.name, 80),
            table_cell(&event.source, 20),
            table_cell(event.summary.as_deref().unwrap_or("-"), 180)
        );
    }
    Ok(())
}

fn agent_spans_for_run<'a>(run_id: &str, spans: &'a [SpanRecord]) -> Vec<&'a SpanRecord> {
    spans
        .iter()
        .filter(|span| {
            agentive_run_id(&span.attributes).as_deref() == Some(run_id)
                || is_agentive_span_name(&span.name)
        })
        .collect()
}

fn model_calls(run_id: &str, spans: &[SpanRecord]) -> Vec<AgentModelCall> {
    let mut calls: Vec<_> = spans
        .iter()
        .filter(|span| agentive_run_id(&span.attributes).as_deref() == Some(run_id))
        .filter(|span| is_model_span(span))
        .map(|span| AgentModelCall {
            trace_id: span.trace_id.clone(),
            span_id: span.span_id.clone(),
            parent_span_id: span.parent_span_id.clone(),
            start_time_unix_nanos: span.start_time_unix_nanos,
            iteration: iteration(&span.attributes),
            provider: attr_string(
                &span.attributes,
                &["gen_ai.system", "llm.system", "provider"],
            ),
            model: attr_string(
                &span.attributes,
                &[
                    "gen_ai.request.model",
                    "gen_ai.response.model",
                    "llm.model_name",
                    "model",
                ],
            ),
            status: span.status_code.clone(),
            duration_unix_nanos: span_duration_one(span),
            input_tokens: attr_i64(
                &span.attributes,
                &[
                    "gen_ai.usage.input_tokens",
                    "llm.usage.prompt_tokens",
                    "prompt_tokens",
                ],
            ),
            output_tokens: attr_i64(
                &span.attributes,
                &[
                    "gen_ai.usage.output_tokens",
                    "llm.usage.completion_tokens",
                    "completion_tokens",
                ],
            ),
            total_tokens: attr_i64(
                &span.attributes,
                &[
                    "gen_ai.usage.total_tokens",
                    "llm.usage.total_tokens",
                    "total_tokens",
                ],
            ),
        })
        .collect();
    calls.sort_by_key(|call| call.start_time_unix_nanos);
    calls
}

fn tool_calls(run_id: &str, spans: &[SpanRecord]) -> Vec<AgentToolCall> {
    let mut calls: Vec<_> = spans
        .iter()
        .filter(|span| agentive_run_id(&span.attributes).as_deref() == Some(run_id))
        .filter(|span| is_tool_span(span))
        .map(|span| AgentToolCall {
            trace_id: span.trace_id.clone(),
            span_id: span.span_id.clone(),
            parent_span_id: span.parent_span_id.clone(),
            start_time_unix_nanos: span.start_time_unix_nanos,
            iteration: iteration(&span.attributes),
            tool_name: attr_string(
                &span.attributes,
                &[
                    "agentive.tool_name",
                    "agentive.tool.name",
                    "gen_ai.tool.name",
                    "tool.name",
                    "tool_name",
                ],
            ),
            tool_call_id: attr_string(
                &span.attributes,
                &[
                    "agentive.tool_call_id",
                    "gen_ai.tool.call.id",
                    "tool.call.id",
                    "tool_call_id",
                ],
            ),
            status: span.status_code.clone(),
            duration_unix_nanos: span_duration_one(span),
        })
        .collect();
    calls.sort_by_key(|call| call.start_time_unix_nanos);
    calls
}

fn agent_events(run_id: &str, related: &RelatedTelemetry) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for event in &related.span_events {
        let parent_has_run_id = related.spans.iter().any(|span| {
            span.trace_id == event.trace_id
                && span.span_id == event.span_id
                && agentive_run_id(&span.attributes).as_deref() == Some(run_id)
        });
        if parent_has_run_id || event.name.contains("agent") {
            events.push(AgentEvent {
                timestamp_unix_nanos: event.timestamp_unix_nanos,
                source: "span_event".to_string(),
                name: event.name.clone(),
                trace_id: Some(event.trace_id.clone()),
                span_id: Some(event.span_id.clone()),
                summary: event_summary(&event.attributes),
                attributes: Some(event.attributes.clone()),
            });
        }
    }
    for event in &related.tauri_events {
        if event.event_name.contains("agent") {
            events.push(AgentEvent {
                timestamp_unix_nanos: event.timestamp_unix_nanos,
                source: "tauri_event".to_string(),
                name: event.event_name.clone(),
                trace_id: event.trace_id.clone(),
                span_id: event.span_id.clone(),
                summary: event
                    .payload_summary
                    .clone()
                    .or_else(|| event.payload_json.as_ref().and_then(event_summary)),
                attributes: event.payload_json.clone(),
            });
        }
    }
    events.sort_by_key(|event| event.timestamp_unix_nanos);
    events
}

fn final_summary(events: &[AgentEvent], related: &RelatedTelemetry) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|event| {
            let name = event.name.to_ascii_lowercase();
            name.contains("done") || name.contains("final") || name.contains("response")
        })
        .and_then(|event| event.summary.clone())
        .or_else(|| {
            related
                .tauri_ipc_calls
                .iter()
                .filter(|call| call.command.contains("agent"))
                .filter_map(|call| call.result_summary.clone())
                .next()
        })
}

fn root_span<'a>(spans: &'a [SpanRecord], trace_id: &str) -> Option<&'a SpanRecord> {
    spans
        .iter()
        .filter(|span| span.trace_id == trace_id)
        .min_by_key(|span| {
            (
                span.parent_span_id.is_some(),
                span.start_time_unix_nanos,
                span.span_id.clone(),
            )
        })
}

fn root_command(calls: &[TauriIpcCall], trace_id: &str) -> Option<String> {
    calls
        .iter()
        .filter(|call| call.trace_id.as_deref() == Some(trace_id))
        .min_by_key(|call| call.timestamp_unix_nanos)
        .map(|call| call.command.clone())
}

fn span_duration(spans: &[SpanRecord]) -> (i64, Option<i64>) {
    let Some(start) = spans.iter().map(|span| span.start_time_unix_nanos).min() else {
        return (0, None);
    };
    let end = spans
        .iter()
        .filter_map(|span| span.end_time_unix_nanos)
        .max()
        .unwrap_or(start);
    (start, Some(end.saturating_sub(start).max(0)))
}

fn span_duration_one(span: &SpanRecord) -> Option<i64> {
    span.end_time_unix_nanos
        .map(|end| end.saturating_sub(span.start_time_unix_nanos).max(0))
}

fn is_model_span(span: &SpanRecord) -> bool {
    span.name.contains("model_call")
        || attr_string(&span.attributes, &["gen_ai.operation.name"])
            .as_deref()
            .is_some_and(|value| value.contains("chat") || value.contains("completion"))
}

fn is_tool_span(span: &SpanRecord) -> bool {
    span.name.contains("tool_call")
        || attr_string(
            &span.attributes,
            &[
                "agentive.tool_name",
                "agentive.tool.name",
                "gen_ai.tool.name",
                "tool.name",
                "tool_name",
            ],
        )
        .is_some()
}

fn is_agentive_span_name(name: &str) -> bool {
    name.starts_with("agentive.") || name.contains("agentive.")
}

fn is_error_level(level: &str) -> bool {
    matches!(
        level.to_ascii_uppercase().as_str(),
        "ERROR" | "FATAL" | "CRITICAL"
    )
}

fn is_error_status(status: &str) -> bool {
    !status.eq_ignore_ascii_case("OK")
}

fn agentive_run_id(attributes: &Value) -> Option<String> {
    attr_string(
        attributes,
        &["agentive.run_id", "agent.run.id", "agent.run_id", "run_id"],
    )
}

fn iteration(attributes: &Value) -> Option<String> {
    attr_string(
        attributes,
        &["agentive.iteration", "agent.iteration", "iteration"],
    )
}

fn event_summary(attributes: &Value) -> Option<String> {
    attr_string(
        attributes,
        &[
            "summary",
            "message",
            "response",
            "final_response",
            "content",
            "output",
            "result",
        ],
    )
}

fn attr_string(attributes: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| attributes.get(*key))
        .find_map(|value| {
            let value = value_to_string(value)?;
            let value = value.trim().to_string();
            (!value.is_empty()).then_some(value)
        })
}

fn attr_i64(attributes: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .filter_map(|key| attributes.get(*key))
        .find_map(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Array(_) | Value::Object(_) => Some(value.to_string()),
        Value::Null => None,
    }
}

fn token_summary(input: Option<i64>, output: Option<i64>, total: Option<i64>) -> String {
    match (input, output, total) {
        (Some(input), Some(output), Some(total)) => format!("{input}/{output}/{total}"),
        (_, _, Some(total)) => total.to_string(),
        (Some(input), Some(output), None) => format!("{input}/{output}/-"),
        _ => "-".to_string(),
    }
}
