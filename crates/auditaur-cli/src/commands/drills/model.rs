//! Drill file model and parser.
//!
//! Drill files are parsed by hand rather than through serde so `drill validate`
//! can report every problem with a precise path and a "did you mean" hint.

use std::{
    collections::HashSet,
    fmt,
    path::{Path, PathBuf},
};

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::commands::drill;

pub const SUITE_KEYS: &[&str] = &[
    "$schema",
    "suite",
    "description",
    "tags",
    "timeoutMs",
    "expectTimeoutMs",
    "retries",
    "serial",
    "env",
    "setup",
    "teardown",
    "beforeAll",
    "beforeEach",
    "afterEach",
    "afterAll",
    "tests",
];

pub const TEST_KEYS: &[&str] = &[
    "name",
    "description",
    "tags",
    "timeoutMs",
    "retries",
    "skip",
    "fixme",
    "only",
    "slow",
    "expectFail",
    "steps",
];

pub const STEP_COMMON_KEYS: &[&str] = &["name", "timeoutMs"];

pub const ACTION_KEYS: &[&str] = &[
    "click",
    "dblclick",
    "fill",
    "type",
    "press",
    "paste",
    "hover",
    "drag",
    "select",
    "check",
    "uncheck",
    "waitFor",
    "evaluate",
    "reload",
    "sleep",
    "screenshot",
    "gate",
    "expect",
    "expectIpc",
    "expectEvent",
    "expectLog",
    "expectNoErrors",
    "expectValue",
    "expectSpan",
    "expectNoTelemetry",
    "run",
    "dialog",
];

/// Dialogs a `dialog` step can answer: the `@tauri-apps/plugin-dialog` functions.
pub const DIALOG_KINDS: &[&str] = &["open", "save", "ask", "confirm", "message"];

/// Matchers for `expectValue`, which asserts on a JavaScript expression's result.
pub const VALUE_MATCHER_KEYS: &[&str] = &["toEqual", "toContain", "toMatch"];

/// What `paste: { … }` puts on the clipboard.
pub const PASTE_KEYS: &[&str] = &["text", "html", "files"];

pub const MATCHER_KEYS: &[&str] = &[
    "toExist",
    "toBeVisible",
    "toBeHidden",
    "toHaveText",
    "toContainText",
    "toHaveCount",
    "toHaveValue",
    "toBeChecked",
    "toMatchAriaSnapshot",
    "toHaveScreenshot",
];

pub const SCREENSHOT_OPTION_KEYS: &[&str] =
    &["maxDiffPixels", "maxDiffPixelRatio", "threshold", "mask"];

/// Step option keys that only apply to some actions.
pub const STEP_OPTION_KEYS: &[&str] = &[
    "button",
    "value",
    "values",
    "target",
    "state",
    "soft",
    "not",
    "status",
    "count",
    "maxMs",
    "level",
    "ignore",
    "attributes",
    "in",
    "args",
    "cwd",
    "exitCode",
    "stdout",
    "stderr",
    "respond",
    "expectOptions",
    "to",
];

pub const LOCATOR_KEYS: &[&str] = &[
    "css",
    "role",
    "text",
    "testId",
    "label",
    "placeholder",
    "name",
    "exact",
    "nth",
];

const LOCATOR_KINDS: &[&str] = &["css", "role", "text", "testId", "label", "placeholder"];

/// Every key the parser understands, used by the schema drift test.
pub fn all_known_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();
    for group in [
        SUITE_KEYS,
        TEST_KEYS,
        STEP_COMMON_KEYS,
        ACTION_KEYS,
        MATCHER_KEYS,
        VALUE_MATCHER_KEYS,
        PASTE_KEYS,
        SCREENSHOT_OPTION_KEYS,
        STEP_OPTION_KEYS,
        LOCATOR_KEYS,
        &["pattern"],
    ] {
        keys.extend_from_slice(group);
    }
    keys
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillSuite {
    pub path: PathBuf,
    pub suite: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    /// Tests depend on earlier ones: a failure skips the rest, and
    /// `--last-failed` reruns the whole suite (Playwright `describe.serial`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub serial: bool,
    /// Environment for the app launch, hooks, and `run` steps, in file order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    /// Process hooks validated against the existing drill hook schema.
    pub setup: Vec<Value>,
    pub teardown: Vec<Value>,
    pub before_all: Vec<Step>,
    pub before_each: Vec<Step>,
    pub after_each: Vec<Step>,
    pub after_all: Vec<Step>,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    pub mode: TestMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_reason: Option<String>,
    pub slow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_fail: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestMode {
    Run,
    Only,
    Skip,
    Fixme,
}

impl TestMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Only => "only",
            Self::Skip => "skip",
            Self::Fixme => "fixme",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub title: String,
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip)]
    pub kind: StepKind,
    /// Original JSON, kept for reporters and canvases.
    pub spec: Value,
}

#[derive(Debug, Clone)]
pub enum StepKind {
    Click {
        target: Locator,
        /// `None` for the main (left) button.
        button: Option<MouseButton>,
    },
    Dblclick(Locator),
    Fill {
        target: Locator,
        value: String,
    },
    Type {
        target: Locator,
        value: String,
    },
    Press {
        key: String,
        target: Option<Locator>,
    },
    Paste(PasteStep),
    Hover(Locator),
    /// Press `source`, move to `to` with the button held, and release.
    Drag {
        source: Locator,
        to: Locator,
    },
    Select {
        target: Locator,
        values: Vec<String>,
    },
    Check(Locator),
    Uncheck(Locator),
    WaitFor {
        target: Locator,
        state: WaitState,
    },
    Evaluate(String),
    Reload,
    Sleep(u64),
    Screenshot {
        name: String,
        target: Option<Locator>,
    },
    Gate(Value),
    Expect(Expectation),
    ExpectValue(ValueExpectation),
    Telemetry(TelemetryExpectation),
    Run(CommandStep),
    Dialog(DialogStep),
}

/// `dialog`: queue the answer to the app's next native dialog (see `withDrillDialogs` in
/// `@auditaur/api`).
#[derive(Debug, Clone)]
pub struct DialogStep {
    pub kind: &'static str,
    /// What the dialog function returns; `${NAME}` in strings expands from the drill env.
    pub respond: Value,
    /// A deep-partial match on the options the app opened the dialog with.
    pub expect_options: Option<Value>,
}

/// `paste`: what goes on the clipboard, pasted into `target` or the focused element. Files are
/// read when the step runs, after `${NAME}` in their paths expands from the drill env.
#[derive(Debug, Clone)]
pub struct PasteStep {
    pub text: Option<String>,
    pub html: Option<String>,
    pub files: Vec<String>,
    pub target: Option<Locator>,
}

/// `run`: a command run mid-test, with the same `cwd` rules as suite hooks.
#[derive(Debug, Clone)]
pub struct CommandStep {
    pub run: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub exit_code: i32,
    pub stdout: Option<Needle>,
    pub stderr: Option<Needle>,
}

impl CommandStep {
    /// The equivalent hook object, so the step runs exactly like a suite hook.
    pub fn hook(&self, name: &str, timeout_ms: u64) -> Value {
        let mut hook = serde_json::json!({
            "name": name,
            "run": self.run,
            "args": self.args,
            "timeoutMs": timeout_ms.max(1),
        });
        if let Some(cwd) = &self.cwd {
            hook["cwd"] = Value::String(cwd.clone());
        }
        hook
    }
}

/// `expectValue`: poll a JavaScript expression until its result matches.
#[derive(Debug, Clone)]
pub struct ValueExpectation {
    pub expression: String,
    pub matcher: ValueMatcher,
    pub not: bool,
    pub soft: bool,
}

#[derive(Debug, Clone)]
pub enum ValueMatcher {
    /// Deep JSON equality; numbers compare by value.
    ToEqual(Value),
    /// Substring of a string result, or an element of an array result.
    ToContain(Value),
    /// Regular expression a string result matches.
    ToMatch(Regex),
}

impl ValueMatcher {
    pub fn matches(&self, actual: &Value) -> bool {
        match self {
            Self::ToEqual(expected) => json_equal(expected, actual),
            Self::ToContain(expected) => match (actual, expected) {
                (Value::String(actual), Value::String(expected)) => actual.contains(expected),
                (Value::Array(items), expected) => {
                    items.iter().any(|item| json_equal(expected, item))
                }
                _ => false,
            },
            Self::ToMatch(pattern) => actual.as_str().is_some_and(|text| pattern.is_match(text)),
        }
    }
}

impl fmt::Display for ValueMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ToEqual(value) => write!(f, "to equal {value}"),
            Self::ToContain(value) => write!(f, "to contain {value}"),
            Self::ToMatch(pattern) => write!(f, "to match /{}/", pattern.as_str()),
        }
    }
}

/// JSON equality where `1` and `1.0` are the same number, as in JavaScript.
pub fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // Integers compare exactly; a float on either side compares by value (`2 == 2.0`).
        (Value::Number(a), Value::Number(b)) if !a.is_f64() && !b.is_f64() => a == b,
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| json_equal(a, b)))
        }
        _ => a == b,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    Attached,
    Detached,
    Visible,
    Hidden,
}

impl WaitState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::Detached => "detached",
            Self::Visible => "visible",
            Self::Hidden => "hidden",
        }
    }
}

/// A `click` step's non-main `button`, as in Playwright's `click({ button })`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Right,
    Middle,
}

impl MouseButton {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Middle => "middle",
        }
    }
}

/// `Some(None)` for no `button` or `"left"`; `None` after reporting an invalid one.
fn parse_button(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    parent: &str,
) -> Option<Option<MouseButton>> {
    match map.get("button") {
        None => Some(None),
        Some(Value::String(button)) => match button.as_str() {
            "left" => Some(None),
            "right" => Some(Some(MouseButton::Right)),
            "middle" => Some(Some(MouseButton::Middle)),
            other => {
                ctx.err(
                    &join(parent, "button"),
                    format!("unknown button {other:?}; use left, right or middle"),
                );
                None
            }
        },
        Some(other) => {
            ctx.err(
                &join(parent, "button"),
                format!("expected string, found {}", type_name(other)),
            );
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expectation {
    pub target: Locator,
    pub matcher: Matcher,
    pub not: bool,
    pub soft: bool,
}

#[derive(Debug, Clone)]
pub enum Matcher {
    ToExist,
    ToBeVisible,
    ToBeHidden,
    ToHaveText(TextMatch),
    ToContainText(String),
    ToHaveCount(u64),
    ToHaveValue(TextMatch),
    ToBeChecked,
    /// Snapshot name; `None` derives one from the test and step.
    ToMatchAriaSnapshot(Option<String>),
    ToHaveScreenshot(ScreenshotMatch),
}

impl Matcher {
    pub fn key(&self) -> &'static str {
        match self {
            Self::ToExist => "toExist",
            Self::ToBeVisible => "toBeVisible",
            Self::ToBeHidden => "toBeHidden",
            Self::ToHaveText(_) => "toHaveText",
            Self::ToContainText(_) => "toContainText",
            Self::ToHaveCount(_) => "toHaveCount",
            Self::ToHaveValue(_) => "toHaveValue",
            Self::ToBeChecked => "toBeChecked",
            Self::ToMatchAriaSnapshot(_) => "toMatchAriaSnapshot",
            Self::ToHaveScreenshot(_) => "toHaveScreenshot",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScreenshotMatch {
    pub name: Option<String>,
    pub max_diff_pixels: Option<u64>,
    pub max_diff_pixel_ratio: Option<f64>,
    pub threshold: f64,
    pub mask: Vec<Locator>,
}

pub const DEFAULT_SCREENSHOT_THRESHOLD: f64 = 0.2;

#[derive(Debug, Clone)]
pub enum TextMatch {
    Exact(String),
    Pattern(Regex),
}

impl TextMatch {
    pub fn matches(&self, actual: &str) -> bool {
        match self {
            Self::Exact(expected) => normalize_whitespace(actual) == normalize_whitespace(expected),
            Self::Pattern(pattern) => pattern.is_match(actual),
        }
    }
}

impl fmt::Display for TextMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(text) => write!(f, "{text:?}"),
            Self::Pattern(pattern) => write!(f, "/{}/", pattern.as_str()),
        }
    }
}

pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub enum TelemetryExpectation {
    Ipc {
        command: TextMatch,
        status: Option<IpcStatus>,
        count: Option<u64>,
        max_ms: Option<u64>,
        not: bool,
        soft: bool,
    },
    Event {
        name: String,
        count: Option<u64>,
        not: bool,
        soft: bool,
    },
    Log {
        contains: String,
        level: Option<String>,
        attributes: Vec<AttributeMatch>,
        count: Option<u64>,
        not: bool,
        soft: bool,
    },
    Span {
        name: TextMatch,
        status: Option<SpanStatus>,
        attributes: Vec<AttributeMatch>,
        count: Option<u64>,
        not: bool,
        soft: bool,
    },
    NoErrors {
        ignore: Vec<String>,
        soft: bool,
    },
    /// Nothing recorded so far contains the needle, e.g. a privacy guard.
    NoTelemetry {
        needle: Needle,
        kinds: Vec<TelemetryKind>,
        soft: bool,
    },
}

/// One `attributes` entry: the attribute equals a JSON value or matches `{ "pattern" }`.
#[derive(Debug, Clone)]
pub struct AttributeMatch {
    pub key: String,
    pub value: AttributeValue,
}

#[derive(Debug, Clone)]
pub enum AttributeValue {
    Equals(Value),
    Pattern(Regex),
}

impl AttributeMatch {
    pub fn matches(&self, attributes: &Value) -> bool {
        let Some(actual) = attributes.get(&self.key) else {
            return false;
        };
        match &self.value {
            AttributeValue::Equals(expected) => json_equal(expected, actual),
            AttributeValue::Pattern(pattern) => match actual {
                Value::String(text) => pattern.is_match(text),
                other => pattern.is_match(&other.to_string()),
            },
        }
    }
}

impl fmt::Display for AttributeMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            AttributeValue::Equals(value) => write!(f, "{}={value}", self.key),
            AttributeValue::Pattern(pattern) => write!(f, "{}=/{}/", self.key, pattern.as_str()),
        }
    }
}

/// A substring, or `{ "pattern": "regex" }`.
#[derive(Debug, Clone)]
pub enum Needle {
    Contains(String),
    Pattern(Regex),
}

impl Needle {
    pub fn is_in(&self, text: &str) -> bool {
        match self {
            Self::Contains(needle) => text.contains(needle.as_str()),
            Self::Pattern(pattern) => pattern.is_match(text),
        }
    }
}

impl fmt::Display for Needle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contains(text) => write!(f, "{text:?}"),
            Self::Pattern(pattern) => write!(f, "/{}/", pattern.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryKind {
    Logs,
    Spans,
    Ipc,
    Events,
}

impl TelemetryKind {
    pub const ALL: [Self; 4] = [Self::Logs, Self::Spans, Self::Ipc, Self::Events];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Spans => "spans",
            Self::Ipc => "ipc",
            Self::Events => "events",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    Ok,
    Error,
    Unset,
}

impl SpanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Unset => "unset",
        }
    }
}

/// A name for titles: exact names bare, patterns as `/regex/`.
pub fn name_label(name: &TextMatch) -> String {
    match name {
        TextMatch::Exact(name) => name.clone(),
        TextMatch::Pattern(pattern) => format!("/{}/", pattern.as_str()),
    }
}

/// Telemetry names (commands, spans) match exactly, without text whitespace normalization.
pub fn name_matches(name: &TextMatch, actual: &str) -> bool {
    match name {
        TextMatch::Exact(name) => name == actual,
        TextMatch::Pattern(pattern) => pattern.is_match(actual),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Locator {
    Css(String),
    CssNth {
        css: String,
        nth: i64,
    },
    Role {
        role: String,
        name: Option<String>,
        exact: bool,
        nth: Option<i64>,
    },
    Text {
        text: String,
        exact: bool,
        nth: Option<i64>,
    },
    TestId {
        id: String,
        nth: Option<i64>,
    },
    Label {
        label: String,
        exact: bool,
        nth: Option<i64>,
    },
    Placeholder {
        placeholder: String,
        exact: bool,
        nth: Option<i64>,
    },
}

impl fmt::Display for Locator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn nth(f: &mut fmt::Formatter<'_>, nth: &Option<i64>) -> fmt::Result {
            match nth {
                Some(nth) => write!(f, " >> nth={nth}"),
                None => Ok(()),
            }
        }
        fn flag(exact: bool) -> &'static str {
            if exact {
                "s"
            } else {
                "i"
            }
        }
        match self {
            Self::Css(css) => write!(f, "{css}"),
            Self::CssNth { css, nth } => write!(f, "{css} >> nth={nth}"),
            Self::Role {
                role,
                name,
                exact,
                nth: n,
            } => {
                write!(f, "role={role}")?;
                if let Some(name) = name {
                    write!(f, "[name={name:?}{}]", flag(*exact))?;
                }
                nth(f, n)
            }
            Self::Text {
                text,
                exact,
                nth: n,
            } => {
                write!(f, "text={text:?}{}", flag(*exact))?;
                nth(f, n)
            }
            Self::TestId { id, nth: n } => {
                write!(f, "testId={id}")?;
                nth(f, n)
            }
            Self::Label {
                label,
                exact,
                nth: n,
            } => {
                write!(f, "label={label:?}{}", flag(*exact))?;
                nth(f, n)
            }
            Self::Placeholder {
                placeholder,
                exact,
                nth: n,
            } => {
                write!(f, "placeholder={placeholder:?}{}", flag(*exact))?;
                nth(f, n)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub suite: Option<DrillSuite>,
    pub issues: Vec<Issue>,
}

pub fn parse_file(path: &Path) -> ParsedFile {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_str(path, &text),
        Err(error) => ParsedFile {
            path: path.to_path_buf(),
            suite: None,
            issues: vec![Issue {
                path: String::new(),
                message: format!("failed to read file: {error}"),
            }],
        },
    }
}

pub fn parse_str(path: &Path, text: &str) -> ParsedFile {
    let mut ctx = Ctx::default();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let suite = match serde_json::from_str::<Value>(text) {
        Ok(value) => parse_suite(&mut ctx, path, &value),
        Err(error) => {
            ctx.err("", format!("invalid JSON: {error}"));
            None
        }
    };
    let suite = if ctx.issues.is_empty() { suite } else { None };
    ParsedFile {
        path: path.to_path_buf(),
        suite,
        issues: ctx.issues,
    }
}

/// Suite name derived from `name.drill.json`.
pub fn default_suite_name(path: &Path) -> String {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    file.strip_suffix(".drill.json")
        .or_else(|| file.strip_suffix(".json"))
        .unwrap_or(&file)
        .to_string()
}

#[derive(Default)]
struct Ctx {
    issues: Vec<Issue>,
}

impl Ctx {
    fn err(&mut self, path: &str, message: impl Into<String>) {
        self.issues.push(Issue {
            path: path.to_string(),
            message: message.into(),
        });
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn index(path: &str, key: &str, index: usize) -> String {
    format!("{}[{index}]", join(path, key))
}

fn as_object<'a>(
    ctx: &mut Ctx,
    value: &'a Value,
    path: &str,
    what: &str,
) -> Option<&'a Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        other => {
            ctx.err(
                path,
                format!("expected {what} object, found {}", type_name(other)),
            );
            None
        }
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check_keys(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    path: &str,
    allowed: &[&str],
    context: &str,
) {
    let known = all_known_keys();
    for key in map.keys() {
        if allowed.contains(&key.as_str()) {
            continue;
        }
        let message = if known.contains(&key.as_str()) {
            format!("`{key}` is not valid {context}")
        } else {
            match suggest(key, allowed) {
                Some(hint) => format!("unknown key `{key}` {context} (did you mean `{hint}`?)"),
                None => format!("unknown key `{key}` {context}"),
            }
        };
        ctx.err(&join(path, key), message);
    }
}

fn suggest<'a>(key: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let lower = key.to_ascii_lowercase();
    candidates
        .iter()
        .map(|candidate| {
            (
                edit_distance(&lower, &candidate.to_ascii_lowercase()),
                *candidate,
            )
        })
        .filter(|(distance, candidate)| *distance <= (candidate.len() / 3).max(2))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()]
}

fn get_string(ctx: &mut Ctx, map: &Map<String, Value>, key: &str, path: &str) -> Option<String> {
    let value = map.get(key)?;
    string_value(ctx, value, &join(path, key))
}

/// Validate a Playwright-style key chord the way the drive bridge tokenizes it:
/// `+` splits only after a token, so `+` and `Shift++` name the `+` key.
fn check_key_chord(chord: &str) -> Result<(), String> {
    const MODIFIERS: [&str; 5] = ["Alt", "Control", "ControlOrMeta", "Meta", "Shift"];
    let mut parts = Vec::new();
    let mut token = String::new();
    for c in chord.chars() {
        if c == '+' && !token.is_empty() {
            parts.push(std::mem::take(&mut token));
        } else {
            token.push(c);
        }
    }
    if token.is_empty() {
        return Err(format!(
            "invalid key chord `{chord}`: missing key after `+`"
        ));
    }
    match parts
        .iter()
        .find(|part| !MODIFIERS.contains(&part.as_str()))
    {
        Some(part) => Err(format!(
            "invalid key chord `{chord}`: unknown modifier `{part}` (use {})",
            MODIFIERS.join(", ")
        )),
        None => Ok(()),
    }
}

fn string_value(ctx: &mut Ctx, value: &Value, path: &str) -> Option<String> {
    match value {
        Value::String(text) if text.trim().is_empty() => {
            ctx.err(path, "must not be empty");
            None
        }
        Value::String(text) => Some(text.clone()),
        other => {
            ctx.err(path, format!("expected string, found {}", type_name(other)));
            None
        }
    }
}

fn get_u64(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    key: &str,
    path: &str,
    min: u64,
) -> Option<u64> {
    let value = map.get(key)?;
    let path = join(path, key);
    match value.as_u64() {
        Some(number) if number >= min => Some(number),
        Some(_) => {
            ctx.err(&path, format!("must be at least {min}"));
            None
        }
        None => {
            ctx.err(
                &path,
                format!("expected non-negative integer, found {}", type_name(value)),
            );
            None
        }
    }
}

fn get_retries(ctx: &mut Ctx, map: &Map<String, Value>, path: &str) -> Option<u32> {
    let retries = get_u64(ctx, map, "retries", path, 0)?;
    if retries > 10 {
        ctx.err(&join(path, "retries"), "must be at most 10");
        return None;
    }
    Some(retries as u32)
}

fn get_bool(ctx: &mut Ctx, map: &Map<String, Value>, key: &str, path: &str) -> bool {
    match map.get(key) {
        None => false,
        Some(Value::Bool(flag)) => *flag,
        Some(other) => {
            ctx.err(
                &join(path, key),
                format!("expected boolean, found {}", type_name(other)),
            );
            false
        }
    }
}

fn get_tags(ctx: &mut Ctx, map: &Map<String, Value>, path: &str) -> Vec<String> {
    let Some(value) = map.get("tags") else {
        return Vec::new();
    };
    let path = join(path, "tags");
    let Some(items) = value.as_array() else {
        ctx.err(
            &path,
            format!("expected array of strings, found {}", type_name(value)),
        );
        return Vec::new();
    };
    let mut tags = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let item_path = format!("{path}[{i}]");
        let Some(tag) = string_value(ctx, item, &item_path) else {
            continue;
        };
        let tag = tag.trim_start_matches('@').to_string();
        if tag.is_empty() || tag.chars().any(char::is_whitespace) {
            ctx.err(
                &item_path,
                "tags must be non-empty and contain no whitespace",
            );
        } else if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// `true` or a non-empty reason string, as used by `skip`, `fixme` and `expectFail`.
fn get_flag_reason(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Option<Option<String>> {
    match map.get(key)? {
        Value::Bool(true) => Some(None),
        Value::Bool(false) => None,
        Value::String(reason) if !reason.trim().is_empty() => Some(Some(reason.clone())),
        other => {
            ctx.err(
                &join(path, key),
                format!(
                    "expected `true` or a reason string, found {}",
                    type_name(other)
                ),
            );
            None
        }
    }
}

const SERIAL_RETRIES: &str =
    "is not valid in a serial suite; a test can't be retried apart from the tests it depends on";

fn parse_suite(ctx: &mut Ctx, path: &Path, value: &Value) -> Option<DrillSuite> {
    let map = as_object(ctx, value, "", "drill file")?;
    check_keys(ctx, map, "", SUITE_KEYS, "at the top level of a drill file");
    if let Some(schema) = map.get("$schema").filter(|value| !value.is_string()) {
        ctx.err(
            "$schema",
            format!("expected string, found {}", type_name(schema)),
        );
    }
    let suite = get_string(ctx, map, "suite", "").unwrap_or_else(|| default_suite_name(path));
    let description = get_string(ctx, map, "description", "");
    let tags = get_tags(ctx, map, "");
    let timeout_ms = get_u64(ctx, map, "timeoutMs", "", 1);
    let expect_timeout_ms = get_u64(ctx, map, "expectTimeoutMs", "", 1);
    let retries = get_retries(ctx, map, "");
    let serial = get_bool(ctx, map, "serial", "");
    if serial && retries.is_some() {
        ctx.err("retries", SERIAL_RETRIES);
    }
    let env = parse_env(ctx, map);
    let setup = parse_hooks(ctx, map, "setup");
    let teardown = parse_hooks(ctx, map, "teardown");
    let before_all = parse_steps(ctx, map, "beforeAll", "");
    let before_each = parse_steps(ctx, map, "beforeEach", "");
    let after_each = parse_steps(ctx, map, "afterEach", "");
    let after_all = parse_steps(ctx, map, "afterAll", "");

    let mut tests = Vec::new();
    match map.get("tests") {
        None => ctx.err("tests", "is required"),
        Some(Value::Array(items)) if items.is_empty() => {
            ctx.err("tests", "must contain at least one test")
        }
        Some(Value::Array(items)) => {
            let mut names = HashSet::new();
            for (i, item) in items.iter().enumerate() {
                if let Some(test) = parse_test(ctx, item, &index("", "tests", i)) {
                    if serial && test.retries.is_some() {
                        ctx.err(&join(&index("", "tests", i), "retries"), SERIAL_RETRIES);
                    }
                    if !names.insert(test.name.clone()) {
                        ctx.err(
                            &index("", "tests", i),
                            format!("duplicate test name {:?} in suite {suite:?}", test.name),
                        );
                    }
                    tests.push(test);
                }
            }
        }
        Some(other) => ctx.err(
            "tests",
            format!("expected array, found {}", type_name(other)),
        ),
    }

    Some(DrillSuite {
        path: path.to_path_buf(),
        suite,
        description,
        tags,
        timeout_ms,
        expect_timeout_ms,
        retries,
        serial,
        env,
        setup,
        teardown,
        before_all,
        before_each,
        after_each,
        after_all,
        tests,
    })
}

fn parse_env(ctx: &mut Ctx, map: &Map<String, Value>) -> Vec<(String, String)> {
    let Some(value) = map.get("env") else {
        return Vec::new();
    };
    let Some(vars) = value.as_object() else {
        ctx.err(
            "env",
            format!("expected object of strings, found {}", type_name(value)),
        );
        return Vec::new();
    };
    let mut env = Vec::new();
    for (name, value) in vars {
        let path = format!("env.{name}");
        if let Err(problem) = check_env_name(name) {
            ctx.err(&path, problem);
        } else if let Some(text) = value.as_str() {
            if text.contains('\0') {
                ctx.err(&path, "must not contain NUL");
            } else {
                env.push((name.clone(), text.to_string()));
            }
        } else {
            ctx.err(
                &path,
                format!("expected string, found {}", type_name(value)),
            );
        }
    }
    env
}

/// Setup hooks append `KEY=VALUE` lines to the file this names to set suite environment.
pub const ENV_FILE_VAR: &str = "AUDITAUR_DRILL_ENV_FILE";

/// Why `name` can't be an environment variable name, if it can't.
pub fn check_env_name(name: &str) -> Result<(), String> {
    if name.eq_ignore_ascii_case(ENV_FILE_VAR) {
        Err(format!("{ENV_FILE_VAR} is set by Auditaur"))
    } else if name.is_empty() {
        Err("environment variable name must not be empty".to_string())
    } else if name.contains(['=', '\0']) || name.chars().any(char::is_whitespace) {
        Err(format!(
            "invalid environment variable name {name:?}: no `=`, NUL, or whitespace"
        ))
    } else {
        Ok(())
    }
}

fn parse_hooks(ctx: &mut Ctx, map: &Map<String, Value>, key: &str) -> Vec<Value> {
    let Some(value) = map.get(key) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        ctx.err(
            key,
            format!("expected array of hooks, found {}", type_name(value)),
        );
        return Vec::new();
    };
    let mut hooks = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match drill::check_hook_json(item) {
            Ok(()) => hooks.push(item.clone()),
            Err(error) => ctx.err(&index("", key, i), format!("invalid hook: {error:#}")),
        }
    }
    hooks
}

fn parse_steps(ctx: &mut Ctx, map: &Map<String, Value>, key: &str, path: &str) -> Vec<Step> {
    let Some(value) = map.get(key) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        ctx.err(
            &join(path, key),
            format!("expected array of steps, found {}", type_name(value)),
        );
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| parse_step(ctx, item, &index(path, key, i)))
        .collect()
}

fn parse_test(ctx: &mut Ctx, value: &Value, path: &str) -> Option<TestCase> {
    let map = as_object(ctx, value, path, "test")?;
    let name = get_string(ctx, map, "name", path);
    if !map.contains_key("name") {
        ctx.err(path, "test `name` is required");
    }
    let path = match &name {
        Some(name) => format!("{path} ({name:?})"),
        None => path.to_string(),
    };
    check_keys(ctx, map, &path, TEST_KEYS, "in a test");
    let description = get_string(ctx, map, "description", &path);
    let tags = get_tags(ctx, map, &path);
    let timeout_ms = get_u64(ctx, map, "timeoutMs", &path, 1);
    let retries = get_retries(ctx, map, &path);
    let slow = get_bool(ctx, map, "slow", &path);
    let only = get_bool(ctx, map, "only", &path);
    let skip = get_flag_reason(ctx, map, "skip", &path);
    let fixme = get_flag_reason(ctx, map, "fixme", &path);
    let expect_fail = get_flag_reason(ctx, map, "expectFail", &path)
        .map(|reason| reason.unwrap_or_else(|| "expected to fail".to_string()));

    if [only, skip.is_some(), fixme.is_some()]
        .iter()
        .filter(|set| **set)
        .count()
        > 1
    {
        ctx.err(&path, "use only one of `only`, `skip` and `fixme`");
    }
    let (mode, mode_reason) = if let Some(reason) = skip {
        (TestMode::Skip, reason)
    } else if let Some(reason) = fixme {
        (TestMode::Fixme, reason)
    } else if only {
        (TestMode::Only, None)
    } else {
        (TestMode::Run, None)
    };

    let steps = parse_steps(ctx, map, "steps", &path);
    let parked = matches!(mode, TestMode::Skip | TestMode::Fixme);
    if !map.contains_key("steps") && !parked {
        ctx.err(
            &path,
            "`steps` is required (or mark the test `skip`/`fixme`)",
        );
    } else if map
        .get("steps")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        && !parked
    {
        ctx.err(&join(&path, "steps"), "must contain at least one step");
    }

    Some(TestCase {
        name: name?,
        description,
        tags,
        timeout_ms,
        retries,
        mode,
        mode_reason,
        slow,
        expect_fail,
        steps,
    })
}

fn parse_step(ctx: &mut Ctx, value: &Value, path: &str) -> Option<Step> {
    let map = as_object(ctx, value, path, "step")?;
    let actions: Vec<&'static str> = ACTION_KEYS
        .iter()
        .copied()
        .filter(|action| map.contains_key(*action))
        .collect();
    let action = match actions.as_slice() {
        [action] => *action,
        [] => {
            let hint = map
                .keys()
                .find_map(|key| {
                    suggest(key, ACTION_KEYS).map(|hint| format!(" (did you mean `{hint}`?)"))
                })
                .unwrap_or_default();
            ctx.err(
                path,
                format!("step needs exactly one action such as `click`, `fill` or `expect`{hint}"),
            );
            return None;
        }
        many => {
            ctx.err(
                path,
                format!(
                    "step has several actions ({}); split them into separate steps",
                    many.iter()
                        .map(|key| format!("`{key}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            return None;
        }
    };
    let mut allowed: Vec<&str> = STEP_COMMON_KEYS.to_vec();
    allowed.push(action);
    let extra: &[&str] = match action {
        "click" => &["button"],
        "fill" | "type" => &["value"],
        "press" | "paste" | "screenshot" => &["target"],
        "select" => &["value", "values"],
        "waitFor" => &["state"],
        "expect" => &["not", "soft"],
        "expectIpc" => &["status", "count", "maxMs", "not", "soft"],
        "expectEvent" => &["count", "not", "soft"],
        "expectLog" => &["level", "attributes", "count", "not", "soft"],
        "expectSpan" => &["status", "attributes", "count", "not", "soft"],
        "expectNoTelemetry" => &["in", "soft"],
        "expectNoErrors" => &["ignore", "soft"],
        "expectValue" => &["toEqual", "toContain", "toMatch", "not", "soft"],
        "run" => &["args", "cwd", "exitCode", "stdout", "stderr"],
        "dialog" => &["respond", "expectOptions"],
        "drag" => &["to"],
        _ => &[],
    };
    allowed.extend_from_slice(extra);
    // Reported below with a pointer to the right key.
    allowed.push("expectTimeoutMs");
    if action == "expect" {
        allowed.extend_from_slice(MATCHER_KEYS);
        allowed.extend_from_slice(SCREENSHOT_OPTION_KEYS);
    }
    check_keys(ctx, map, path, &allowed, &format!("with `{action}`"));
    if map.contains_key("expectTimeoutMs") {
        // A common slip: the suite's default retry window, written on one step.
        ctx.err(
            &join(path, "expectTimeoutMs"),
            "`expectTimeoutMs` is a suite setting; set this step's retry window with `timeoutMs`",
        );
    }

    let timeout_ms = get_u64(ctx, map, "timeoutMs", path, 1);
    let custom_name = get_string(ctx, map, "name", path);
    let kind = parse_step_kind(ctx, map, action, path)?;
    let title = custom_name.unwrap_or_else(|| describe(&kind));
    Some(Step {
        title,
        action,
        timeout_ms,
        kind,
        spec: value.clone(),
    })
}

fn optional_locator(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    key: &str,
    parent: &str,
) -> Result<Option<Locator>, ()> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => parse_locator(ctx, value, &join(parent, key))
            .map(Some)
            .ok_or(()),
    }
}

fn parse_step_kind(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    action: &str,
    parent: &str,
) -> Option<StepKind> {
    let raw = &map[action];
    let path = join(parent, action);
    let path = path.as_str();
    let required_value = |ctx: &mut Ctx| match map.get("value") {
        None => {
            ctx.err(parent, format!("`{action}` requires `value`"));
            None
        }
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => {
            ctx.err(
                &join(parent, "value"),
                format!("expected string, found {}", type_name(other)),
            );
            None
        }
    };
    Some(match action {
        "click" => {
            let target = parse_locator(ctx, raw, path);
            let button = parse_button(ctx, map, parent)?;
            StepKind::Click {
                target: target?,
                button,
            }
        }
        "dblclick" => StepKind::Dblclick(parse_locator(ctx, raw, path)?),
        "hover" => StepKind::Hover(parse_locator(ctx, raw, path)?),
        "drag" => {
            let source = parse_locator(ctx, raw, path);
            let to = match map.get("to") {
                Some(value) => parse_locator(ctx, value, &join(parent, "to")),
                None => {
                    ctx.err(parent, "`drag` requires `to`, the locator to drop on");
                    None
                }
            };
            StepKind::Drag {
                source: source?,
                to: to?,
            }
        }
        "check" => StepKind::Check(parse_locator(ctx, raw, path)?),
        "uncheck" => StepKind::Uncheck(parse_locator(ctx, raw, path)?),
        "fill" | "type" => {
            let target = parse_locator(ctx, raw, path);
            let value = required_value(ctx);
            let (target, value) = (target?, value?);
            if action == "fill" {
                StepKind::Fill { target, value }
            } else {
                StepKind::Type { target, value }
            }
        }
        "press" => {
            let key = string_value(ctx, raw, path).filter(|chord| match check_key_chord(chord) {
                Ok(()) => true,
                Err(message) => {
                    ctx.err(path, message);
                    false
                }
            });
            let target = optional_locator(ctx, map, "target", parent);
            StepKind::Press {
                key: key?,
                target: target.ok()?,
            }
        }
        "select" => {
            let target = parse_locator(ctx, raw, path);
            let values = match (map.get("value"), map.get("values")) {
                (Some(_), Some(_)) => {
                    ctx.err(parent, "use either `value` or `values`, not both");
                    None
                }
                (Some(value), None) => {
                    string_value(ctx, value, &join(parent, "value")).map(|value| vec![value])
                }
                (None, Some(Value::Array(items))) if !items.is_empty() => {
                    let values_path = join(parent, "values");
                    let parsed: Vec<Option<String>> = items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| string_value(ctx, item, &format!("{values_path}[{i}]")))
                        .collect();
                    parsed.into_iter().collect()
                }
                (None, Some(_)) => {
                    ctx.err(
                        &join(parent, "values"),
                        "expected a non-empty array of strings",
                    );
                    None
                }
                (None, None) => {
                    ctx.err(parent, "`select` requires `value` or `values`");
                    None
                }
            };
            StepKind::Select {
                target: target?,
                values: values?,
            }
        }
        "waitFor" => {
            let target = parse_locator(ctx, raw, path);
            let state = match map.get("state") {
                None => Some(WaitState::Visible),
                Some(Value::String(state)) => match state.as_str() {
                    "attached" => Some(WaitState::Attached),
                    "detached" => Some(WaitState::Detached),
                    "visible" => Some(WaitState::Visible),
                    "hidden" => Some(WaitState::Hidden),
                    other => {
                        ctx.err(
                            &join(parent, "state"),
                            format!(
                                "unknown state {other:?}; use attached, detached, visible or hidden"
                            ),
                        );
                        None
                    }
                },
                Some(other) => {
                    ctx.err(
                        &join(parent, "state"),
                        format!("expected string, found {}", type_name(other)),
                    );
                    None
                }
            };
            StepKind::WaitFor {
                target: target?,
                state: state?,
            }
        }
        "evaluate" => StepKind::Evaluate(string_value(ctx, raw, path)?),
        "reload" => {
            if raw != &Value::Bool(true) {
                ctx.err(path, "expected `true`");
                return None;
            }
            StepKind::Reload
        }
        "sleep" => match raw.as_u64() {
            Some(ms) if (1..=60_000).contains(&ms) => StepKind::Sleep(ms),
            _ => {
                ctx.err(
                    path,
                    "expected milliseconds between 1 and 60000; prefer `waitFor` or `expect`",
                );
                return None;
            }
        },
        "screenshot" => {
            let name = parse_snapshot_name(ctx, raw, path, false);
            let target = optional_locator(ctx, map, "target", parent);
            StepKind::Screenshot {
                name: name??,
                target: target.ok()?,
            }
        }
        "gate" => {
            if let Err(error) = drill::check_gate_json(raw) {
                ctx.err(path, format!("invalid gate: {error:#}"));
                return None;
            }
            StepKind::Gate(raw.clone())
        }
        "expect" => StepKind::Expect(parse_expectation(ctx, map, raw, parent, path)?),
        "expectValue" => {
            StepKind::ExpectValue(parse_value_expectation(ctx, map, raw, parent, path)?)
        }
        "run" => StepKind::Run(parse_command_step(ctx, map, raw, parent, path)?),
        "dialog" => StepKind::Dialog(parse_dialog_step(ctx, map, raw, parent, path)?),
        "paste" => StepKind::Paste(parse_paste_step(ctx, map, raw, parent, path)?),
        _ => StepKind::Telemetry(parse_telemetry(ctx, map, action, raw, parent, path)?),
    })
}

/// `paste: "text"`, or `paste: { text?, html?, files? }` with at least one.
fn parse_paste_step(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    raw: &Value,
    parent: &str,
    path: &str,
) -> Option<PasteStep> {
    let target = optional_locator(ctx, map, "target", parent);
    let nonempty = |ctx: &mut Ctx, value: &Value, path: &str| match value {
        Value::String(text) if text.is_empty() => {
            ctx.err(path, "must not be empty");
            None
        }
        Value::String(text) => Some(text.clone()),
        other => {
            ctx.err(path, format!("expected string, found {}", type_name(other)));
            None
        }
    };
    let content = match raw {
        Value::String(_) => nonempty(ctx, raw, path).map(|text| (Some(text), None, Vec::new())),
        Value::Object(content) => {
            check_keys(ctx, content, path, PASTE_KEYS, "in `paste`");
            let mut ok = true;
            let mut field = |ctx: &mut Ctx, key: &str| {
                let value = content.get(key)?;
                let parsed = nonempty(ctx, value, &join(path, key));
                ok &= parsed.is_some();
                parsed
            };
            let text = field(ctx, "text");
            let html = field(ctx, "html");
            let files = match content.get("files") {
                None => Vec::new(),
                Some(Value::Array(items)) if !items.is_empty() => {
                    let files_path = join(path, "files");
                    items
                        .iter()
                        .enumerate()
                        .filter_map(|(i, item)| {
                            let file = string_value(ctx, item, &format!("{files_path}[{i}]"));
                            ok &= file.is_some();
                            file
                        })
                        .collect()
                }
                Some(_) => {
                    ctx.err(
                        &join(path, "files"),
                        "expected a non-empty array of file paths",
                    );
                    ok = false;
                    Vec::new()
                }
            };
            // Any other key was reported above as unknown.
            if ok && content.is_empty() {
                ctx.err(path, "`paste` needs `text`, `html`, or `files`");
                ok = false;
            }
            ok.then_some((text, html, files))
        }
        other => {
            ctx.err(
                path,
                format!(
                    "expected the text to paste, or {{ text, html, files }}, found {}",
                    type_name(other)
                ),
            );
            None
        }
    };
    let (text, html, files) = content?;
    Some(PasteStep {
        text,
        html,
        files,
        target: target.ok()?,
    })
}

fn parse_dialog_step(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    raw: &Value,
    parent: &str,
    path: &str,
) -> Option<DialogStep> {
    let kind = match raw {
        Value::String(kind) => match DIALOG_KINDS.iter().find(|known| **known == kind) {
            Some(known) => Some(*known),
            None => {
                ctx.err(
                    path,
                    format!("unknown dialog {kind:?}; use {}", DIALOG_KINDS.join(", ")),
                );
                None
            }
        },
        other => {
            ctx.err(path, format!("expected string, found {}", type_name(other)));
            None
        }
    };
    let respond_path = join(parent, "respond");
    let respond = match (kind, map.get("respond")) {
        (None, _) => None,
        // `message` returns the clicked button's label; most callers ignore it.
        (Some("message"), None) => Some(Value::String("Ok".to_string())),
        (Some(kind), None) => {
            ctx.err(
                parent,
                format!("`dialog: {kind}` requires `respond` (use null to cancel)"),
            );
            None
        }
        (Some(kind), Some(value)) => {
            let strings = |items: &[Value]| items.iter().all(Value::is_string);
            let valid = match kind {
                "open" => match value {
                    Value::Null | Value::String(_) => true,
                    Value::Array(items) => !items.is_empty() && strings(items),
                    _ => false,
                },
                "save" => matches!(value, Value::Null | Value::String(_)),
                "ask" | "confirm" => value.is_boolean(),
                _ => value.is_string(),
            };
            if valid {
                Some(value.clone())
            } else {
                let expected = match kind {
                    "open" => "a path, a non-empty array of paths, or null (cancel)",
                    "save" => "a path, or null (cancel)",
                    "ask" | "confirm" => "true or false",
                    _ => "the clicked button's label, such as \"Ok\"",
                };
                ctx.err(
                    &respond_path,
                    format!(
                        "`{kind}` dialogs respond with {expected}, found {}",
                        type_name(value)
                    ),
                );
                None
            }
        }
    };
    let expect_options = match map.get("expectOptions") {
        None => Some(None),
        Some(value @ Value::Object(_)) => Some(Some(value.clone())),
        Some(other) => {
            ctx.err(
                &join(parent, "expectOptions"),
                format!(
                    "expected an object of the dialog options to match, found {}",
                    type_name(other)
                ),
            );
            None
        }
    };
    Some(DialogStep {
        kind: kind?,
        respond: respond?,
        expect_options: expect_options?,
    })
}

fn parse_command_step(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    raw: &Value,
    parent: &str,
    path: &str,
) -> Option<CommandStep> {
    let run = string_value(ctx, raw, path);
    let args = match map.get("args") {
        None => Some(Vec::new()),
        Some(Value::Array(items)) => {
            let args_path = join(parent, "args");
            let parsed: Vec<Option<String>> = items
                .iter()
                .enumerate()
                .map(|(i, item)| match item {
                    Value::String(text) => Some(text.clone()),
                    other => {
                        ctx.err(
                            &format!("{args_path}[{i}]"),
                            format!("expected string, found {}", type_name(other)),
                        );
                        None
                    }
                })
                .collect();
            parsed.into_iter().collect()
        }
        Some(other) => {
            ctx.err(
                &join(parent, "args"),
                format!("expected an array of strings, found {}", type_name(other)),
            );
            None
        }
    };
    let cwd = get_string(ctx, map, "cwd", parent);
    let exit_code = match map.get("exitCode") {
        None => Some(0),
        Some(value) => match value.as_i64().and_then(|code| i32::try_from(code).ok()) {
            Some(code) => Some(code),
            None => {
                ctx.err(&join(parent, "exitCode"), "expected an integer exit code");
                None
            }
        },
    };
    let mut output = |key: &str| match map.get(key) {
        None => Some(None),
        Some(value) => parse_name_match(ctx, value, &join(parent, key)).map(|m| Some(needle(m))),
    };
    let stdout = output("stdout");
    let stderr = output("stderr");
    Some(CommandStep {
        run: run?,
        args: args?,
        cwd,
        exit_code: exit_code?,
        stdout: stdout?,
        stderr: stderr?,
    })
}

fn needle(text: TextMatch) -> Needle {
    match text {
        TextMatch::Exact(text) => Needle::Contains(text),
        TextMatch::Pattern(pattern) => Needle::Pattern(pattern),
    }
}

fn parse_value_expectation(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    raw: &Value,
    parent: &str,
    path: &str,
) -> Option<ValueExpectation> {
    let expression = string_value(ctx, raw, path);
    let not = get_bool(ctx, map, "not", parent);
    let soft = get_bool(ctx, map, "soft", parent);
    let present: Vec<&str> = VALUE_MATCHER_KEYS
        .iter()
        .copied()
        .filter(|key| map.contains_key(*key))
        .collect();
    let key = match present.as_slice() {
        [key] => *key,
        [] => {
            ctx.err(
                parent,
                format!(
                    "`expectValue` needs one matcher: {}",
                    VALUE_MATCHER_KEYS.join(", ")
                ),
            );
            return None;
        }
        many => {
            ctx.err(
                parent,
                format!(
                    "`expectValue` has several matchers ({}); use one per step",
                    many.join(", ")
                ),
            );
            return None;
        }
    };
    let value = &map[key];
    let matcher = match key {
        "toEqual" => Some(ValueMatcher::ToEqual(value.clone())),
        "toContain" => Some(ValueMatcher::ToContain(value.clone())),
        "toMatch" => {
            let matcher_path = join(parent, key);
            string_value(ctx, value, &matcher_path).and_then(|pattern| match Regex::new(&pattern) {
                Ok(regex) => Some(ValueMatcher::ToMatch(regex)),
                Err(error) => {
                    ctx.err(
                        &matcher_path,
                        format!("invalid regular expression: {error}"),
                    );
                    None
                }
            })
        }
        _ => unreachable!("value matcher keys are exhaustive"),
    };
    Some(ValueExpectation {
        expression: expression?,
        matcher: matcher?,
        not,
        soft,
    })
}

fn parse_expectation(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    raw: &Value,
    parent: &str,
    path: &str,
) -> Option<Expectation> {
    let target = parse_locator(ctx, raw, path);
    let not = get_bool(ctx, map, "not", parent);
    let soft = get_bool(ctx, map, "soft", parent);
    let matchers: Vec<&str> = MATCHER_KEYS
        .iter()
        .copied()
        .filter(|key| map.contains_key(*key))
        .collect();
    let key = match matchers.as_slice() {
        [key] => *key,
        [] => {
            ctx.err(
                parent,
                format!("`expect` needs one matcher: {}", MATCHER_KEYS.join(", ")),
            );
            return None;
        }
        many => {
            ctx.err(
                parent,
                format!(
                    "`expect` has several matchers ({}); use one per step",
                    many.join(", ")
                ),
            );
            return None;
        }
    };
    let matcher_path = join(parent, key);
    let value = &map[key];
    let flag = |ctx: &mut Ctx, matcher: Matcher| {
        if value == &Value::Bool(true) {
            Some(matcher)
        } else {
            ctx.err(&matcher_path, "expected `true`");
            None
        }
    };
    let matcher = match key {
        "toExist" => flag(ctx, Matcher::ToExist),
        "toBeVisible" => flag(ctx, Matcher::ToBeVisible),
        "toBeHidden" => flag(ctx, Matcher::ToBeHidden),
        "toBeChecked" => flag(ctx, Matcher::ToBeChecked),
        "toHaveText" => parse_text_match(ctx, value, &matcher_path).map(Matcher::ToHaveText),
        "toHaveValue" => parse_text_match(ctx, value, &matcher_path).map(Matcher::ToHaveValue),
        "toContainText" => string_value(ctx, value, &matcher_path).map(Matcher::ToContainText),
        "toHaveCount" => match value.as_u64() {
            Some(count) => Some(Matcher::ToHaveCount(count)),
            None => {
                ctx.err(&matcher_path, "expected non-negative integer");
                None
            }
        },
        "toMatchAriaSnapshot" => {
            parse_snapshot_name(ctx, value, &matcher_path, true).map(Matcher::ToMatchAriaSnapshot)
        }
        "toHaveScreenshot" => {
            let name = parse_snapshot_name(ctx, value, &matcher_path, true);
            let options = parse_screenshot_options(ctx, map, parent);
            match (name, options) {
                (Some(name), Some(mut options)) => {
                    options.name = name;
                    Some(Matcher::ToHaveScreenshot(options))
                }
                _ => None,
            }
        }
        _ => unreachable!("matcher keys are exhaustive"),
    };
    if key != "toHaveScreenshot" {
        for option in SCREENSHOT_OPTION_KEYS {
            if map.contains_key(*option) {
                ctx.err(&join(parent, option), "only valid with `toHaveScreenshot`");
            }
        }
    }
    if not && matches!(key, "toMatchAriaSnapshot" | "toHaveScreenshot") {
        ctx.err(
            &join(parent, "not"),
            format!("`not` is not supported with `{key}`"),
        );
        return None;
    }
    Some(Expectation {
        target: target?,
        matcher: matcher?,
        not,
        soft,
    })
}

/// Device names like `CON` or `com1.x` cannot be files on Windows, whatever the extension.
fn is_reserved_on_windows(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
/// Snapshot names become file names, so keep them to a safe character set.
fn parse_snapshot_name(
    ctx: &mut Ctx,
    value: &Value,
    path: &str,
    allow_auto: bool,
) -> Option<Option<String>> {
    match value {
        Value::Bool(true) if allow_auto => Some(None),
        Value::String(name) => {
            let valid = !name.is_empty()
                && !name.starts_with('.')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            if !valid {
                ctx.err(
                    path,
                    "snapshot names may only use letters, digits, `-`, `_` and `.`",
                );
                None
            } else if is_reserved_on_windows(name) {
                ctx.err(path, format!("`{name}` is a reserved file name on Windows"));
                None
            } else {
                Some(Some(name.clone()))
            }
        }
        other => {
            let expected = if allow_auto {
                "`true` or a snapshot name"
            } else {
                "a snapshot name"
            };
            ctx.err(
                path,
                format!("expected {expected}, found {}", type_name(other)),
            );
            None
        }
    }
}

fn parse_screenshot_options(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    parent: &str,
) -> Option<ScreenshotMatch> {
    let mut ok = true;
    let max_diff_pixels = get_u64(ctx, map, "maxDiffPixels", parent, 0);
    if map.contains_key("maxDiffPixels") && max_diff_pixels.is_none() {
        ok = false;
    }
    let mut ratio = |ctx: &mut Ctx, key: &str| match map.get(key) {
        None => None,
        Some(value) => match value.as_f64() {
            Some(number) if (0.0..=1.0).contains(&number) => Some(number),
            _ => {
                ctx.err(&join(parent, key), "expected a number between 0 and 1");
                ok = false;
                None
            }
        },
    };
    let max_diff_pixel_ratio = ratio(ctx, "maxDiffPixelRatio");
    let threshold = ratio(ctx, "threshold").unwrap_or(DEFAULT_SCREENSHOT_THRESHOLD);
    let mut mask = Vec::new();
    if let Some(value) = map.get("mask") {
        let mask_path = join(parent, "mask");
        match value.as_array() {
            Some(items) => {
                for (i, item) in items.iter().enumerate() {
                    match parse_locator(ctx, item, &format!("{mask_path}[{i}]")) {
                        Some(locator) => mask.push(locator),
                        None => ok = false,
                    }
                }
            }
            None => {
                ctx.err(&mask_path, "expected array of locators");
                ok = false;
            }
        }
    }
    ok.then_some(ScreenshotMatch {
        name: None,
        max_diff_pixels,
        max_diff_pixel_ratio,
        threshold,
        mask,
    })
}

fn parse_text_match(ctx: &mut Ctx, value: &Value, path: &str) -> Option<TextMatch> {
    match value {
        Value::String(text) => Some(TextMatch::Exact(text.clone())),
        Value::Object(map) => {
            check_keys(ctx, map, path, &["pattern"], "in a text pattern");
            if !map.contains_key("pattern") {
                ctx.err(path, "expected `{ \"pattern\": \"...\" }`");
                return None;
            }
            let pattern = get_string(ctx, map, "pattern", path)?;
            match Regex::new(&pattern) {
                Ok(regex) => Some(TextMatch::Pattern(regex)),
                Err(error) => {
                    ctx.err(
                        &join(path, "pattern"),
                        format!("invalid regular expression: {error}"),
                    );
                    None
                }
            }
        }
        other => {
            ctx.err(
                path,
                format!(
                    "expected string or {{ \"pattern\": ... }}, found {}",
                    type_name(other)
                ),
            );
            None
        }
    }
}

/// A string or `{ "pattern" }` that names something; an empty string is a mistake.
fn parse_name_match(ctx: &mut Ctx, value: &Value, path: &str) -> Option<TextMatch> {
    if value.as_str().is_some_and(|text| text.trim().is_empty()) {
        ctx.err(path, "must not be empty");
        return None;
    }
    parse_text_match(ctx, value, path)
}

fn parse_telemetry(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    action: &str,
    raw: &Value,
    parent: &str,
    path: &str,
) -> Option<TelemetryExpectation> {
    let not = get_bool(ctx, map, "not", parent);
    let soft = get_bool(ctx, map, "soft", parent);
    let count = get_u64(ctx, map, "count", parent, 0);
    if not && count.is_some() {
        ctx.err(parent, "use either `not` or `count`, not both");
        return None;
    }
    let attributes = parse_attributes(ctx, map, parent);
    match action {
        "expectIpc" => {
            let command = parse_name_match(ctx, raw, path);
            let status = match map.get("status") {
                None => Some(None),
                Some(Value::String(status)) if status == "ok" => Some(Some(IpcStatus::Ok)),
                Some(Value::String(status)) if status == "error" => Some(Some(IpcStatus::Error)),
                Some(_) => {
                    ctx.err(&join(parent, "status"), "expected \"ok\" or \"error\"");
                    None
                }
            };
            let max_ms = get_u64(ctx, map, "maxMs", parent, 1);
            Some(TelemetryExpectation::Ipc {
                command: command?,
                status: status?,
                count,
                max_ms,
                not,
                soft,
            })
        }
        "expectEvent" => Some(TelemetryExpectation::Event {
            name: string_value(ctx, raw, path)?,
            count,
            not,
            soft,
        }),
        "expectLog" => {
            let contains = string_value(ctx, raw, path);
            let level =
                get_string(ctx, map, "level", parent).map(|level| level.to_ascii_lowercase());
            Some(TelemetryExpectation::Log {
                contains: contains?,
                level,
                attributes: attributes?,
                count,
                not,
                soft,
            })
        }
        "expectSpan" => {
            let name = parse_name_match(ctx, raw, path);
            let status = match map.get("status") {
                None => Some(None),
                Some(Value::String(status)) => match status.as_str() {
                    "ok" => Some(Some(SpanStatus::Ok)),
                    "error" => Some(Some(SpanStatus::Error)),
                    "unset" => Some(Some(SpanStatus::Unset)),
                    _ => None,
                },
                Some(_) => None,
            };
            if status.is_none() {
                ctx.err(
                    &join(parent, "status"),
                    "expected \"ok\", \"error\" or \"unset\"",
                );
            }
            Some(TelemetryExpectation::Span {
                name: name?,
                status: status?,
                attributes: attributes?,
                count,
                not,
                soft,
            })
        }
        "expectNoTelemetry" => {
            let needle = needle(parse_name_match(ctx, raw, path)?);
            let kinds = match map.get("in") {
                None => Some(TelemetryKind::ALL.to_vec()),
                Some(Value::Array(items)) if !items.is_empty() => {
                    let in_path = join(parent, "in");
                    let mut kinds = Vec::new();
                    let mut ok = true;
                    for (i, item) in items.iter().enumerate() {
                        match TelemetryKind::ALL
                            .into_iter()
                            .find(|kind| item.as_str() == Some(kind.as_str()))
                        {
                            Some(kind) if !kinds.contains(&kind) => kinds.push(kind),
                            Some(_) => {}
                            None => {
                                ctx.err(
                                    &format!("{in_path}[{i}]"),
                                    "expected \"logs\", \"spans\", \"ipc\" or \"events\"",
                                );
                                ok = false;
                            }
                        }
                    }
                    ok.then_some(kinds)
                }
                Some(_) => {
                    ctx.err(
                        &join(parent, "in"),
                        "expected a non-empty array of \"logs\", \"spans\", \"ipc\" or \"events\"",
                    );
                    None
                }
            };
            Some(TelemetryExpectation::NoTelemetry {
                needle,
                kinds: kinds?,
                soft,
            })
        }
        "expectNoErrors" => {
            if raw != &Value::Bool(true) {
                ctx.err(path, "expected `true`");
                return None;
            }
            let mut ignore = Vec::new();
            if let Some(value) = map.get("ignore") {
                let ignore_path = join(parent, "ignore");
                match value.as_array() {
                    Some(items) => {
                        for (i, item) in items.iter().enumerate() {
                            ignore.extend(string_value(ctx, item, &format!("{ignore_path}[{i}]")));
                        }
                    }
                    None => ctx.err(&ignore_path, "expected array of strings"),
                }
            }
            Some(TelemetryExpectation::NoErrors { ignore, soft })
        }
        _ => unreachable!("telemetry actions are exhaustive"),
    }
}

/// `attributes: { "key": value | { "pattern": "regex" } }`; `None` when invalid.
fn parse_attributes(
    ctx: &mut Ctx,
    map: &Map<String, Value>,
    parent: &str,
) -> Option<Vec<AttributeMatch>> {
    let Some(value) = map.get("attributes") else {
        return Some(Vec::new());
    };
    let path = join(parent, "attributes");
    let Value::Object(entries) = value else {
        ctx.err(&path, "expected an object of attribute names to values");
        return None;
    };
    let mut ok = true;
    let mut matches = Vec::new();
    for (key, expected) in entries {
        let value = match expected {
            Value::Object(object) if object.contains_key("pattern") => {
                match parse_text_match(ctx, expected, &join(&path, key)) {
                    Some(TextMatch::Pattern(pattern)) => AttributeValue::Pattern(pattern),
                    _ => {
                        ok = false;
                        continue;
                    }
                }
            }
            Value::Object(_) | Value::Array(_) | Value::Null => {
                ctx.err(
                    &join(&path, key),
                    "expected a string, number, boolean or { \"pattern\": ... }",
                );
                ok = false;
                continue;
            }
            scalar => AttributeValue::Equals(scalar.clone()),
        };
        matches.push(AttributeMatch {
            key: key.clone(),
            value,
        });
    }
    ok.then_some(matches)
}

/// Parse a standalone locator, e.g. from a CLI flag or MCP tool argument.
pub fn parse_locator_value(value: &Value) -> Result<Locator, Vec<Issue>> {
    let mut ctx = Ctx::default();
    match parse_locator(&mut ctx, value, "") {
        Some(locator) if ctx.issues.is_empty() => Ok(locator),
        _ => Err(ctx.issues),
    }
}

fn parse_locator(ctx: &mut Ctx, value: &Value, path: &str) -> Option<Locator> {
    let map = match value {
        Value::String(css) if css.trim().is_empty() => {
            ctx.err(path, "locator must not be empty");
            return None;
        }
        Value::String(css) => return Some(Locator::Css(css.clone())),
        Value::Object(map) => map,
        other => {
            ctx.err(
                path,
                format!(
                    "expected a CSS selector string or locator object, found {}",
                    type_name(other)
                ),
            );
            return None;
        }
    };
    let before = ctx.issues.len();
    check_keys(ctx, map, path, LOCATOR_KEYS, "in a locator");
    let kinds: Vec<&str> = LOCATOR_KINDS
        .iter()
        .copied()
        .filter(|key| map.contains_key(*key))
        .collect();
    let kind = match kinds.as_slice() {
        [kind] => *kind,
        [] => {
            ctx.err(
                path,
                format!("locator needs one of: {}", LOCATOR_KINDS.join(", ")),
            );
            return None;
        }
        many => {
            ctx.err(
                path,
                format!("locator has several kinds ({}); use one", many.join(", ")),
            );
            return None;
        }
    };
    let value = get_string(ctx, map, kind, path);
    let exact = get_bool(ctx, map, "exact", path);
    let nth = match map.get("nth") {
        None => None,
        Some(value) => match value.as_i64() {
            Some(nth) => Some(nth),
            None => {
                ctx.err(
                    &join(path, "nth"),
                    "expected integer (negative counts from the end)",
                );
                None
            }
        },
    };
    let name = get_string(ctx, map, "name", path);
    if map.contains_key("name") && kind != "role" {
        ctx.err(&join(path, "name"), "only valid with `role`");
    }
    if map.contains_key("exact") && matches!(kind, "css" | "testId") {
        ctx.err(&join(path, "exact"), format!("not valid with `{kind}`"));
    }
    if map.contains_key("exact") && kind == "role" && !map.contains_key("name") {
        ctx.err(&join(path, "exact"), "requires `name`");
    }
    if ctx.issues.len() > before {
        return None;
    }
    let value = value?;
    Some(match kind {
        "css" => match nth {
            Some(nth) => Locator::CssNth { css: value, nth },
            None => Locator::Css(value),
        },
        "role" => Locator::Role {
            role: value,
            name,
            exact,
            nth,
        },
        "text" => Locator::Text {
            text: value,
            exact,
            nth,
        },
        "testId" => Locator::TestId { id: value, nth },
        "label" => Locator::Label {
            label: value,
            exact,
            nth,
        },
        "placeholder" => Locator::Placeholder {
            placeholder: value,
            exact,
            nth,
        },
        _ => unreachable!("locator kinds are exhaustive"),
    })
}

/// Default human-readable title for a step.
pub fn describe(kind: &StepKind) -> String {
    match kind {
        StepKind::Click {
            target,
            button: None,
        } => format!("click {target}"),
        StepKind::Click {
            target,
            button: Some(button),
        } => format!("{}-click {target}", button.as_str()),
        StepKind::Dblclick(target) => format!("double-click {target}"),
        StepKind::Fill { target, value } => format!("fill {target} with {value:?}"),
        StepKind::Type { target, value } => format!("type {value:?} into {target}"),
        StepKind::Press {
            key,
            target: Some(target),
        } => format!("press {key} on {target}"),
        StepKind::Press { key, target: None } => format!("press {key}"),
        StepKind::Paste(paste) => {
            let mut parts = Vec::new();
            if let Some(text) = &paste.text {
                parts.push(format!("{:?}", clip(text, 40)));
            }
            if paste.html.is_some() {
                parts.push("html".to_string());
            }
            match paste.files.len() {
                0 => {}
                1 => parts.push("1 file".to_string()),
                n => parts.push(format!("{n} files")),
            }
            match &paste.target {
                Some(target) => format!("paste {} into {target}", parts.join(" + ")),
                None => format!("paste {}", parts.join(" + ")),
            }
        }
        StepKind::Hover(target) => format!("hover {target}"),
        StepKind::Drag { source, to } => format!("drag {source} to {to}"),
        StepKind::Select { target, values } => format!("select {} in {target}", values.join(", ")),
        StepKind::Check(target) => format!("check {target}"),
        StepKind::Uncheck(target) => format!("uncheck {target}"),
        StepKind::WaitFor { target, state } => {
            format!("wait for {target} to be {}", state.as_str())
        }
        StepKind::Evaluate(expression) => format!("evaluate {}", clip(expression, 60)),
        StepKind::Reload => "reload page".to_string(),
        StepKind::Sleep(ms) => format!("sleep {ms}ms"),
        StepKind::Screenshot { name, .. } => format!("screenshot {name}"),
        StepKind::Gate(gate) => format!(
            "gate {}",
            gate.get("name")
                .and_then(Value::as_str)
                .unwrap_or("manual step")
        ),
        StepKind::Expect(expectation) => {
            let not = if expectation.not { "not " } else { "" };
            let matcher = match &expectation.matcher {
                Matcher::ToExist => "to exist".to_string(),
                Matcher::ToBeVisible => "to be visible".to_string(),
                Matcher::ToBeHidden => "to be hidden".to_string(),
                Matcher::ToBeChecked => "to be checked".to_string(),
                Matcher::ToHaveText(text) => format!("to have text {text}"),
                Matcher::ToHaveValue(text) => format!("to have value {text}"),
                Matcher::ToContainText(text) => format!("to contain text {text:?}"),
                Matcher::ToHaveCount(count) => format!("to have count {count}"),
                Matcher::ToMatchAriaSnapshot(_) => "to match ARIA snapshot".to_string(),
                Matcher::ToHaveScreenshot(_) => "to match screenshot".to_string(),
            };
            format!("expect {} {not}{matcher}", expectation.target)
        }
        StepKind::Run(command) => {
            let line = std::iter::once(command.run.as_str())
                .chain(command.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            format!("run {}", clip(&line, 60))
        }
        StepKind::Dialog(dialog) => {
            let answer = match &dialog.respond {
                Value::Null => "cancel".to_string(),
                Value::String(text) => clip(text, 60),
                Value::Array(items) => format!("{} paths", items.len()),
                other => other.to_string(),
            };
            format!("answer {} dialog with {answer}", dialog.kind)
        }
        StepKind::ExpectValue(expectation) => {
            let not = if expectation.not { "not " } else { "" };
            format!(
                "expect value of {} {not}{}",
                clip(&expectation.expression, 60),
                expectation.matcher
            )
        }
        StepKind::Telemetry(expectation) => {
            let times = |verb: &str, not: bool, count: Option<u64>| match count {
                _ if not => format!("not {verb}"),
                Some(0) => format!("not {verb}"),
                Some(1) => format!("{verb} once"),
                Some(n) => format!("{verb} {n} times"),
                None => verb.to_string(),
            };
            match expectation {
                TelemetryExpectation::Ipc {
                    command,
                    status,
                    count,
                    not,
                    ..
                } => {
                    let status = match status {
                        Some(IpcStatus::Ok) => " ok",
                        Some(IpcStatus::Error) => " with an error",
                        None => "",
                    };
                    format!(
                        "expect IPC `{}` {}",
                        name_label(command),
                        times(&format!("called{status}"), *not, *count)
                    )
                }
                TelemetryExpectation::Event {
                    name, count, not, ..
                } => format!("expect event `{name}` {}", times("emitted", *not, *count)),
                TelemetryExpectation::Log {
                    contains,
                    level,
                    count,
                    not,
                    ..
                } => {
                    let level = level
                        .as_deref()
                        .map(|l| format!(" {l}"))
                        .unwrap_or_default();
                    format!(
                        "expect{level} log containing {contains:?} {}",
                        times("logged", *not, *count)
                    )
                }
                TelemetryExpectation::NoErrors { .. } => "expect no errors".to_string(),
                TelemetryExpectation::Span {
                    name,
                    status,
                    count,
                    not,
                    ..
                } => {
                    let status = status
                        .map(|status| format!(" with status {}", status.as_str()))
                        .unwrap_or_default();
                    format!(
                        "expect span `{}`{status} {}",
                        name_label(name),
                        times("recorded", *not, *count)
                    )
                }
                TelemetryExpectation::NoTelemetry { needle, .. } => {
                    format!("expect no telemetry containing {needle}")
                }
            }
        }
    }
}

fn clip(value: &str, max: usize) -> String {
    let line = value.lines().next().unwrap_or_default();
    if line.chars().count() > max || value.lines().count() > 1 {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ParsedFile {
        parse_str(Path::new("greet.drill.json"), text)
    }

    fn messages(text: &str) -> Vec<String> {
        parse(text).issues.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_full_suite() {
        let parsed = parse(
            r##"{
              "$schema": "https://auditaur.dev/schemas/drill.schema.json",
              "tags": ["@smoke"],
              "expectTimeoutMs": 2000,
              "beforeEach": [{ "reload": true }],
              "tests": [
                {
                  "name": "greets",
                  "tags": ["fast"],
                  "retries": 1,
                  "steps": [
                    { "fill": { "testId": "name" }, "value": "Seth" },
                    { "click": { "role": "button", "name": "Greet" } },
                    { "press": "Enter", "target": "#name" },
                    { "select": "#pick", "values": ["a", "b"] },
                    { "waitFor": { "text": "Hello", "nth": -1 }, "state": "visible" },
                    { "expect": "#output", "toContainText": "Hello, Seth" },
                    { "expect": "#output", "toHaveText": { "pattern": "^Hello" }, "soft": true },
                    { "expect": "#toast", "not": true, "toBeVisible": true },
                    { "expect": "body", "toHaveScreenshot": "home", "maxDiffPixels": 10, "mask": ["#clock"] },
                    { "expect": "main", "toMatchAriaSnapshot": true },
                    { "expectIpc": "greet", "status": "ok", "maxMs": 200 },
                    { "expectEvent": "app:greeted", "count": 1 },
                    { "expectLog": "boom", "not": true },
                    { "expectNoErrors": true, "ignore": ["ResizeObserver"] },
                    { "gate": { "name": "Approve", "instructions": "Click approve", "manualContinue": true } },
                    { "name": "custom title", "sleep": 10 }
                  ]
                },
                { "name": "later", "fixme": "needs auth" }
              ]
            }"##,
        );
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let suite = parsed.suite.unwrap();
        assert_eq!(suite.suite, "greet");
        assert_eq!(suite.tags, ["smoke"]);
        assert_eq!(suite.before_each.len(), 1);
        let test = &suite.tests[0];
        assert_eq!(test.steps.len(), 16);
        assert_eq!(test.steps[1].title, "click role=button[name=\"Greet\"i]");
        assert_eq!(
            test.steps[4].title,
            "wait for text=\"Hello\"i >> nth=-1 to be visible"
        );
        assert_eq!(test.steps[7].title, "expect #toast not to be visible");
        assert_eq!(test.steps[15].title, "custom title");
        assert_eq!(suite.tests[1].mode, TestMode::Fixme);
        assert_eq!(suite.tests[1].mode_reason.as_deref(), Some("needs auth"));
    }

    #[test]
    fn parses_dialog_steps() {
        let parsed = parse(
            r##"{ "tests": [{ "name": "t", "steps": [
                { "dialog": "save", "respond": "${OUT}/a.docx", "expectOptions": { "title": "Export" } },
                { "dialog": "open", "respond": ["a.md", "b.md"] },
                { "dialog": "open", "respond": null },
                { "dialog": "confirm", "respond": false },
                { "dialog": "message" }
            ] }] }"##,
        );
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let steps = &parsed.suite.unwrap().tests[0].steps;
        let titles: Vec<_> = steps.iter().map(|step| step.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "answer save dialog with ${OUT}/a.docx",
                "answer open dialog with 2 paths",
                "answer open dialog with cancel",
                "answer confirm dialog with false",
                "answer message dialog with Ok",
            ]
        );
        let StepKind::Dialog(save) = &steps[0].kind else {
            panic!("not a dialog step");
        };
        assert_eq!(save.kind, "save");
        assert_eq!(
            save.expect_options,
            Some(serde_json::json!({ "title": "Export" }))
        );
    }

    #[test]
    fn rejects_bad_dialog_steps() {
        let issues = messages(
            r##"{ "tests": [{ "name": "t", "steps": [
                { "dialog": "print", "respond": true },
                { "dialog": "save" },
                { "dialog": "open", "respond": [] },
                { "dialog": "ask", "respond": "yes" },
                { "dialog": "save", "respond": "a", "expectOptions": "Export" }
            ] }] }"##,
        );
        let expected = [
            "unknown dialog \"print\"",
            "`dialog: save` requires `respond` (use null to cancel)",
            "`open` dialogs respond with a path, a non-empty array of paths, or null",
            "`ask` dialogs respond with true or false, found string",
            "expected an object of the dialog options to match, found string",
        ];
        for needle in expected {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
    }

    #[test]
    fn reports_unknown_keys_with_suggestions() {
        let issues =
            messages(r##"{ "tests": [{ "name": "a", "steps": [{ "clik": "#x" }] }], "tag": [] }"##);
        assert!(
            issues
                .iter()
                .any(|m| m.contains("unknown key `tag`") && m.contains("did you mean `tags`")),
            "{issues:?}"
        );
        assert!(
            issues.iter().any(|m| m.contains("did you mean `click`")),
            "{issues:?}"
        );
    }

    #[test]
    fn validates_press_chords() {
        for ok in [
            "Enter",
            "+",
            "Control+s",
            "ControlOrMeta+Shift+K",
            "Shift++",
        ] {
            let doc =
                format!(r#"{{ "tests": [{{ "name": "a", "steps": [{{ "press": "{ok}" }}] }}] }}"#);
            assert!(messages(&doc).is_empty(), "{ok}: {:?}", messages(&doc));
        }
        let issues = messages(
            r#"{ "tests": [{ "name": "a", "steps": [{ "press": "Ctrl+s" }, { "press": "Control+" }] }] }"#,
        );
        assert!(issues[0].contains("unknown modifier `Ctrl`"), "{issues:?}");
        assert!(issues[1].contains("missing key after `+`"), "{issues:?}");
    }

    #[test]
    fn reports_misplaced_known_keys() {
        let issues = messages(
            r##"{ "tests": [{ "name": "a.b", "steps": [{ "click": "#x", "value": "y" }] }] }"##,
        );
        assert_eq!(
            issues,
            ["tests[0] (\"a.b\").steps[0].value: `value` is not valid with `click`"]
        );
    }

    #[test]
    fn collects_every_issue() {
        let issues = messages(
            r##"{ "tests": [
                { "name": "a", "steps": [{ "fill": "#x" }, { "click": "#a", "hover": "#b" }] },
                { "name": "a", "steps": [{ "expect": "#x" }] },
                { "steps": [] }
            ] }"##,
        );
        for needle in [
            "`fill` requires `value`",
            "several actions",
            "duplicate test name",
            "needs one matcher",
            "test `name` is required",
        ] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
    }

    #[test]
    fn validates_locators() {
        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "click": { "css": "#a", "role": "button" } },
                { "click": { "testId": "a", "name": "x" } },
                { "click": { "rol": "button" } },
                { "click": "" },
                { "click": { "role": "button", "exact": true } }
            ] }] }"##,
        );
        for needle in [
            "several kinds",
            "name: only valid with `role`",
            "did you mean `role`",
            "locator must not be empty",
            "exact: requires `name`",
        ] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
    }

    #[test]
    fn validates_matchers_and_telemetry() {
        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "expect": "#a", "toHaveText": { "pattern": "(" } },
                { "expect": "#a", "toBeVisible": true, "maxDiffPixels": 3 },
                { "expect": "#a", "toHaveScreenshot": "../x" },
                { "expect": "#a", "toMatchAriaSnapshot": "Com1.page" },
                { "expect": "#a", "toHaveScreenshot": true, "not": true },
                { "expectIpc": "greet", "not": true, "count": 2 },
                { "expectIpc": "greet", "status": "fine" },
                { "screenshot": true }
            ] }] }"##,
        );
        for needle in [
            "invalid regular expression",
            "only valid with `toHaveScreenshot`",
            "snapshot names",
            "reserved file name on Windows",
            "`not` is not supported",
            "either `not` or `count`",
            "\"ok\" or \"error\"",
            "expected a snapshot name",
        ] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
    }

    #[test]
    fn validates_test_modes_and_hooks() {
        let issues = messages(
            r##"{ "setup": [{ "name": "seed" }], "tests": [
                { "name": "a", "skip": true, "only": true },
                { "name": "b", "skip": 3 },
                { "name": "c", "skip": "later" }
            ] }"##,
        );
        assert!(
            issues
                .iter()
                .any(|m| m.starts_with("setup[0]: invalid hook")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|m| m.contains("only one of `only`, `skip` and `fixme`")),
            "{issues:?}"
        );
        assert!(
            issues.iter().any(|m| m.contains("reason string")),
            "{issues:?}"
        );
        assert!(!issues.iter().any(|m| m.contains("\"c\"")), "{issues:?}");
    }

    #[test]
    fn validates_telemetry_assertions() {
        let valid = messages(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "expectIpc": { "pattern": "^plugin:chat\\|" } },
                { "expectLog": "sent", "attributes": { "harness": "prompty", "n": 2, "id": { "pattern": "^u" } } },
                { "expectSpan": "chat.send", "status": "ok", "attributes": { "harness": "prompty" }, "count": 1 },
                { "expectNoTelemetry": "ada@example.com", "in": ["logs", "ipc"] },
                { "expectNoTelemetry": { "pattern": "codename-\\w+" } }
            ] }] }"##,
        );
        assert!(valid.is_empty(), "{valid:?}");
        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "expectIpc": "" },
                { "expectSpan": "s", "status": "done" },
                { "expectSpan": "s", "attributes": { "k": [1] } },
                { "expectNoTelemetry": "x", "in": [] },
                { "expectNoTelemetry": "x", "in": ["metrics"] },
                { "expectNoTelemetry": "x", "count": 1 },
                { "expectIpc": "x", "attributes": {} }
            ] }] }"##,
        );
        for needle in [
            "expectIpc: must not be empty",
            "steps[1].status: expected \"ok\", \"error\" or \"unset\"",
            "attributes.k",
            "in: expected a non-empty array",
            "in[0]: expected",
            "`count` is not valid with `expectNoTelemetry`",
            "`attributes` is not valid with `expectIpc`",
        ] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
    }

    #[test]
    fn validates_value_expectations() {
        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "expectValue": "1" },
                { "expectValue": "1", "toEqual": 1, "toMatch": "x" },
                { "expectValue": "1", "toMatch": "(" },
                { "expectValue": " ", "toEqual": 1 },
                { "expectValue": "1", "toEqual": 1, "count": 1 },
                { "expectValue": "1", "toEqual": 1, "expectTimeoutMs": 9000 }
            ] }] }"##,
        );
        for needle in [
            "`expectValue` needs one matcher",
            "several matchers",
            "invalid regular expression",
            "expectValue: must not be empty",
            "`count` is not valid with `expectValue`",
            "steps[5].expectTimeoutMs: `expectTimeoutMs` is a suite setting; set this step's retry window with `timeoutMs`",
        ] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
        assert!(
            !issues
                .iter()
                .any(|m| m.contains("`expectTimeoutMs` is not valid")),
            "{issues:?}"
        );
    }

    #[test]
    fn parses_and_validates_run_steps() {
        let parsed = parse(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "run": "git", "args": ["log", "-1"], "cwd": "fixture", "exitCode": 2, "stdout": { "pattern": "^a" }, "stderr": "warn" }
            ] }] }"##,
        );
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let suite = parsed.suite.as_ref().unwrap();
        let StepKind::Run(command) = &suite.tests[0].steps[0].kind else {
            panic!("not a run step");
        };
        assert_eq!(command.args, ["log", "-1"]);
        assert_eq!(command.cwd.as_deref(), Some("fixture"));
        assert_eq!(command.exit_code, 2);
        assert!(command.stdout.as_ref().unwrap().is_in("abc"));
        assert!(command.stderr.as_ref().unwrap().is_in("a warning"));
        let hook = command.hook("t", 5);
        assert_eq!(hook["timeoutMs"], 5);
        assert_eq!(hook["cwd"], "fixture");

        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "run": " " },
                { "run": "git", "args": "log" },
                { "run": "git", "exitCode": 1.5 },
                { "run": "git", "stdout": "" },
                { "run": "git", "soft": true }
            ] }] }"##,
        );
        for needle in [
            "run: must not be empty",
            "args",
            "exitCode",
            "stdout",
            "`soft` is not valid with `run`",
        ] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
    }

    #[test]
    fn parses_and_validates_suite_env() {
        let parsed = parse(
            r##"{ "env": { "B": "2", "A": "x=y" }, "tests": [{ "name": "a", "steps": [{ "click": "#a" }] }] }"##,
        );
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let env = &parsed.suite.as_ref().unwrap().env;
        assert!(env.contains(&("A".into(), "x=y".into())), "{env:?}");
        assert_eq!(env.len(), 2);
        let issues = messages(
            r##"{ "env": { "A=B": "1", "": "2", "N": 3 }, "tests": [{ "name": "a", "steps": [{ "click": "#a" }] }] }"##,
        );
        for needle in ["no `=`", "must not be empty", "expected string, found"] {
            assert!(
                issues.iter().any(|m| m.contains(needle)),
                "{needle}: {issues:?}"
            );
        }
        assert!(messages(r##"{ "env": [], "tests": [] }"##)
            .iter()
            .any(|m| m.contains("expected object of strings")));
    }

    #[test]
    fn drag_steps_need_a_drop_target() {
        let ok = parse(
            r##"{ "tests": [{ "name": "a", "steps": [{ "drag": { "role": "button", "name": "Move row 1" }, "to": "#row-3" }] }] }"##,
        );
        assert!(ok.issues.is_empty(), "{:?}", ok.issues);
        let suite = ok.suite.unwrap();
        let step = &suite.tests[0].steps[0];
        assert!(matches!(step.kind, StepKind::Drag { .. }));
        assert!(step.title.starts_with("drag "), "{}", step.title);
        assert!(step.title.contains(" to "), "{}", step.title);
        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [{ "drag": "#a" }, { "click": "#a", "to": "#b" }] }] }"##,
        );
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].contains("`drag` requires `to`"), "{issues:?}");
        assert!(issues[1].contains("`to`"), "{issues:?}");
    }

    #[test]
    fn paste_steps_take_text_or_clipboard_content() {
        let ok = parse(
            r##"{ "tests": [{ "name": "a", "steps": [
                { "paste": "hello" },
                { "paste": { "text": "a", "html": "<b>a</b>", "files": ["x.png", "y.png"] }, "target": "#editor" }
            ] }] }"##,
        );
        assert!(ok.issues.is_empty(), "{:?}", ok.issues);
        let suite = ok.suite.unwrap();
        let steps = &suite.tests[0].steps;
        assert_eq!(steps[0].title, r#"paste "hello""#);
        assert_eq!(steps[1].title, r#"paste "a" + html + 2 files into #editor"#);
        let StepKind::Paste(paste) = &steps[1].kind else {
            panic!("expected paste, got {:?}", steps[1].kind);
        };
        assert_eq!(paste.files, ["x.png", "y.png"]);
        let issues = messages(
            r##"{ "tests": [{ "name": "a", "steps": [{ "paste": {} }, { "paste": { "txt": "a" } }, { "paste": { "files": [] } }] }] }"##,
        );
        assert!(
            issues[0].contains("needs `text`, `html`, or `files`"),
            "{issues:?}"
        );
        assert!(issues[1].contains("did you mean `text`"), "{issues:?}");
        assert!(issues[2].contains("non-empty array"), "{issues:?}");
    }

    #[test]
    fn serial_suites_reject_retries() {
        let ok = parse(r##"{ "serial": true, "tests": [{ "name": "a", "skip": true }] }"##);
        assert!(ok.issues.is_empty());
        assert!(ok.suite.unwrap().serial);
        let issues = messages(
            r##"{ "serial": true, "retries": 1, "tests": [{ "name": "a", "retries": 2, "skip": true }] }"##,
        );
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].starts_with("retries: is not valid in a serial suite"));
        assert!(issues[1].starts_with("tests[0].retries: is not valid in a serial suite"));
    }

    #[test]
    fn value_matchers_compare_like_javascript() {
        let equal = ValueMatcher::ToEqual(serde_json::json!({ "n": 1.0, "xs": [1, "a"] }));
        assert!(equal.matches(&serde_json::json!({ "xs": [1, "a"], "n": 1 })));
        assert!(!equal.matches(&serde_json::json!({ "n": 1 })));
        let contain = ValueMatcher::ToContain(serde_json::json!("ell"));
        assert!(contain.matches(&serde_json::json!("hello")));
        assert!(!contain.matches(&serde_json::json!(["hello"])));
        let element = ValueMatcher::ToContain(serde_json::json!({ "id": 2 }));
        assert!(element.matches(&serde_json::json!([{ "id": 1 }, { "id": 2.0 }])));
        let pattern = ValueMatcher::ToMatch(Regex::new("^v\\d+$").unwrap());
        assert!(pattern.matches(&serde_json::json!("v12")));
        assert!(!pattern.matches(&serde_json::json!(12)));
        let big = ValueMatcher::ToEqual(serde_json::json!(9_007_199_254_740_992_u64));
        assert!(!big.matches(&serde_json::json!(9_007_199_254_740_993_u64)));
        assert!(!name_matches(
            &TextMatch::Exact("chat send".into()),
            "chat  send"
        ));
    }

    #[test]
    fn rejects_invalid_json_and_missing_tests() {
        assert!(messages("{").iter().any(|m| m.contains("invalid JSON")));
        assert_eq!(messages("{}"), ["tests: is required"]);
        assert_eq!(
            messages(r##"{ "tests": [] }"##),
            ["tests: must contain at least one test"]
        );
        assert!(
            parse("\u{feff}{ \"tests\": [{ \"name\": \"a\", \"skip\": true }] }")
                .issues
                .is_empty()
        );
    }

    #[test]
    fn text_matching_normalizes_whitespace() {
        assert!(TextMatch::Exact("Hello  world".into()).matches(" Hello\nworld "));
        assert!(!TextMatch::Exact("Hello".into()).matches("Hello world"));
        assert!(TextMatch::Pattern(Regex::new("^Hel+o").unwrap()).matches("Hello"));
    }

    #[test]
    fn default_suite_name_strips_extension() {
        assert_eq!(default_suite_name(Path::new("a/login.drill.json")), "login");
        assert_eq!(default_suite_name(Path::new("a/login.json")), "login");
    }
}
