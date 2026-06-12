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
    #[serde(default)]
    pub spans: Vec<SpanRecord>,
    #[serde(default)]
    pub span_events: Vec<SpanEventRecord>,
    #[serde(default)]
    pub logs: Vec<LogRecord>,
    #[serde(default)]
    pub frontend_errors: Vec<FrontendError>,
    #[serde(default)]
    pub tauri_ipc_calls: Vec<TauriIpcCall>,
    #[serde(default)]
    pub tauri_events: Vec<TauriEventRecord>,
}

#[cfg(test)]
mod tests {
    use super::OTelBatch;

    #[test]
    fn missing_batch_arrays_default_to_empty() {
        let batch: OTelBatch = serde_json::from_str("{}").unwrap();

        assert!(batch.spans.is_empty());
        assert!(batch.span_events.is_empty());
        assert!(batch.logs.is_empty());
        assert!(batch.frontend_errors.is_empty());
        assert!(batch.tauri_ipc_calls.is_empty());
        assert!(batch.tauri_events.is_empty());
    }

    #[test]
    fn older_frontend_batch_without_span_events_is_accepted() {
        let batch: OTelBatch = serde_json::from_str(
            r#"{
                "spans": [],
                "logs": [],
                "frontendErrors": [],
                "tauriIpcCalls": [],
                "tauriEvents": []
            }"#,
        )
        .unwrap();

        assert!(batch.span_events.is_empty());
    }
}
