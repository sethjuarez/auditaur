use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub session_name: Option<String>,
    pub service_name: String,
    pub service_version: Option<String>,
    pub app_identifier: Option<String>,
    pub pid: Option<i64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub schema_version: i64,
    pub auditaur_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub session_id: String,
    pub timestamp_unix_nanos: i64,
    pub observed_timestamp_unix_nanos: Option<i64>,
    pub severity_text: Option<String>,
    pub severity_number: Option<i64>,
    pub body: Option<String>,
    pub body_json: Option<Value>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub scope_name: Option<String>,
    pub scope_version: Option<String>,
    pub attributes: Value,
    pub source: TelemetrySource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpanRecord {
    pub session_id: String,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: Option<String>,
    pub start_time_unix_nanos: i64,
    pub end_time_unix_nanos: Option<i64>,
    pub status_code: Option<String>,
    pub status_message: Option<String>,
    pub scope_name: Option<String>,
    pub scope_version: Option<String>,
    pub attributes: Value,
    pub source: TelemetrySource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FrontendError {
    pub session_id: String,
    pub timestamp_unix_nanos: i64,
    pub message: String,
    pub stack: Option<String>,
    pub filename: Option<String>,
    pub line_number: Option<i64>,
    pub column_number: Option<i64>,
    pub error_type: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub window_label: Option<String>,
    pub attributes: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TauriIpcCall {
    pub session_id: String,
    pub timestamp_unix_nanos: i64,
    pub duration_ms: Option<f64>,
    pub command: String,
    pub status: String,
    pub error_message: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub window_label: Option<String>,
    pub args_json: Option<Value>,
    pub args_redacted: bool,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TauriEventRecord {
    pub session_id: String,
    pub timestamp_unix_nanos: i64,
    pub event_name: String,
    pub direction: String,
    pub target: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub window_label: Option<String>,
    pub payload_summary: Option<String>,
    pub payload_json: Option<Value>,
    pub payload_redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TauriWindowState {
    pub session_id: String,
    pub timestamp_unix_nanos: i64,
    pub window_label: String,
    pub webview_label: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub focused: Option<bool>,
    pub visible: Option<bool>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub scale_factor: Option<f64>,
    pub attributes: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelemetrySource {
    Frontend,
    Backend,
    Plugin,
    ThirdPartyOtel,
}

impl TelemetrySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Frontend => "frontend",
            Self::Backend => "backend",
            Self::Plugin => "plugin",
            Self::ThirdPartyOtel => "third_party_otel",
        }
    }

    pub fn from_storage(value: &str) -> Self {
        match value {
            "frontend" => Self::Frontend,
            "backend" => Self::Backend,
            "plugin" => Self::Plugin,
            "third_party_otel" => Self::ThirdPartyOtel,
            _ => Self::ThirdPartyOtel,
        }
    }
}
