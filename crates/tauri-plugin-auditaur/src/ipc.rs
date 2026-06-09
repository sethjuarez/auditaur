use serde::Deserialize;

/// Reserved Tauri invoke argument used by Auditaur's experimental IPC trace bridge.
pub const IPC_CONTEXT_ARG: &str = "auditaurTraceContext";
/// Reserved Tauri invoke request header used by Auditaur's IPC trace bridge.
pub const IPC_TRACEPARENT_HEADER: &str = "traceparent";

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

/// Extracts a `traceparent` field value from a Tauri IPC request.
pub fn ipc_traceparent_from_request<'a>(request: &'a tauri::ipc::Request<'a>) -> &'a str {
    ipc_traceparent_from_headers(request.headers())
}

/// Extracts a `traceparent` field value from a Tauri IPC request, falling back to
/// the legacy reserved invoke argument when request headers are unavailable.
pub fn ipc_traceparent_from_request_or_context<'a>(
    request: &'a tauri::ipc::Request<'a>,
    context: Option<&'a IpcTraceContext>,
) -> &'a str {
    let header_traceparent = ipc_traceparent_from_request(request);
    if header_traceparent.is_empty() {
        ipc_traceparent(context)
    } else {
        header_traceparent
    }
}

/// Extracts a `traceparent` field value from Tauri IPC request headers.
pub fn ipc_traceparent_from_headers(headers: &tauri::http::HeaderMap) -> &str {
    headers
        .get(IPC_TRACEPARENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| is_w3c_traceparent(value))
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
    use super::{ipc_traceparent, ipc_traceparent_from_headers, IpcTraceContext};
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

    #[test]
    fn extracts_valid_traceparent_from_ipc_headers() {
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert(
            "traceparent",
            "00-00112233445566778899aabbccddeeff-0123456789abcdef-01"
                .parse()
                .unwrap(),
        );

        assert_eq!(
            ipc_traceparent_from_headers(&headers),
            "00-00112233445566778899aabbccddeeff-0123456789abcdef-01"
        );
    }

    #[test]
    fn ignores_invalid_traceparent_header() {
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert("traceparent", "not-a-traceparent".parse().unwrap());

        assert_eq!(ipc_traceparent_from_headers(&headers), "");
    }
}
