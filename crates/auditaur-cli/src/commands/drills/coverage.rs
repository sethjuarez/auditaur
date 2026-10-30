//! `auditaur drill coverage`: which IPC commands and events a run's tests exercised.
//!
//! Each attempt folder holds a `telemetry.json` attachment (the app telemetry
//! recorded while the attempt ran). Coverage folds those into per-command and
//! per-event tallies. Optional telemetry stores (`--db`) add everything the app
//! has been seen doing, and the registered command surface (see [`super::surface`])
//! adds every command the app declares, so ones no test touched show up too.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use auditaur_core::{
    model::{SpanRecord, TauriIpcCall, TelemetrySource},
    storage::{RelatedTelemetry, SpanQuery, TauriEventQuery, TauriIpcQuery},
};
use serde::Serialize;

use super::run::{self, RunReport, Status};
use super::surface::{self, SurfaceSummary};
use crate::commands::read::{is_failed_ipc_status, open_validated_store};

const TELEMETRY_ATTACHMENT: &str = "telemetry";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub run_id: String,
    pub run_status: Status,
    pub dir: String,
    pub totals: CoverageTotals,
    pub commands: Vec<CommandCoverage>,
    pub events: Vec<EventCoverage>,
    pub tests: Vec<TestCoverage>,
    pub sources: Vec<SourceSummary>,
    /// Where the registered command surface was read from.
    pub surface: Vec<SurfaceSummary>,
    /// Whether totals are out of a known surface rather than only what the run touched.
    pub baseline: Baseline,
    /// What no test exercised; empty for a kind without a baseline.
    pub unexercised: Unexercised,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    /// A readable store or registered surface.
    pub commands: bool,
    /// A readable store.
    pub events: bool,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Unexercised {
    pub commands: Vec<String>,
    pub events: Vec<String>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoverageTotals {
    pub commands: usize,
    pub commands_exercised: usize,
    pub events: usize,
    pub events_exercised: usize,
    pub tests: usize,
    /// Tests whose every recorded attempt's telemetry could be read.
    pub tests_with_telemetry: usize,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandCoverage {
    pub command: String,
    /// Calls made while tests ran.
    pub calls: usize,
    pub failed: usize,
    /// Ids of the tests that called it, in run order.
    pub tests: Vec<String>,
    /// Calls recorded in the `--db` stores; 0 when no store was given.
    pub seen: usize,
    /// Listed by the registered command surface.
    pub registered: bool,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EventCoverage {
    pub event: String,
    /// Directions observed while tests ran, such as `emit` and `receive`.
    pub directions: BTreeSet<String>,
    pub count: usize,
    pub tests: Vec<String>,
    pub seen: usize,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestCoverage {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub commands: Vec<String>,
    pub events: Vec<String>,
    /// `recorded`, `none` (no attempt recorded any), `unreadable`, or
    /// `partial` (some attempts read, others not; see `error`).
    pub telemetry: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub db: String,
    pub ipc_calls: usize,
    pub events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What is known about the app beyond the run: telemetry recorded in stores, reduced to what
/// coverage needs, and the commands it registers.
#[derive(Debug, Default)]
pub struct Seen {
    pub commands: BTreeMap<String, usize>,
    pub events: BTreeMap<String, usize>,
    pub registered: BTreeSet<String>,
}

/// Build coverage for one run folder plus telemetry already read from stores.
pub fn build(run_dir: &Path, report: &RunReport, seen: &Seen) -> Coverage {
    let mut commands: BTreeMap<String, CommandCoverage> = BTreeMap::new();
    let mut events: BTreeMap<String, EventCoverage> = BTreeMap::new();
    let mut tests = Vec::new();

    for test in report.suites.iter().flat_map(|suite| &suite.tests) {
        let mut entry = TestCoverage {
            id: test.id.clone(),
            title: test.title.clone(),
            status: test.status,
            commands: Vec::new(),
            events: Vec::new(),
            telemetry: "none",
            error: None,
        };
        let mut test_commands = BTreeSet::new();
        let mut test_events = BTreeSet::new();
        let mut recorded = false;
        for summary in &test.attempts {
            let telemetry = match read_telemetry(run_dir, summary) {
                Ok(Some(telemetry)) => telemetry,
                Ok(None) => continue,
                Err(error) => {
                    entry.error.get_or_insert_with(|| format!("{error:#}"));
                    continue;
                }
            };
            recorded = true;
            for (command, failed) in command_calls(&telemetry.tauri_ipc_calls, &telemetry.spans) {
                let item = commands
                    .entry(command.to_string())
                    .or_insert_with(|| CommandCoverage {
                        command: command.to_string(),
                        ..Default::default()
                    });
                item.calls += 1;
                if failed {
                    item.failed += 1;
                }
                if test_commands.insert(command.to_string()) {
                    item.tests.push(test.id.clone());
                }
            }
            for event in &telemetry.tauri_events {
                let item =
                    events
                        .entry(event.event_name.clone())
                        .or_insert_with(|| EventCoverage {
                            event: event.event_name.clone(),
                            ..Default::default()
                        });
                item.count += 1;
                item.directions.insert(event.direction.clone());
                if test_events.insert(event.event_name.clone()) {
                    item.tests.push(test.id.clone());
                }
            }
        }
        entry.telemetry = match (recorded, entry.error.is_some()) {
            (true, false) => "recorded",
            (true, true) => "partial",
            (false, true) => "unreadable",
            (false, false) => "none",
        };
        entry.commands = test_commands.into_iter().collect();
        entry.events = test_events.into_iter().collect();
        tests.push(entry);
    }

    for (command, count) in &seen.commands {
        commands
            .entry(command.clone())
            .or_insert_with(|| CommandCoverage {
                command: command.clone(),
                ..Default::default()
            })
            .seen = *count;
    }
    for command in &seen.registered {
        commands
            .entry(command.clone())
            .or_insert_with(|| CommandCoverage {
                command: command.clone(),
                ..Default::default()
            })
            .registered = true;
    }
    for (event, count) in &seen.events {
        events
            .entry(event.clone())
            .or_insert_with(|| EventCoverage {
                event: event.clone(),
                ..Default::default()
            })
            .seen = *count;
    }

    let commands: Vec<CommandCoverage> = commands.into_values().collect();
    let events: Vec<EventCoverage> = events.into_values().collect();
    let totals = CoverageTotals {
        commands: commands.len(),
        commands_exercised: commands.iter().filter(|c| !c.tests.is_empty()).count(),
        events: events.len(),
        events_exercised: events.iter().filter(|e| !e.tests.is_empty()).count(),
        tests: tests.len(),
        tests_with_telemetry: tests.iter().filter(|t| t.telemetry == "recorded").count(),
    };
    Coverage {
        run_id: report.run_id.clone(),
        run_status: report.status,
        dir: format!("{}/{}", run::RUNS_DIR, report.run_id),
        totals,
        commands,
        events,
        tests,
        sources: Vec::new(),
        surface: Vec::new(),
        baseline: Baseline::default(),
        unexercised: Unexercised::default(),
    }
}

/// Mark which kinds have a baseline, from readable stores and surfaces, and list what no test
/// exercised for those kinds.
fn settle_baseline(coverage: &mut Coverage) {
    let store = coverage.sources.iter().any(|s| s.error.is_none());
    let surface = coverage.surface.iter().any(|s| s.error.is_none());
    coverage.baseline = Baseline {
        commands: store || surface,
        events: store,
    };
    let missed = |tests: &Vec<String>| tests.is_empty();
    coverage.unexercised = Unexercised {
        commands: match coverage.baseline.commands {
            true => coverage
                .commands
                .iter()
                .filter(|c| missed(&c.tests))
                .map(|c| c.command.clone())
                .collect(),
            false => Vec::new(),
        },
        events: match coverage.baseline.events {
            true => coverage
                .events
                .iter()
                .filter(|e| missed(&e.tests))
                .map(|e| e.event.clone())
                .collect(),
            false => Vec::new(),
        },
    };
}

/// The attempt's `telemetry.json`, or `None` when the attempt has none (yet).
fn read_telemetry(
    run_dir: &Path,
    summary: &run::AttemptSummary,
) -> Result<Option<RelatedTelemetry>> {
    let attempt = match run::read_attempt(run_dir, summary) {
        Ok(attempt) => attempt,
        // A running attempt has no attempt.json yet.
        Err(_) if summary.status == Status::Running => return Ok(None),
        Err(error) => return Err(error),
    };
    let Some(attachment) = attempt
        .attachments
        .iter()
        .find(|attachment| attachment.name == TELEMETRY_ATTACHMENT)
    else {
        return Ok(None);
    };
    if !run::is_contained(&attachment.path) {
        anyhow::bail!("telemetry `{}` is not inside the run", attachment.path);
    }
    let path = run_dir.join(&attachment.path);
    // Links inside the run must not lead outside it.
    let root = run_dir
        .canonicalize()
        .with_context(|| format!("failed to resolve `{}`", run_dir.display()))?;
    let real = path
        .canonicalize()
        .with_context(|| format!("failed to resolve `{}`", path.display()))?;
    if !real.starts_with(&root) {
        anyhow::bail!("telemetry `{}` resolves outside the run", attachment.path);
    }
    let text = fs::read_to_string(&real)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    let telemetry = serde_json::from_str(&text)
        .with_context(|| format!("`{}` is not attempt telemetry", path.display()))?;
    Ok(Some(telemetry))
}

/// Every command call as `(command, failed)`: the frontend's recorded IPC calls, plus backend
/// command spans (named by `tauri.command`) that no recorded call made, such as commands an
/// app invokes without `@auditaur/api`'s instrumented `invoke`.
fn command_calls<'a>(
    calls: &'a [TauriIpcCall],
    spans: &'a [SpanRecord],
) -> impl Iterator<Item = (&'a str, bool)> {
    // A backend span continues the frontend invoke span whose traceparent it read. Spans that
    // read none (propagation off, older plugins) pair with a same-named call nothing continued.
    let backend: Vec<(&SpanRecord, &str)> = spans
        .iter()
        .filter(|span| span.source == TelemetrySource::Backend)
        .filter_map(|span| Some((span, span.attributes.get("tauri.command")?.as_str()?)))
        .collect();
    let continued: BTreeSet<(&str, &str)> = backend
        .iter()
        .filter_map(|(span, _)| Some((span.trace_id.as_str(), span.parent_span_id.as_deref()?)))
        .collect();
    let invoked: BTreeSet<(&str, &str)> = calls
        .iter()
        .filter_map(|call| Some((call.trace_id.as_deref()?, call.span_id.as_deref()?)))
        .collect();
    let mut unpaired: BTreeMap<&str, usize> = BTreeMap::new();
    for call in calls {
        let linked = call
            .trace_id
            .as_deref()
            .zip(call.span_id.as_deref())
            .is_some_and(|key| continued.contains(&key));
        if !linked {
            *unpaired.entry(call.command.as_str()).or_default() += 1;
        }
    }
    let backend = backend.into_iter().filter_map(move |(span, command)| {
        let parent = span.parent_span_id.as_deref();
        if parent.is_some_and(|parent| invoked.contains(&(span.trace_id.as_str(), parent))) {
            return None;
        }
        if let Some(count) = unpaired.get_mut(command).filter(|count| **count > 0) {
            *count -= 1;
            return None;
        }
        Some((command, span.status_code.as_deref() == Some("ERROR")))
    });
    calls
        .iter()
        .map(|call| (call.command.as_str(), is_failed_ipc_status(&call.status)))
        .chain(backend)
}

/// Tally commands and events recorded in each store. A store that cannot be
/// read is reported in its summary rather than failing the whole command.
pub fn read_seen(dbs: &[PathBuf]) -> (Seen, Vec<SourceSummary>) {
    let mut seen = Seen::default();
    let mut sources = Vec::new();
    for db in dbs {
        let mut summary = SourceSummary {
            db: db.display().to_string(),
            ipc_calls: 0,
            events: 0,
            error: None,
        };
        if let Err(error) = tally_store(db, &mut seen, &mut summary) {
            summary.error = Some(format!("{error:#}"));
        }
        sources.push(summary);
    }
    (seen, sources)
}

fn tally_store(db: &Path, seen: &mut Seen, summary: &mut SourceSummary) -> Result<()> {
    // Opening a missing path would create an empty store.
    if !db.is_file() {
        anyhow::bail!("no telemetry store at `{}`", db.display());
    }
    let store = open_validated_store(db)?;
    let calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id: None,
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let events = store.list_tauri_events(&TauriEventQuery {
        session_id: None,
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let spans = store.list_spans(&SpanQuery {
        session_id: None,
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    summary.ipc_calls = calls.len();
    summary.events = events.len();
    for (command, _) in command_calls(&calls, &spans) {
        *seen.commands.entry(command.to_string()).or_default() += 1;
    }
    for event in events {
        *seen.events.entry(event.event_name).or_default() += 1;
    }
    Ok(())
}

/// The `drill coverage --json` document.
pub fn coverage_json(workspace: &Path, options: &CoverageOptions) -> Result<Coverage> {
    let dir = run::resolve_run(&run::runs_root(&workspace), options.run.as_deref())?;
    let report = run::read_run(&dir)?;
    let (mut seen, sources) = read_seen(&options.dbs);
    let (registered, surface) = surface::read(surface::paths(
        workspace,
        &options.commands,
        &options.config,
    ));
    seen.registered = registered;
    let mut coverage = build(&dir, &report, &seen);
    coverage.sources = sources;
    coverage.surface = surface;
    settle_baseline(&mut coverage);
    Ok(coverage)
}

#[derive(Debug, Default)]
pub struct CoverageOptions {
    /// `latest` (or `None`), a run id, or a prefix.
    pub run: Option<String>,
    pub dbs: Vec<PathBuf>,
    /// Registered-command surface paths; empty means the config's, or the default.
    pub commands: Vec<PathBuf>,
    /// `.auditaur/config.json`, relative to the workspace unless absolute.
    pub config: PathBuf,
    pub json: bool,
}

/// `auditaur drill coverage [run] [--db <path>]... [--commands <path>]...`.
pub fn coverage(options: CoverageOptions) -> Result<()> {
    let workspace = std::env::current_dir()?;
    let json = options.json;
    let coverage = coverage_json(&workspace, &options)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&coverage)?);
        return Ok(());
    }
    print_coverage(&coverage);
    Ok(())
}

fn print_coverage(coverage: &Coverage) {
    let totals = &coverage.totals;
    println!(
        "Run {}  {}  ({} of {} tests recorded telemetry)",
        coverage.run_id,
        coverage.run_status.as_str(),
        totals.tests_with_telemetry,
        totals.tests
    );
    for source in &coverage.sources {
        match &source.error {
            Some(error) => eprintln!("  store {}: {error}", source.db),
            None => println!(
                "  store {}: {} calls, {} events",
                source.db, source.ipc_calls, source.events
            ),
        }
    }
    for surface in &coverage.surface {
        match &surface.error {
            Some(error) => eprintln!("  commands {}: {error}", surface.path),
            None => println!(
                "  commands {}: {} registered",
                surface.path, surface.commands
            ),
        }
    }
    // Without a baseline, the totals are only what the run touched: "5 of 5" would claim
    // full coverage of a surface nothing listed.
    let tally = |baseline: bool, exercised: usize, total: usize| match baseline {
        true => format!("{exercised} of {total} exercised"),
        false => format!("{exercised} touched"),
    };
    let registered = coverage.surface.iter().any(|s| s.error.is_none());
    if !coverage.baseline.commands {
        println!("  no command baseline: list src-tauri/src or pass --commands <path> or --db <app store> to show what no test called");
    }
    if !coverage.baseline.events {
        println!("  no event baseline: pass --db <app store> to list events no test touched");
    }
    println!();
    println!(
        "Commands  {}",
        tally(
            coverage.baseline.commands,
            totals.commands_exercised,
            totals.commands
        )
    );
    for command in &coverage.commands {
        let failed = match command.failed {
            0 => String::new(),
            count => format!(", {count} failed"),
        };
        // Called but not listed: a plugin command, or one the surface misses.
        let unregistered = match registered && !command.registered {
            true => ", not registered",
            false => "",
        };
        println!(
            "  {}  {}  ({} calls{failed}, {} tests{unregistered})",
            mark(&command.tests),
            command.command,
            command.calls,
            command.tests.len()
        );
    }
    println!();
    println!(
        "Events  {}",
        tally(
            coverage.baseline.events,
            totals.events_exercised,
            totals.events
        )
    );
    for event in &coverage.events {
        let directions: Vec<&str> = event.directions.iter().map(String::as_str).collect();
        println!(
            "  {}  {}  ({} {}, {} tests)",
            mark(&event.tests),
            event.event,
            event.count,
            if directions.is_empty() {
                "records".to_string()
            } else {
                directions.join("/")
            },
            event.tests.len()
        );
    }
}

fn mark(tests: &[String]) -> &'static str {
    if tests.is_empty() {
        "-"
    } else {
        "+"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(path: &Path, value: &serde_json::Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn attempt(dir: &str, status: &str) -> serde_json::Value {
        json!({ "attempt": 0, "status": status, "startedAt": "t", "durationMs": 1,
                "error": null, "dir": dir })
    }

    fn test(id: &str, status: &str, attempts: Vec<serde_json::Value>) -> serde_json::Value {
        json!({ "id": id, "name": id, "title": format!("s > {id}"), "tags": [],
                "mode": "run", "modeReason": null, "expectedStatus": "passed",
                "status": status, "outcome": null, "durationMs": 1, "attempts": attempts })
    }

    fn attempt_json(root: &Path, dir: &str, telemetry: Option<serde_json::Value>) {
        let attachments = match &telemetry {
            Some(_) => json!([{ "name": "telemetry", "contentType": "application/json",
                                "path": format!("{dir}/telemetry.json") }]),
            None => json!([]),
        };
        write(
            &root.join(dir).join("attempt.json"),
            &json!({ "schemaVersion": 1, "testId": "x", "title": "x", "attempt": 0,
                     "status": "passed", "startedAt": "t", "durationMs": 1, "error": null,
                     "steps": [], "attachments": attachments }),
        );
        if let Some(telemetry) = telemetry {
            write(&root.join(dir).join("telemetry.json"), &telemetry);
        }
    }

    fn telemetry(calls: &[(&str, &str)], events: &[(&str, &str)]) -> serde_json::Value {
        json!({
            "filterNotes": [], "spans": [], "spanEvents": [], "logs": [],
            "frontendErrors": [], "tauriWindows": [],
            "tauriIpcCalls": calls.iter().map(|(command, status)| json!({
                "sessionId": "s", "timestampUnixNanos": 1, "durationMs": null,
                "command": command, "status": status, "errorMessage": null,
                "traceId": null, "spanId": null, "windowLabel": null, "argsJson": null,
                "argsRedacted": false, "resultSummary": null,
            })).collect::<Vec<_>>(),
            "tauriEvents": events.iter().map(|(name, direction)| json!({
                "sessionId": "s", "timestampUnixNanos": 1, "eventName": name,
                "direction": direction, "target": null, "traceId": null, "spanId": null,
                "windowLabel": null, "payloadSummary": null, "payloadJson": null,
                "payloadRedacted": false,
            })).collect::<Vec<_>>(),
        })
    }

    fn report(tests: Vec<serde_json::Value>) -> RunReport {
        serde_json::from_value(json!({
            "schemaVersion": 1, "runId": "r1", "status": "passed", "seq": 0,
            "startedAt": "t", "endedAt": null, "durationMs": null, "pid": 1,
            "workspace": ".", "invocation": { "args": [], "patterns": [], "grep": null, "grepInvert": null, "tags": [] },
            "totals": { "tests": 0, "passed": 0, "failed": 0, "flaky": 0, "skipped": 0, "notRun": 0 },
            "environment": { "os": "x", "arch": "x", "auditaurVersion": "x" },
            "suites": [{ "id": "s", "suite": "s", "path": "s.drill.json", "status": "passed",
                         "startedAt": null, "durationMs": null, "app": null, "error": null,
                         "hooks": [], "tests": tests }],
        }))
        .unwrap()
    }

    #[test]
    fn folds_attempt_telemetry_into_commands_events_and_tests() {
        let root = tempfile::tempdir().unwrap();
        attempt_json(
            root.path(),
            "a/attempt-0",
            Some(telemetry(
                &[("save", "OK"), ("save", "ERROR")],
                &[("ping", "emit")],
            )),
        );
        attempt_json(
            root.path(),
            "b/attempt-0",
            Some(telemetry(&[("save", "OK")], &[("ping", "receive")])),
        );
        attempt_json(
            root.path(),
            "b/attempt-1",
            Some(telemetry(&[("load", "OK")], &[])),
        );
        let report = report(vec![
            test("a", "passed", vec![attempt("a/attempt-0", "passed")]),
            test(
                "b",
                "passed",
                vec![
                    attempt("b/attempt-0", "failed"),
                    attempt("b/attempt-1", "passed"),
                ],
            ),
            test("c", "skipped", vec![]),
        ]);
        let mut seen = Seen::default();
        seen.commands.insert("save".into(), 9);
        seen.commands.insert("delete".into(), 2);
        seen.events.insert("tick".into(), 4);

        let coverage = build(root.path(), &report, &seen);

        assert_eq!(
            coverage.totals,
            CoverageTotals {
                commands: 3,
                commands_exercised: 2,
                events: 2,
                events_exercised: 1,
                tests: 3,
                tests_with_telemetry: 2,
            }
        );
        let save = coverage
            .commands
            .iter()
            .find(|c| c.command == "save")
            .unwrap();
        assert_eq!((save.calls, save.failed, save.seen), (3, 1, 9));
        assert_eq!(save.tests, vec!["a", "b"]);
        let delete = coverage
            .commands
            .iter()
            .find(|c| c.command == "delete")
            .unwrap();
        assert!(delete.tests.is_empty());
        assert_eq!(delete.seen, 2);
        let ping = &coverage.events[0];
        assert_eq!(ping.event, "ping");
        assert_eq!(
            ping.directions.iter().collect::<Vec<_>>(),
            ["emit", "receive"]
        );
        assert_eq!(coverage.tests[1].commands, vec!["load", "save"]);
        assert_eq!(coverage.tests[2].telemetry, "none");
    }

    #[test]
    fn backend_command_spans_count_once_unless_an_invoke_made_them() {
        let call: TauriIpcCall = serde_json::from_value(json!({
            "sessionId": "s", "timestampUnixNanos": 1, "durationMs": null,
            "command": "load", "status": "OK", "errorMessage": null,
            "traceId": "t1", "spanId": "f1", "windowLabel": null, "argsJson": null,
            "argsRedacted": false, "resultSummary": null,
        }))
        .unwrap();
        let span = |command: &str, parent: Option<&str>, source: &str, status: Option<&str>| {
            serde_json::from_value::<SpanRecord>(json!({
                "sessionId": "s", "traceId": "t1", "spanId": "b", "parentSpanId": parent,
                "name": command, "kind": null, "startTimeUnixNanos": 1, "endTimeUnixNanos": 2,
                "statusCode": status, "statusMessage": null, "scopeName": null,
                "scopeVersion": null, "attributes": { "tauri.command": command },
                "source": source,
            }))
            .unwrap()
        };
        let spans = vec![
            // Continues the recorded invoke: already counted by the call.
            span("load", Some("f1"), "backend", Some("OK")),
            // Invoked without the instrumented `invoke`.
            span("save_chat_session", None, "backend", Some("ERROR")),
            // The frontend's own invoke span.
            span("load", None, "frontend", Some("OK")),
            // Propagation off: pairs with the `save` call nothing continued.
            span("save", None, "backend", Some("OK")),
            // Still running when the test ended (AR-36): exercised, not failed.
            {
                let mut open = span("agent_chat", None, "backend", None);
                open.end_time_unix_nanos = None;
                open
            },
        ];
        let save: TauriIpcCall = serde_json::from_value(json!({
            "sessionId": "s", "timestampUnixNanos": 1, "durationMs": null,
            "command": "save", "status": "OK", "errorMessage": null,
            "traceId": "t2", "spanId": "f2", "windowLabel": null, "argsJson": null,
            "argsRedacted": false, "resultSummary": null,
        }))
        .unwrap();

        let recorded = [call, save];
        let calls: Vec<_> = command_calls(&recorded, &spans).collect();

        assert_eq!(
            calls,
            vec![
                ("load", false),
                ("save", false),
                ("save_chat_session", true),
                ("agent_chat", false)
            ]
        );
    }

    #[test]
    fn unreadable_telemetry_is_reported_per_test() {
        let root = tempfile::tempdir().unwrap();
        attempt_json(root.path(), "a/attempt-0", Some(json!({ "nope": true })));
        fs::write(root.path().join("a/attempt-0/telemetry.json"), "not json").unwrap();
        attempt_json(root.path(), "b/attempt-0", None);
        let report = report(vec![
            test("a", "passed", vec![attempt("a/attempt-0", "passed")]),
            test("b", "passed", vec![attempt("b/attempt-0", "passed")]),
            test("c", "running", vec![attempt("c/attempt-0", "running")]),
        ]);

        let coverage = build(root.path(), &report, &Seen::default());

        assert_eq!(coverage.tests[0].telemetry, "unreadable");
        assert!(coverage.tests[0]
            .error
            .as_deref()
            .unwrap()
            .contains("not attempt telemetry"));
        assert_eq!(coverage.tests[1].telemetry, "none");
        assert_eq!(coverage.tests[2].telemetry, "none");
        assert!(coverage.tests[2].error.is_none());
        assert_eq!(coverage.totals.tests_with_telemetry, 0);
    }

    #[test]
    fn telemetry_outside_the_run_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("a/attempt-0/attempt.json"),
            &json!({ "schemaVersion": 1, "testId": "x", "title": "x", "attempt": 0,
                     "status": "passed", "startedAt": "t", "durationMs": 1, "error": null,
                     "steps": [], "attachments": [{ "name": "telemetry",
                     "contentType": "application/json", "path": "../escape.json" }] }),
        );
        let report = report(vec![test(
            "a",
            "passed",
            vec![attempt("a/attempt-0", "passed")],
        )]);

        let coverage = build(root.path(), &report, &Seen::default());

        assert_eq!(coverage.tests[0].telemetry, "unreadable");
        assert!(coverage.tests[0]
            .error
            .as_deref()
            .unwrap()
            .contains("not inside the run"));
    }

    #[test]
    fn a_retry_with_unreadable_telemetry_marks_the_test_partial() {
        let root = tempfile::tempdir().unwrap();
        attempt_json(
            root.path(),
            "a/attempt-0",
            Some(telemetry(&[("save", "OK")], &[])),
        );
        attempt_json(root.path(), "a/attempt-1", Some(json!({})));
        fs::write(root.path().join("a/attempt-1/telemetry.json"), "not json").unwrap();
        let report = report(vec![test(
            "a",
            "passed",
            vec![
                attempt("a/attempt-0", "failed"),
                attempt("a/attempt-1", "passed"),
            ],
        )]);

        let coverage = build(root.path(), &report, &Seen::default());

        assert_eq!(coverage.tests[0].telemetry, "partial");
        assert!(coverage.tests[0].error.is_some());
        assert_eq!(coverage.tests[0].commands, vec!["save"]);
        assert_eq!(coverage.totals.tests_with_telemetry, 0);
    }

    fn link_dir(target: &Path, link: &Path) {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn telemetry_behind_a_link_out_of_the_run_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(
            &outside.path().join("telemetry.json"),
            &telemetry(&[("secret", "OK")], &[]),
        );
        write(
            &root.path().join("a/attempt-0/attempt.json"),
            &json!({ "schemaVersion": 1, "testId": "x", "title": "x", "attempt": 0,
                     "status": "passed", "startedAt": "t", "durationMs": 1, "error": null,
                     "steps": [], "attachments": [{ "name": "telemetry",
                     "contentType": "application/json", "path": "a/attempt-0/out/telemetry.json" }] }),
        );
        link_dir(
            outside.path(),
            &root.path().join("a").join("attempt-0").join("out"),
        );
        let report = report(vec![test(
            "a",
            "passed",
            vec![attempt("a/attempt-0", "passed")],
        )]);

        let coverage = build(root.path(), &report, &Seen::default());

        assert_eq!(coverage.tests[0].telemetry, "unreadable");
        assert!(coverage.commands.is_empty());
        assert!(coverage.tests[0]
            .error
            .as_deref()
            .unwrap()
            .contains("resolves outside the run"));
    }

    #[test]
    fn registered_commands_set_a_command_baseline_only() {
        let root = tempfile::tempdir().unwrap();
        attempt_json(
            root.path(),
            "a/attempt-0",
            Some(telemetry(&[("save", "OK"), ("plugin:x|y", "OK")], &[])),
        );
        let report = report(vec![test(
            "a",
            "passed",
            vec![attempt("a/attempt-0", "passed")],
        )]);
        let mut seen = Seen::default();
        seen.registered = ["save".to_string(), "delete".to_string()].into();

        let mut coverage = build(root.path(), &report, &seen);
        settle_baseline(&mut coverage);
        assert_eq!(coverage.baseline, Baseline::default());
        assert!(coverage.unexercised.commands.is_empty());

        coverage.surface.push(SurfaceSummary {
            path: "src-tauri/src".into(),
            commands: 2,
            error: None,
        });
        settle_baseline(&mut coverage);
        assert_eq!(
            coverage.baseline,
            Baseline {
                commands: true,
                events: false
            }
        );
        assert_eq!(coverage.unexercised.commands, ["delete"]);
        assert_eq!(
            (coverage.totals.commands_exercised, coverage.totals.commands),
            (2, 3)
        );
        let registered: Vec<_> = coverage
            .commands
            .iter()
            .map(|c| (c.command.as_str(), c.registered))
            .collect();
        assert_eq!(
            registered,
            [("delete", true), ("plugin:x|y", false), ("save", true)]
        );
    }

    #[test]
    fn a_missing_store_is_reported_not_fatal() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.sqlite");
        let (seen, sources) = read_seen(std::slice::from_ref(&missing));
        assert!(seen.commands.is_empty());
        assert_eq!(sources.len(), 1);
        assert!(sources[0]
            .error
            .as_deref()
            .unwrap()
            .contains("no telemetry store"));
        assert!(!missing.exists());
    }
}
