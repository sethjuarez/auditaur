//! Judges telemetry assertions against what the app recorded since the test started.
//!
//! Telemetry only accumulates, so "too many" is final while "not yet" is worth retrying.
//! As in Playwright, an assertion passes the moment it holds: `not`, `count`, and
//! `expectNoErrors` check what has been recorded (after a flush), not what may come later.
//! Events count frontend emits when there are any, otherwise receives (one per listener),
//! since the two cannot be correlated.

use std::collections::BTreeMap;

use auditaur_core::{
    model::{LogRecord, SpanRecord},
    storage::RelatedTelemetry,
};
use serde_json::Value;

use crate::commands::drills::model::{
    name_label, name_matches, AttributeMatch, IpcStatus, Needle, SpanStatus, TelemetryExpectation,
    TelemetryKind,
};

/// How many records a failure message lists.
const SHOWN: usize = 5;
const CLIP: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgement {
    Pass,
    /// Not yet; more telemetry may arrive.
    Pending,
    /// More telemetry cannot fix this.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub judgement: Judgement,
    pub expected: String,
    pub actual: String,
}

/// Drop the drive bridge's own logs; they describe the test, not the app.
pub fn scrub(mut telemetry: RelatedTelemetry) -> RelatedTelemetry {
    telemetry.logs.retain(|log| !is_harness(log));
    telemetry
}

fn is_harness(log: &LogRecord) -> bool {
    log.attributes.get("auditaur.driver.backend").is_some()
        || log
            .body
            .as_deref()
            .is_some_and(|body| body.starts_with("Auditaur drive bridge"))
}

pub fn judge(expectation: &TelemetryExpectation, telemetry: &RelatedTelemetry) -> Verdict {
    match expectation {
        TelemetryExpectation::Ipc {
            command,
            status,
            count,
            max_ms,
            not,
            ..
        } => {
            let matching: Vec<_> = telemetry
                .tauri_ipc_calls
                .iter()
                .filter(|call| name_matches(command, &call.command))
                .filter(|call| status.is_none_or(|status| ipc_status(&call.status) == status))
                .collect();
            let command = name_label(command);
            let what = match status {
                Some(status) => format!(
                    "IPC call to `{command}` with status {}",
                    status_name(*status)
                ),
                None => format!("IPC call to `{command}`"),
            };
            let seen = || {
                listing(
                    telemetry
                        .tauri_ipc_calls
                        .iter()
                        .map(|call| match call.duration_ms {
                            Some(ms) => format!(
                                "{} ({}, {ms:.0}ms)",
                                call.command,
                                call.status.to_ascii_lowercase()
                            ),
                            None => {
                                format!("{} ({})", call.command, call.status.to_ascii_lowercase())
                            }
                        }),
                    "no IPC calls",
                )
            };
            let verdict = quantity(matching.len(), *count, *not, &what, seen);
            match (verdict.judgement, max_ms) {
                (Judgement::Pass, Some(max_ms)) if !not => {
                    let slowest = matching
                        .iter()
                        .filter_map(|call| call.duration_ms)
                        .fold(None, |max: Option<f64>, ms| {
                            Some(max.map_or(ms, |max| max.max(ms)))
                        });
                    match slowest {
                        Some(ms) if ms > *max_ms as f64 => Verdict {
                            judgement: Judgement::Failed,
                            expected: format!("every {what} within {max_ms}ms"),
                            actual: format!("slowest took {ms:.0}ms"),
                        },
                        _ => verdict,
                    }
                }
                _ => verdict,
            }
        }
        TelemetryExpectation::Event {
            name, count, not, ..
        } => {
            let named = telemetry
                .tauri_events
                .iter()
                .filter(|event| &event.event_name == name);
            let emits = named
                .clone()
                .filter(|event| event.direction == "emit")
                .count();
            // Backend events are only seen by frontend listeners.
            let found = if emits > 0 { emits } else { named.count() };
            let seen = || {
                let mut names = BTreeMap::<&str, usize>::new();
                for event in &telemetry.tauri_events {
                    *names.entry(&event.event_name).or_default() += 1;
                }
                listing(
                    names.into_iter().map(|(name, n)| format!("{name} ×{n}")),
                    "no events",
                )
            };
            quantity(found, *count, *not, &format!("event `{name}`"), seen)
        }
        TelemetryExpectation::Log {
            contains,
            level,
            attributes,
            count,
            not,
            ..
        } => {
            let entries = log_entries(telemetry);
            let level = level.as_deref().map(normalize_level);
            let found = entries
                .iter()
                .filter(|(entry_level, text, entry_attributes)| {
                    text.contains(contains.as_str())
                        && level.as_deref().is_none_or(|level| entry_level == level)
                        && attributes
                            .iter()
                            .all(|attribute| attribute.matches(entry_attributes))
                })
                .count();
            let what = match &level {
                Some(level) => format!("{level} log containing {contains:?}"),
                None => format!("log containing {contains:?}"),
            } + &with_attributes(attributes);
            let seen = || {
                listing(
                    entries
                        .iter()
                        .rev()
                        .map(|(level, text, _)| format!("[{level}] {}", clip(text))),
                    "no logs",
                )
            };
            quantity(found, *count, *not, &what, seen)
        }
        TelemetryExpectation::Span {
            name,
            status,
            attributes,
            count,
            not,
            ..
        } => {
            let found = telemetry
                .spans
                .iter()
                .filter(|span| name_matches(name, &span.name))
                .filter(|span| status.is_none_or(|status| span_status(span) == status))
                .filter(|span| {
                    attributes
                        .iter()
                        .all(|attribute| attribute.matches(&span.attributes))
                })
                .count();
            let status_text = status
                .map(|status| format!(" with status {}", status.as_str()))
                .unwrap_or_default();
            let what =
                format!("span `{}`{status_text}", name_label(name)) + &with_attributes(attributes);
            let seen = || {
                let mut names = BTreeMap::<&str, usize>::new();
                for span in &telemetry.spans {
                    *names.entry(&span.name).or_default() += 1;
                }
                listing(
                    names.into_iter().map(|(name, n)| format!("{name} ×{n}")),
                    "no spans",
                )
            };
            quantity(found, *count, *not, &what, seen)
        }
        TelemetryExpectation::NoTelemetry { needle, kinds, .. } => {
            let hits = telemetry_hits(telemetry, needle, kinds);
            let scope = kinds
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Verdict {
                judgement: if hits.is_empty() {
                    Judgement::Pass
                } else {
                    Judgement::Failed
                },
                expected: format!("no {scope} containing {needle}"),
                actual: listing(hits.into_iter(), "none"),
            }
        }
        TelemetryExpectation::NoErrors { ignore, .. } => {
            let errors: Vec<String> = errors(telemetry)
                .into_iter()
                .filter(|error| !ignore.iter().any(|ignore| error.contains(ignore.as_str())))
                .collect();
            Verdict {
                judgement: if errors.is_empty() {
                    Judgement::Pass
                } else {
                    Judgement::Failed
                },
                expected: "no errors".to_string(),
                actual: listing(errors.iter().map(|error| clip(error)), "no errors"),
            }
        }
    }
}

/// `not` means none; `count` means exactly that many; otherwise at least one.
fn quantity(
    found: usize,
    count: Option<u64>,
    not: bool,
    what: &str,
    seen: impl FnOnce() -> String,
) -> Verdict {
    let found = found as u64;
    let (expected, judgement) = match (not, count) {
        (true, _) | (false, Some(0)) => (
            format!("no {what}"),
            if found == 0 {
                Judgement::Pass
            } else {
                Judgement::Failed
            },
        ),
        (false, Some(count)) => (
            format!("exactly {count} × {what}"),
            match found.cmp(&count) {
                std::cmp::Ordering::Equal => Judgement::Pass,
                std::cmp::Ordering::Less => Judgement::Pending,
                std::cmp::Ordering::Greater => Judgement::Failed,
            },
        ),
        (false, None) => (
            format!("at least 1 × {what}"),
            if found > 0 {
                Judgement::Pass
            } else {
                Judgement::Pending
            },
        ),
    };
    let actual = if judgement == Judgement::Pass {
        format!("{found} matching")
    } else {
        format!("{found} matching; seen: {}", seen())
    };
    Verdict {
        judgement,
        expected,
        actual,
    }
}

/// Logs and frontend errors as `(level, text, attributes)`, oldest first.
fn log_entries(telemetry: &RelatedTelemetry) -> Vec<(String, String, &Value)> {
    let mut entries: Vec<(i64, String, String, &Value)> = telemetry
        .logs
        .iter()
        .map(|log| {
            (
                log.timestamp_unix_nanos,
                log_level(log),
                log_text(log),
                &log.attributes,
            )
        })
        .chain(telemetry.frontend_errors.iter().map(|error| {
            (
                error.timestamp_unix_nanos,
                "error".to_string(),
                error.message.clone(),
                &error.attributes,
            )
        }))
        .collect();
    entries.sort_by_key(|(at, ..)| *at);
    entries
        .into_iter()
        .map(|(_, level, text, attributes)| (level, text, attributes))
        .collect()
}

fn with_attributes(attributes: &[AttributeMatch]) -> String {
    if attributes.is_empty() {
        return String::new();
    }
    let list = attributes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(" with {list}")
}

fn span_status(span: &SpanRecord) -> SpanStatus {
    let code = span.status_code.as_deref().unwrap_or_default();
    let code = code.to_ascii_lowercase();
    match code.trim_start_matches("status_code_") {
        "ok" => SpanStatus::Ok,
        "error" => SpanStatus::Error,
        _ => SpanStatus::Unset,
    }
}

/// Where the needle appears, as `kind: name` labels. Every string in a record is searched,
/// including nested attributes and payloads; numbers and booleans as their JSON text.
fn telemetry_hits(
    telemetry: &RelatedTelemetry,
    needle: &Needle,
    kinds: &[TelemetryKind],
) -> Vec<String> {
    fn found<T: serde::Serialize>(needle: &Needle, record: &T) -> bool {
        // The record's own field names, IDs, timestamps, and durations are bookkeeping, not
        // app data; nested keys are data.
        match serde_json::to_value(record) {
            Ok(Value::Object(fields)) => fields
                .iter()
                .filter(|(key, _)| !is_bookkeeping(key))
                .any(|(_, value)| contains(needle, value)),
            Ok(value) => contains(needle, &value),
            Err(_) => false,
        }
    }
    fn is_bookkeeping(key: &str) -> bool {
        key.ends_with("Id") || key.ends_with("Nanos") || key == "durationMs"
    }
    fn contains(needle: &Needle, value: &Value) -> bool {
        match value {
            Value::String(text) => needle.is_in(text),
            Value::Array(items) => items.iter().any(|item| contains(needle, item)),
            Value::Object(map) => map
                .iter()
                .any(|(key, value)| needle.is_in(key) || contains(needle, value)),
            Value::Number(number) => needle.is_in(&number.to_string()),
            Value::Bool(_) | Value::Null => false,
        }
    }
    let mut hits = Vec::new();
    for kind in kinds {
        match kind {
            TelemetryKind::Logs => {
                for log in &telemetry.logs {
                    if found(needle, log) {
                        hits.push(format!("log: {}", clip(&log_text(log))));
                    }
                }
                for error in &telemetry.frontend_errors {
                    if found(needle, error) {
                        hits.push(format!("frontend error: {}", clip(&error.message)));
                    }
                }
            }
            TelemetryKind::Spans => {
                for span in &telemetry.spans {
                    if found(needle, span) {
                        hits.push(format!("span: {}", span.name));
                    }
                }
                for event in &telemetry.span_events {
                    if found(needle, event) {
                        hits.push(format!("span event: {}", event.name));
                    }
                }
            }
            TelemetryKind::Ipc => {
                for call in &telemetry.tauri_ipc_calls {
                    if found(needle, call) {
                        hits.push(format!("IPC: {}", call.command));
                    }
                }
            }
            TelemetryKind::Events => {
                for event in &telemetry.tauri_events {
                    if found(needle, event) {
                        hits.push(format!("event: {}", event.event_name));
                    }
                }
            }
        }
    }
    hits
}

/// Every error the app reported: frontend errors, error logs, and failed IPC calls.
fn errors(telemetry: &RelatedTelemetry) -> Vec<String> {
    let mut errors = Vec::new();
    for error in &telemetry.frontend_errors {
        errors.push((
            error.timestamp_unix_nanos,
            format!("frontend error: {}", error.message),
        ));
    }
    for log in &telemetry.logs {
        if is_error_level(&log_level(log)) {
            errors.push((
                log.timestamp_unix_nanos,
                format!("{} log: {}", log_level(log), log_text(log)),
            ));
        }
    }
    for call in &telemetry.tauri_ipc_calls {
        if ipc_status(&call.status) == IpcStatus::Error {
            let message = call.error_message.as_deref().unwrap_or("no message");
            errors.push((
                call.timestamp_unix_nanos,
                format!("IPC `{}` failed: {message}", call.command),
            ));
        }
    }
    errors.sort_by_key(|(at, _)| *at);
    errors.into_iter().map(|(_, text)| text).collect()
}

fn ipc_status(status: &str) -> IpcStatus {
    if status.eq_ignore_ascii_case("ok") {
        IpcStatus::Ok
    } else {
        IpcStatus::Error
    }
}

fn status_name(status: IpcStatus) -> &'static str {
    match status {
        IpcStatus::Ok => "ok",
        IpcStatus::Error => "error",
    }
}

/// OpenTelemetry severity, as one of trace, debug, info, warn, error, fatal.
fn log_level(log: &LogRecord) -> String {
    if let Some(text) = log.severity_text.as_deref().filter(|text| !text.is_empty()) {
        return normalize_level(text);
    }
    match log.severity_number.unwrap_or(9) {
        ..=4 => "trace",
        5..=8 => "debug",
        9..=12 => "info",
        13..=16 => "warn",
        17..=20 => "error",
        _ => "fatal",
    }
    .to_string()
}

fn normalize_level(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "warning" => "warn".to_string(),
        "err" => "error".to_string(),
        "critical" => "fatal".to_string(),
        other => other.to_string(),
    }
}

fn is_error_level(level: &str) -> bool {
    matches!(level, "error" | "fatal")
}

fn log_text(log: &LogRecord) -> String {
    match (&log.body, &log.body_json) {
        (Some(body), _) => body.clone(),
        (None, Some(json)) => json.to_string(),
        (None, None) => String::new(),
    }
}

fn listing(items: impl Iterator<Item = String>, empty: &str) -> String {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        return empty.to_string();
    }
    let mut shown = items
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > SHOWN {
        shown.push_str(&format!(" (and {} more)", items.len() - SHOWN));
    }
    shown
}

fn clip(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= CLIP {
        text
    } else {
        format!("{}…", text.chars().take(CLIP).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::drills::model::{AttributeValue, TextMatch};
    use auditaur_core::model::{FrontendError, TauriEventRecord, TauriIpcCall, TelemetrySource};
    use regex::Regex;
    use serde_json::json;

    fn span(name: &str, status: Option<&str>, attributes: Value) -> SpanRecord {
        SpanRecord {
            session_id: "s".into(),
            trace_id: "t".into(),
            span_id: "x".into(),
            parent_span_id: None,
            name: name.into(),
            kind: None,
            start_time_unix_nanos: 1,
            end_time_unix_nanos: Some(2),
            status_code: status.map(str::to_string),
            status_message: None,
            scope_name: None,
            scope_version: None,
            attributes,
            source: TelemetrySource::Backend,
        }
    }

    fn attribute(key: &str, value: AttributeValue) -> AttributeMatch {
        AttributeMatch {
            key: key.into(),
            value,
        }
    }

    #[test]
    fn spans_match_name_status_and_attributes() {
        let telemetry = RelatedTelemetry {
            spans: vec![
                span(
                    "chat.send",
                    Some("STATUS_CODE_OK"),
                    json!({ "harness": "prompty", "turns": 2 }),
                ),
                span("chat.send", Some("Error"), json!({ "harness": "other" })),
                span("startup", None, json!({})),
            ],
            ..RelatedTelemetry::default()
        };
        let expect = |name: TextMatch, status, attributes, count| TelemetryExpectation::Span {
            name,
            status,
            attributes,
            count,
            not: false,
            soft: false,
        };
        let exact = || TextMatch::Exact("chat.send".into());
        assert_eq!(
            judge(&expect(exact(), None, vec![], Some(2)), &telemetry).judgement,
            Judgement::Pass
        );
        let ok = expect(
            exact(),
            Some(SpanStatus::Ok),
            vec![
                attribute("harness", AttributeValue::Equals(json!("prompty"))),
                attribute("turns", AttributeValue::Equals(json!(2.0))),
            ],
            Some(1),
        );
        assert_eq!(judge(&ok, &telemetry).judgement, Judgement::Pass);
        let unset = expect(
            TextMatch::Pattern(Regex::new("^start").unwrap()),
            Some(SpanStatus::Unset),
            vec![],
            None,
        );
        assert_eq!(judge(&unset, &telemetry).judgement, Judgement::Pass);
        let missing = judge(
            &expect(
                exact(),
                None,
                vec![attribute(
                    "harness",
                    AttributeValue::Pattern(Regex::new("^none$").unwrap()),
                )],
                None,
            ),
            &telemetry,
        );
        assert_eq!(missing.judgement, Judgement::Pending);
        assert_eq!(missing.actual, "0 matching; seen: chat.send ×2, startup ×1");
    }

    #[test]
    fn logs_match_attributes_and_ipc_matches_patterns() {
        let mut tagged = log("INFO", "sent", 1);
        tagged.attributes = json!({ "harness": "prompty" });
        let telemetry = RelatedTelemetry {
            logs: vec![tagged, log("INFO", "sent", 2)],
            tauri_ipc_calls: vec![call("plugin:chat|send", "OK", 1.0)],
            ..RelatedTelemetry::default()
        };
        let expect = |attributes| TelemetryExpectation::Log {
            contains: "sent".into(),
            level: None,
            attributes,
            count: Some(1),
            not: false,
            soft: false,
        };
        let harness = vec![attribute(
            "harness",
            AttributeValue::Equals(json!("prompty")),
        )];
        assert_eq!(
            judge(&expect(harness), &telemetry).judgement,
            Judgement::Pass
        );
        let pattern = TelemetryExpectation::Ipc {
            command: TextMatch::Pattern(Regex::new("^plugin:chat\\|").unwrap()),
            status: None,
            count: None,
            max_ms: None,
            not: false,
            soft: false,
        };
        assert_eq!(judge(&pattern, &telemetry).judgement, Judgement::Pass);
    }

    #[test]
    fn no_telemetry_searches_selected_kinds_but_not_bookkeeping() {
        let mut leaky = call("save", "OK", 1.0);
        leaky.args_json = Some(r#"{"author":"ada@example.com"}"#.into());
        let mut nested = log("INFO", "fine", 1);
        nested.attributes = json!({ "user": { "ada@example.com": true } });
        let telemetry = RelatedTelemetry {
            logs: vec![nested],
            tauri_ipc_calls: vec![leaky],
            ..RelatedTelemetry::default()
        };
        let expect = |needle, kinds: &[TelemetryKind]| TelemetryExpectation::NoTelemetry {
            needle,
            kinds: kinds.to_vec(),
            soft: false,
        };
        let email = || Needle::Contains("ada@example.com".into());
        let all = judge(&expect(email(), &TelemetryKind::ALL), &telemetry);
        assert_eq!(all.judgement, Judgement::Failed);
        assert_eq!(all.actual, "log: fine, IPC: save");
        assert_eq!(
            judge(
                &expect(email(), &[TelemetryKind::Spans, TelemetryKind::Events]),
                &telemetry
            )
            .judgement,
            Judgement::Pass
        );
        // Session IDs, timestamps, and durations are not app data.
        let digits = Needle::Pattern(Regex::new("^1$").unwrap());
        assert_eq!(
            judge(&expect(digits, &[TelemetryKind::Ipc]), &telemetry).judgement,
            Judgement::Pass
        );
    }

    fn call(command: &str, status: &str, ms: f64) -> TauriIpcCall {
        TauriIpcCall {
            session_id: "s".into(),
            timestamp_unix_nanos: 1,
            duration_ms: Some(ms),
            command: command.into(),
            status: status.into(),
            error_message: (status == "ERROR").then(|| "boom".to_string()),
            trace_id: None,
            span_id: None,
            window_label: None,
            args_json: None,
            args_redacted: false,
            result_summary: None,
        }
    }

    fn event(name: &str, direction: &str) -> TauriEventRecord {
        TauriEventRecord {
            session_id: "s".into(),
            timestamp_unix_nanos: 1,
            event_name: name.into(),
            direction: direction.into(),
            target: None,
            trace_id: None,
            span_id: None,
            window_label: None,
            payload_summary: None,
            payload_json: None,
            payload_redacted: false,
        }
    }

    fn log(level: &str, body: &str, at: i64) -> LogRecord {
        LogRecord {
            session_id: "s".into(),
            timestamp_unix_nanos: at,
            observed_timestamp_unix_nanos: None,
            severity_text: Some(level.into()),
            severity_number: None,
            body: Some(body.into()),
            body_json: None,
            trace_id: None,
            span_id: None,
            scope_name: None,
            scope_version: None,
            attributes: json!({}),
            source: TelemetrySource::Frontend,
        }
    }

    fn frontend_error(message: &str, at: i64) -> FrontendError {
        FrontendError {
            session_id: "s".into(),
            timestamp_unix_nanos: at,
            message: message.into(),
            stack: None,
            filename: None,
            line_number: None,
            column_number: None,
            error_type: None,
            trace_id: None,
            span_id: None,
            window_label: None,
            attributes: json!({}),
        }
    }

    fn ipc(
        command: &str,
        status: Option<IpcStatus>,
        count: Option<u64>,
        max_ms: Option<u64>,
        not: bool,
    ) -> TelemetryExpectation {
        TelemetryExpectation::Ipc {
            command: TextMatch::Exact(command.into()),
            status,
            count,
            max_ms,
            not,
            soft: false,
        }
    }

    fn with_calls(calls: Vec<TauriIpcCall>) -> RelatedTelemetry {
        RelatedTelemetry {
            tauri_ipc_calls: calls,
            ..RelatedTelemetry::default()
        }
    }

    #[test]
    fn ipc_counts_are_monotonic() {
        let empty = with_calls(vec![]);
        let one = with_calls(vec![call("greet", "OK", 5.0)]);
        let two = with_calls(vec![call("greet", "OK", 5.0), call("greet", "OK", 5.0)]);
        let any = ipc("greet", None, None, None, false);
        assert_eq!(judge(&any, &empty).judgement, Judgement::Pending);
        assert_eq!(judge(&any, &one).judgement, Judgement::Pass);

        let exactly_one = ipc("greet", None, Some(1), None, false);
        assert_eq!(judge(&exactly_one, &empty).judgement, Judgement::Pending);
        assert_eq!(judge(&exactly_one, &one).judgement, Judgement::Pass);
        let too_many = judge(&exactly_one, &two);
        assert_eq!(too_many.judgement, Judgement::Failed);
        assert_eq!(too_many.expected, "exactly 1 × IPC call to `greet`");
        assert_eq!(
            too_many.actual,
            "2 matching; seen: greet (ok, 5ms), greet (ok, 5ms)"
        );

        let none = ipc("greet", None, None, None, true);
        assert_eq!(judge(&none, &empty).judgement, Judgement::Pass);
        assert_eq!(judge(&none, &one).judgement, Judgement::Failed);
        let zero = ipc("greet", None, Some(0), None, false);
        assert_eq!(judge(&zero, &one).judgement, Judgement::Failed);
    }

    #[test]
    fn ipc_filters_status_and_budgets_duration() {
        let telemetry = with_calls(vec![call("save", "ERROR", 3.0), call("other", "OK", 1.0)]);
        let ok = judge(
            &ipc("save", Some(IpcStatus::Ok), None, None, false),
            &telemetry,
        );
        assert_eq!(ok.judgement, Judgement::Pending);
        assert_eq!(
            ok.expected,
            "at least 1 × IPC call to `save` with status ok"
        );
        assert_eq!(
            ok.actual,
            "0 matching; seen: save (error, 3ms), other (ok, 1ms)"
        );
        let error = judge(
            &ipc("save", Some(IpcStatus::Error), None, None, false),
            &telemetry,
        );
        assert_eq!(error.judgement, Judgement::Pass);

        let slow = with_calls(vec![call("save", "OK", 40.0), call("save", "OK", 250.4)]);
        let budget = judge(&ipc("save", None, None, Some(200), false), &slow);
        assert_eq!(budget.judgement, Judgement::Failed);
        assert_eq!(budget.expected, "every IPC call to `save` within 200ms");
        assert_eq!(budget.actual, "slowest took 250ms");
        let fast = judge(&ipc("save", None, None, Some(300), false), &slow);
        assert_eq!(fast.judgement, Judgement::Pass);
    }

    #[test]
    fn events_count_emits_before_receives() {
        let expect = |count| TelemetryExpectation::Event {
            name: "ping".into(),
            count,
            not: false,
            soft: false,
        };
        // A frontend emit that the frontend also listens to is one event.
        let both = RelatedTelemetry {
            tauri_events: vec![event("ping", "emit"), event("ping", "receive")],
            ..RelatedTelemetry::default()
        };
        assert_eq!(judge(&expect(Some(1)), &both).judgement, Judgement::Pass);
        // A backend event is only seen by listeners.
        let received = RelatedTelemetry {
            tauri_events: vec![event("ping", "receive"), event("other", "emit")],
            ..RelatedTelemetry::default()
        };
        assert_eq!(
            judge(&expect(Some(1)), &received).judgement,
            Judgement::Pass
        );
        let missing = judge(
            &TelemetryExpectation::Event {
                name: "pong".into(),
                count: None,
                not: false,
                soft: false,
            },
            &received,
        );
        assert_eq!(missing.judgement, Judgement::Pending);
        assert_eq!(missing.actual, "0 matching; seen: other ×1, ping ×1");
    }

    #[test]
    fn logs_include_frontend_errors_and_match_level() {
        let telemetry = RelatedTelemetry {
            logs: vec![
                log("INFO", "saved draft", 1),
                log("WARNING", "slow disk", 2),
            ],
            frontend_errors: vec![frontend_error("Intentional failure", 3)],
            ..RelatedTelemetry::default()
        };
        let expect = |contains: &str, level: Option<&str>| TelemetryExpectation::Log {
            contains: contains.into(),
            level: level.map(str::to_string),
            attributes: vec![],
            count: None,
            not: false,
            soft: false,
        };
        assert_eq!(
            judge(&expect("saved", None), &telemetry).judgement,
            Judgement::Pass
        );
        assert_eq!(
            judge(&expect("saved", Some("error")), &telemetry).judgement,
            Judgement::Pending
        );
        assert_eq!(
            judge(&expect("slow", Some("warn")), &telemetry).judgement,
            Judgement::Pass
        );
        assert_eq!(
            judge(&expect("slow", Some("warning")), &telemetry).judgement,
            Judgement::Pass
        );
        assert_eq!(
            judge(&expect("Intentional", Some("error")), &telemetry).judgement,
            Judgement::Pass
        );
        let missing = judge(&expect("nothing", None), &telemetry);
        assert_eq!(
            missing.actual,
            "0 matching; seen: [error] Intentional failure, [warn] slow disk, [info] saved draft"
        );
    }

    #[test]
    fn no_errors_collects_every_error_kind_and_honors_ignore() {
        let mut quiet_numbered = log("", "fine", 1);
        quiet_numbered.severity_text = None;
        quiet_numbered.severity_number = Some(9);
        let mut loud_numbered = log("", "kaboom", 2);
        loud_numbered.severity_text = None;
        loud_numbered.severity_number = Some(18);
        let telemetry = RelatedTelemetry {
            logs: vec![quiet_numbered, loud_numbered, log("warn", "careful", 3)],
            frontend_errors: vec![frontend_error("ResizeObserver loop", 4)],
            tauri_ipc_calls: vec![call("save", "ERROR", 1.0)],
            ..RelatedTelemetry::default()
        };
        let expect = |ignore: &[&str]| TelemetryExpectation::NoErrors {
            ignore: ignore.iter().map(|s| s.to_string()).collect(),
            soft: false,
        };
        let verdict = judge(&expect(&[]), &telemetry);
        assert_eq!(verdict.judgement, Judgement::Failed);
        assert_eq!(
            verdict.actual,
            "IPC `save` failed: boom, error log: kaboom, frontend error: ResizeObserver loop"
        );
        let ignored = judge(&expect(&["ResizeObserver", "boom", "kaboom"]), &telemetry);
        assert_eq!(ignored.judgement, Judgement::Pass);
        assert_eq!(
            judge(&expect(&[]), &RelatedTelemetry::default()).judgement,
            Judgement::Pass
        );
    }

    #[test]
    fn scrub_drops_drive_bridge_logs() {
        let mut driver = log("ERROR", "auditaur drive inspect", 1);
        driver.attributes = json!({ "auditaur.driver.backend": "bridge" });
        let telemetry = scrub(RelatedTelemetry {
            logs: vec![
                driver,
                log("DEBUG", "Auditaur drive bridge received request", 2),
                log("ERROR", "real", 3),
            ],
            ..RelatedTelemetry::default()
        });
        assert_eq!(telemetry.logs.len(), 1);
        assert_eq!(telemetry.logs[0].body.as_deref(), Some("real"));
    }
}
