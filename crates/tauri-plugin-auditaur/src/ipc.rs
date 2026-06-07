use serde::Deserialize;

/// Reserved Tauri invoke argument used by Auditaur's experimental IPC trace bridge.
pub const IPC_CONTEXT_ARG: &str = "auditaurTraceContext";

/// W3C trace context carried from Auditaur's frontend invoke wrapper.
///
/// Add this as an optional `auditaur_trace_context` argument on Tauri commands
/// that should continue frontend invoke traces in backend `tracing` spans.
#[derive(Debug, Clone, Deserialize)]
pub struct IpcTraceContext {
    traceparent: Option<String>,
}

impl IpcTraceContext {
    /// Returns the valid W3C `traceparent` value, if one was provided.
    pub fn traceparent(&self) -> Option<&str> {
        self.traceparent
            .as_deref()
            .filter(|value| is_w3c_traceparent(value))
    }
}

/// Extracts a `traceparent` field value for use in `#[tracing::instrument]`.
pub fn ipc_traceparent(context: Option<&IpcTraceContext>) -> &str {
    context
        .and_then(IpcTraceContext::traceparent)
        .unwrap_or_default()
}

fn is_w3c_traceparent(value: &str) -> bool {
    let mut parts = value.split('-');
    let version = parts.next();
    let trace_id = parts.next();
    let parent_span_id = parts.next();
    let flags = parts.next();
    parts.next().is_none()
        && version.is_some_and(|value| is_hex_len(value, 2))
        && trace_id.is_some_and(|value| is_hex_len(value, 32))
        && parent_span_id.is_some_and(|value| is_hex_len(value, 16))
        && flags.is_some_and(|value| is_hex_len(value, 2))
}

fn is_hex_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{ipc_traceparent, IpcTraceContext};
    use serde_json::json;

    #[test]
    fn accepts_valid_traceparent() {
        let context = IpcTraceContext {
            traceparent: Some(
                "00-00112233445566778899aabbccddeeff-0123456789abcdef-01".to_string(),
            ),
        };

        assert_eq!(
            ipc_traceparent(Some(&context)),
            "00-00112233445566778899aabbccddeeff-0123456789abcdef-01"
        );
    }

    #[test]
    fn ignores_invalid_traceparent() {
        let context = IpcTraceContext {
            traceparent: Some("not-a-traceparent".to_string()),
        };

        assert_eq!(ipc_traceparent(Some(&context)), "");
        assert_eq!(ipc_traceparent(None), "");
    }

    #[test]
    fn deserializes_missing_or_extra_fields_safely() {
        let missing: IpcTraceContext = serde_json::from_value(json!({})).unwrap();
        let extra: IpcTraceContext = serde_json::from_value(json!({
            "traceparent": "00-00112233445566778899aabbccddeeff-0123456789abcdef-01",
            "future": true
        }))
        .unwrap();

        assert_eq!(ipc_traceparent(Some(&missing)), "");
        assert_eq!(
            ipc_traceparent(Some(&extra)),
            "00-00112233445566778899aabbccddeeff-0123456789abcdef-01"
        );
    }
}
