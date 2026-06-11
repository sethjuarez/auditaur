use auditaur_core::model::{
    FrontendError, LogRecord, SpanEventRecord, SpanRecord, TauriEventRecord, TauriIpcCall,
};
use serde::{Deserialize, Serialize};

pub mod otlp {
    pub const RECEIVER_STATUS: &str = "planned";
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OTelBatch {
    pub spans: Vec<SpanRecord>,
    pub span_events: Vec<SpanEventRecord>,
    pub logs: Vec<LogRecord>,
    pub frontend_errors: Vec<FrontendError>,
    pub tauri_ipc_calls: Vec<TauriIpcCall>,
    pub tauri_events: Vec<TauriEventRecord>,
}
