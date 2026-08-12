use anyhow::Result;
use auditaur_core::{
    model::{FrontendError, TauriIpcCall},
    storage::{FrontendErrorQuery, TauriIpcQuery},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use crate::{commands::read, discovery, output::table_cell};

pub struct ExceptionOptions {
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub since: Option<String>,
    pub fingerprint: Option<String>,
    pub json: bool,
    pub markdown: bool,
    pub output: Option<PathBuf>,
    pub limit: usize,
}

pub fn run(
    db: &Option<PathBuf>,
    session_file: &Option<PathBuf>,
    mut options: ExceptionOptions,
) -> Result<()> {
    let (db, session_id) =
        read::resolve_read_selectors(db, session_file, options.session_id.take())?;
    options.session_id = session_id;
    let db = discovery::resolve_db(db)?;
    let store = read::open_validated_store(&db)?;
    let mut frontend_errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id: options.session_id.clone(),
        trace_id: options.trace_id.clone(),
        limit: Some(usize::MAX),
    })?;
    let mut ipc_calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id: options.session_id,
        trace_id: options.trace_id,
        limit: Some(usize::MAX),
    })?;

    if let Some(cutoff) = read::parse_since_cutoff(options.since.as_deref())? {
        frontend_errors.retain(|error| error.timestamp_unix_nanos >= cutoff);
        ipc_calls.retain(|call| call.timestamp_unix_nanos >= cutoff);
    }
    ipc_calls.retain(read::is_failed_ipc);

    let mut reports = exception_reports(exception_candidates(frontend_errors, ipc_calls));
    if let Some(fingerprint) = &options.fingerprint {
        reports.retain(|report| report.fingerprint == *fingerprint);
    }
    reports.truncate(options.limit);

    if options.markdown {
        let markdown = render_markdown(&reports);
        if let Some(output) = options.output {
            fs::write(output, markdown)?;
        } else {
            print!("{markdown}");
        }
        return Ok(());
    }
    if let Some(output) = options.output {
        fs::write(output, serde_json::to_string_pretty(&reports)?)?;
        return Ok(());
    }
    read::print_json_or_table(options.json, &reports, || print_table(&reports))
}

fn exception_candidates(
    frontend_errors: Vec<FrontendError>,
    ipc_calls: Vec<TauriIpcCall>,
) -> Vec<ExceptionCandidate> {
    frontend_errors
        .into_iter()
        .map(ExceptionCandidate::from_frontend_error)
        .chain(ipc_calls.into_iter().map(ExceptionCandidate::from_ipc_call))
        .collect()
}

fn exception_reports(candidates: Vec<ExceptionCandidate>) -> Vec<ExceptionReport> {
    let mut groups: BTreeMap<String, ExceptionGroup> = BTreeMap::new();
    for candidate in candidates {
        let fingerprint = exception_fingerprint(&candidate);
        groups.entry(fingerprint).or_default().push(candidate);
    }

    let mut reports: Vec<_> = groups
        .into_iter()
        .map(|(fingerprint, group)| group.into_report(fingerprint))
        .collect();
    reports.sort_by(|left, right| {
        right
            .last_seen_unix_nanos
            .cmp(&left.last_seen_unix_nanos)
            .then_with(|| right.count.cmp(&left.count))
    });
    reports
}

fn exception_fingerprint(candidate: &ExceptionCandidate) -> String {
    let mut hasher = StableHasher::default();
    candidate.source.hash(&mut hasher);
    normalized(Some(&candidate.kind)).hash(&mut hasher);
    if candidate.source != ExceptionSource::FailedIpc {
        normalized_for_grouping(Some(&candidate.message)).hash(&mut hasher);
    }
    normalized(candidate.grouping_key.as_deref()).hash(&mut hasher);
    format!("ex_{:016x}", hasher.finish())
}

fn normalized(value: Option<&str>) -> String {
    value
        .unwrap_or("")
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_for_grouping(value: Option<&str>) -> String {
    let normalized = normalized(value);
    let mut output = String::with_capacity(normalized.len());
    let mut previous_was_placeholder = false;
    for part in normalized.split(' ') {
        let scrubbed = if is_dynamic_token(part) {
            "<var>"
        } else {
            part
        };
        if !output.is_empty() {
            output.push(' ');
        }
        if scrubbed == "<var>" && previous_was_placeholder {
            continue;
        }
        output.push_str(scrubbed);
        previous_was_placeholder = scrubbed == "<var>";
    }
    output
}

fn is_dynamic_token(value: &str) -> bool {
    let trimmed = value.trim_matches(|char: char| {
        matches!(
            char,
            ',' | '.' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\''
        )
    });
    trimmed.chars().any(|char| char.is_ascii_digit())
        && trimmed
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
}

fn top_stack_frame(stack: Option<&str>) -> Option<&str> {
    stack?
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("at ") || line.contains('@'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExceptionSource {
    FrontendError,
    RustPanic,
    FailedIpc,
}

impl ExceptionSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::FrontendError => "frontend_error",
            Self::RustPanic => "rust_panic",
            Self::FailedIpc => "failed_ipc",
        }
    }
}

#[derive(Debug, Clone)]
struct ExceptionCandidate {
    source: ExceptionSource,
    session_id: String,
    timestamp_unix_nanos: i64,
    message: String,
    kind: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    window_label: Option<String>,
    location: Option<String>,
    stack: Option<String>,
    grouping_key: Option<String>,
    context: BTreeMap<String, String>,
}

impl ExceptionCandidate {
    fn from_frontend_error(error: FrontendError) -> Self {
        let source = if error
            .attributes
            .get("auditaur.source")
            .and_then(serde_json::Value::as_str)
            == Some("panic_hook")
        {
            ExceptionSource::RustPanic
        } else {
            ExceptionSource::FrontendError
        };
        let location = error_location(&error);
        let grouping_key = top_stack_frame(error.stack.as_deref())
            .map(str::to_string)
            .or_else(|| location.clone());
        let mut context = BTreeMap::new();
        if let Some(filename) = &error.filename {
            context.insert("filename".to_string(), filename.clone());
        }
        if let Some(line) = error.line_number {
            context.insert("line".to_string(), line.to_string());
        }
        if let Some(column) = error.column_number {
            context.insert("column".to_string(), column.to_string());
        }
        Self {
            source,
            session_id: error.session_id,
            timestamp_unix_nanos: error.timestamp_unix_nanos,
            message: error.message,
            kind: error.error_type.unwrap_or_else(|| "Error".to_string()),
            trace_id: error.trace_id,
            span_id: error.span_id,
            window_label: error.window_label,
            location,
            stack: error.stack,
            grouping_key,
            context,
        }
    }

    fn from_ipc_call(call: TauriIpcCall) -> Self {
        let mut context = BTreeMap::new();
        context.insert("command".to_string(), call.command.clone());
        context.insert("status".to_string(), call.status.clone());
        if let Some(duration_ms) = call.duration_ms {
            context.insert("durationMs".to_string(), duration_ms.to_string());
        }
        let message = call.error_message.clone().unwrap_or_else(|| {
            format!(
                "Tauri IPC command `{}` failed with status {}",
                call.command, call.status
            )
        });
        Self {
            source: ExceptionSource::FailedIpc,
            session_id: call.session_id,
            timestamp_unix_nanos: call.timestamp_unix_nanos,
            message,
            kind: format!("Tauri IPC {}", call.command),
            trace_id: call.trace_id,
            span_id: call.span_id,
            window_label: call.window_label,
            location: Some(format!("ipc:{}", call.command)),
            stack: None,
            grouping_key: Some(call.command),
            context,
        }
    }
}

#[derive(Default)]
struct ExceptionGroup {
    candidates: Vec<ExceptionCandidate>,
}

impl ExceptionGroup {
    fn push(&mut self, candidate: ExceptionCandidate) {
        self.candidates.push(candidate);
    }

    fn into_report(self, fingerprint: String) -> ExceptionReport {
        let first = self.candidates.first().expect("group is never empty");
        let mut first_seen = first.timestamp_unix_nanos;
        let mut last_seen = first.timestamp_unix_nanos;
        let mut sessions = BTreeSet::new();
        let mut traces = BTreeSet::new();
        let mut spans = BTreeSet::new();
        let mut windows = BTreeSet::new();
        let mut locations = BTreeSet::new();
        let mut sample_stack = None;
        let mut sample_context = BTreeMap::new();

        for candidate in &self.candidates {
            first_seen = first_seen.min(candidate.timestamp_unix_nanos);
            last_seen = last_seen.max(candidate.timestamp_unix_nanos);
            sessions.insert(candidate.session_id.clone());
            if let Some(trace_id) = &candidate.trace_id {
                traces.insert(trace_id.clone());
            }
            if let Some(span_id) = &candidate.span_id {
                spans.insert(span_id.clone());
            }
            if let Some(window_label) = &candidate.window_label {
                windows.insert(window_label.clone());
            }
            if let Some(location) = &candidate.location {
                locations.insert(location.clone());
            }
            if sample_stack.is_none() {
                sample_stack = candidate.stack.clone();
            }
            if sample_context.is_empty() {
                sample_context = candidate.context.clone();
            }
        }

        let report = ExceptionReport {
            fingerprint,
            source: first.source,
            message: first.message.clone(),
            kind: first.kind.clone(),
            count: self.candidates.len(),
            first_seen_unix_nanos: first_seen,
            last_seen_unix_nanos: last_seen,
            sessions: sessions.into_iter().collect(),
            traces: traces.into_iter().collect(),
            spans: spans.into_iter().collect(),
            windows: windows.into_iter().collect(),
            locations: locations.into_iter().collect(),
            sample_context,
            sample_stack,
            issue_title: String::new(),
            issue_body_markdown: String::new(),
        };
        report.with_issue_fields()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExceptionReport {
    fingerprint: String,
    source: ExceptionSource,
    message: String,
    kind: String,
    count: usize,
    first_seen_unix_nanos: i64,
    last_seen_unix_nanos: i64,
    sessions: Vec<String>,
    traces: Vec<String>,
    spans: Vec<String>,
    windows: Vec<String>,
    locations: Vec<String>,
    sample_context: BTreeMap<String, String>,
    sample_stack: Option<String>,
    issue_title: String,
    issue_body_markdown: String,
}

impl ExceptionReport {
    fn with_issue_fields(mut self) -> Self {
        self.issue_title = markdown_line(&format!("{}: {}", self.kind, self.message));
        self.issue_body_markdown = issue_body_markdown(&self);
        self
    }
}

fn issue_body_markdown(report: &ExceptionReport) -> String {
    let mut body = String::new();
    body.push_str("## Auditaur exception report\n\n");
    body.push_str(&format!("- Fingerprint: `{}`\n", report.fingerprint));
    body.push_str(&format!("- Source: `{}`\n", report.source.as_str()));
    body.push_str(&format!("- Count: {}\n", report.count));
    body.push_str(&format!(
        "- First seen: `{}`\n",
        report.first_seen_unix_nanos
    ));
    body.push_str(&format!("- Last seen: `{}`\n", report.last_seen_unix_nanos));
    body.push_str(&format!("- Kind: {}\n", markdown_line(&report.kind)));
    body.push_str(&format!("- Message: {}\n", markdown_line(&report.message)));
    if !report.locations.is_empty() {
        body.push_str(&format!("- Locations: {}\n", report.locations.join(", ")));
    }
    if !report.windows.is_empty() {
        body.push_str(&format!("- Windows: {}\n", report.windows.join(", ")));
    }
    if !report.traces.is_empty() {
        body.push_str(&format!("- Trace IDs: {}\n", report.traces.join(", ")));
    }
    if !report.sample_context.is_empty() {
        body.push_str("\n## Sample context\n\n");
        for (key, value) in &report.sample_context {
            body.push_str(&format!(
                "- `{}`: `{}`\n",
                markdown_line(key),
                markdown_line(value)
            ));
        }
    }
    body.push_str("\n## Privacy note\n\n");
    body.push_str("Review and redact this local report before posting it to a public tracker.\n");
    if let Some(stack) = &report.sample_stack {
        body.push_str("\n## Sample stack\n\n```text\n");
        body.push_str(&escape_code_fence(stack));
        body.push_str("\n```\n");
    }
    body
}

fn markdown_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn escape_code_fence(value: &str) -> String {
    value.replace("```", "`` `")
}

fn error_location(error: &FrontendError) -> Option<String> {
    let filename = error.filename.as_ref()?;
    Some(match (error.line_number, error.column_number) {
        (Some(line), Some(column)) => format!("{filename}:{line}:{column}"),
        (Some(line), None) => format!("{filename}:{line}"),
        _ => filename.clone(),
    })
}

fn print_table(reports: &[ExceptionReport]) -> Result<()> {
    println!("FINGERPRINT\tSOURCE\tCOUNT\tLAST_SEEN\tKIND\tMESSAGE");
    for report in reports {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            report.fingerprint,
            report.source.as_str(),
            report.count,
            report.last_seen_unix_nanos,
            table_cell(&report.kind, 60),
            table_cell(&report.message, 160)
        );
    }
    Ok(())
}

fn render_markdown(reports: &[ExceptionReport]) -> String {
    let mut output = String::new();
    for (index, report) in reports.iter().enumerate() {
        if index > 0 {
            output.push_str("\n---\n\n");
        }
        output.push_str(&format!("# {}\n\n", report.issue_title));
        output.push_str(&report.issue_body_markdown);
        if !output.ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

struct StableHasher(u64);

impl Default for StableHasher {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{exception_candidates, exception_fingerprint, exception_reports, ExceptionSource};
    use auditaur_core::model::{FrontendError, TauriIpcCall};
    use serde_json::json;

    #[test]
    fn groups_repeated_frontend_errors_into_issue_ready_reports() {
        let candidates = exception_candidates(
            vec![
                test_error("session-a", 100, Some("trace-a")),
                test_error("session-a", 200, Some("trace-b")),
                FrontendError {
                    message: "different".to_string(),
                    timestamp_unix_nanos: 300,
                    ..test_error("session-b", 300, None)
                },
            ],
            Vec::new(),
        );

        let reports = exception_reports(candidates);

        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].count, 1);
        assert_eq!(reports[1].count, 2);
        assert!(reports[1].issue_body_markdown.contains("Privacy note"));
        assert!(reports[1].traces.contains(&"trace-a".to_string()));
        assert!(reports[1].traces.contains(&"trace-b".to_string()));
    }

    #[test]
    fn includes_failed_ipc_calls_as_exception_reports() {
        let reports = exception_reports(exception_candidates(
            Vec::new(),
            vec![
                test_ipc("session-a", 100, Some("trace-a")),
                test_ipc("session-b", 200, Some("trace-b")),
            ],
        ));

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].source, ExceptionSource::FailedIpc);
        assert_eq!(reports[0].count, 2);
        assert!(reports[0].sample_context.contains_key("command"));
        assert!(reports[0]
            .issue_body_markdown
            .contains("Tauri IPC fixture_command"));
    }

    #[test]
    fn ipc_fingerprints_group_varying_error_messages_for_same_command() {
        let mut first = test_ipc("session-a", 100, Some("trace-a"));
        first.error_message = Some("failed for user 123".to_string());
        let mut second = test_ipc("session-a", 200, Some("trace-b"));
        second.error_message = Some("failed for user 456".to_string());

        let reports = exception_reports(exception_candidates(Vec::new(), vec![first, second]));

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].count, 2);
    }

    #[test]
    fn panic_hook_frontend_errors_are_classified_as_rust_panics() {
        let reports = exception_reports(exception_candidates(vec![test_panic()], Vec::new()));

        assert_eq!(reports[0].source, ExceptionSource::RustPanic);
        assert_eq!(reports[0].kind, "Panic");
    }

    #[test]
    fn dynamic_tokens_do_not_split_frontend_exception_groups() {
        let mut first = test_error("session-a", 100, Some("trace-a"));
        first.message = "Failed to load user 123e4567-e89b-12d3-a456-426614174000".to_string();
        let mut second = test_error("session-a", 200, Some("trace-b"));
        second.message = "Failed to load user 987e6543-e21b-12d3-a456-426614174999".to_string();

        let reports = exception_reports(exception_candidates(vec![first, second], Vec::new()));

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].count, 2);
    }

    #[test]
    fn markdown_escapes_stack_code_fences() {
        let mut error = test_error("session-a", 100, None);
        error.stack = Some("before\n```\nafter".to_string());

        let reports = exception_reports(exception_candidates(vec![error], Vec::new()));

        assert!(reports[0].issue_body_markdown.contains("`` `"));
        assert!(!reports[0].issue_body_markdown.contains("\n```\nafter"));
    }

    #[test]
    fn fingerprints_are_stable_for_matching_error_shapes() {
        let left = exception_candidates(
            vec![test_error("session-a", 100, Some("trace-a"))],
            Vec::new(),
        )
        .pop()
        .unwrap();
        let right = exception_candidates(
            vec![test_error("session-b", 200, Some("trace-b"))],
            Vec::new(),
        )
        .pop()
        .unwrap();
        assert_eq!(exception_fingerprint(&left), exception_fingerprint(&right));
    }

    fn test_error(
        session_id: &str,
        timestamp_unix_nanos: i64,
        trace_id: Option<&str>,
    ) -> FrontendError {
        FrontendError {
            session_id: session_id.to_string(),
            timestamp_unix_nanos,
            message: "boom".to_string(),
            stack: Some("Error: boom\n    at doThing (main.ts:10:2)".to_string()),
            filename: Some("main.ts".to_string()),
            line_number: Some(10),
            column_number: Some(2),
            error_type: Some("Error".to_string()),
            trace_id: trace_id.map(str::to_string),
            span_id: None,
            window_label: Some("main".to_string()),
            attributes: json!({ "auditaur.source": "frontend" }),
        }
    }

    fn test_panic() -> FrontendError {
        FrontendError {
            message: "panic boom".to_string(),
            error_type: Some("Panic".to_string()),
            attributes: json!({ "auditaur.source": "panic_hook" }),
            ..test_error("session-a", 100, None)
        }
    }

    fn test_ipc(
        session_id: &str,
        timestamp_unix_nanos: i64,
        trace_id: Option<&str>,
    ) -> TauriIpcCall {
        TauriIpcCall {
            session_id: session_id.to_string(),
            timestamp_unix_nanos,
            duration_ms: Some(3.0),
            command: "fixture_command".to_string(),
            status: "ERROR".to_string(),
            error_message: Some("fixture failure".to_string()),
            trace_id: trace_id.map(str::to_string),
            span_id: Some("span-fixture".to_string()),
            window_label: Some("main".to_string()),
            args_json: None,
            args_redacted: true,
            result_summary: None,
        }
    }
}
