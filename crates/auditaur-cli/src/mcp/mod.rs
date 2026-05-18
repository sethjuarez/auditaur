use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::{
    protocol::TraceSummary,
    storage::{
        FrontendErrorQuery, LogQuery, SpanQuery, TauriEventQuery, TauriIpcQuery, TauriWindowQuery,
    },
};
use serde::Deserialize;
use serde_json::{json, Value};

pub const MCP_COMMAND_NAME: &str = "mcp";
const MAX_LIST_LIMIT: usize = 1_000;
const MAX_TRACE_COMPONENT_LIMIT: usize = 2_000;
const MAX_TOOL_RESPONSE_BYTES: usize = 1_000_000;

pub fn run() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_line(line: &str) -> Option<Value> {
    match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => handle_request(request),
        Err(error) => Some(json_rpc_error(
            Value::Null,
            -32700,
            format!("Parse error: {error}"),
        )),
    }
}

fn handle_request(request: JsonRpcRequest) -> Option<Value> {
    if request.id.is_none() {
        return None;
    }

    let id = request.id.clone().unwrap_or(Value::Null);
    match request.method.as_str() {
        "initialize" => {
            let protocol_version = request
                .params
                .as_ref()
                .and_then(|params| params.get("protocolVersion"))
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05");
            Some(json_rpc_result(
                id,
                json!({
                    "protocolVersion": protocol_version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "auditaur", "version": env!("CARGO_PKG_VERSION") }
                }),
            ))
        }
        "tools/list" => Some(json_rpc_result(id, json!({ "tools": tools() }))),
        "tools/call" => Some(match request.params {
            Some(params) => match call_tool(params) {
                Ok(value) => json_rpc_result(id, tool_result(value, false)),
                Err(error) => json_rpc_result(id, tool_text_result(error.to_string(), true)),
            },
            None => json_rpc_error(id, -32602, "Missing params"),
        }),
        _ => Some(json_rpc_error(
            id,
            -32601,
            format!("Unknown method {}", request.method),
        )),
    }
}

fn call_tool(params: Value) -> Result<Value> {
    let call: ToolCall = serde_json::from_value(params)?;
    let arguments = call.arguments.unwrap_or_default();
    match call.name.as_str() {
        "doctor" => {
            let db = optional_path(&arguments, "db")?;
            let report = crate::commands::doctor::report(db.as_deref());
            Ok(serde_json::to_value(report)?)
        }
        "list_sessions" => {
            let store = open_store(&arguments)?;
            Ok(serde_json::to_value(store.list_sessions(limit(
                &arguments,
                20,
                MAX_LIST_LIMIT,
            ))?)?)
        }
        "list_logs" => {
            let store = open_store(&arguments)?;
            Ok(serde_json::to_value(store.list_logs(&LogQuery {
                session_id: optional_string(&arguments, "sessionId"),
                trace_id: optional_string(&arguments, "traceId"),
                limit: limit(&arguments, 200, MAX_LIST_LIMIT),
            })?)?)
        }
        "list_errors" => {
            let store = open_store(&arguments)?;
            Ok(serde_json::to_value(store.list_frontend_errors(
                &FrontendErrorQuery {
                    session_id: optional_string(&arguments, "sessionId"),
                    trace_id: optional_string(&arguments, "traceId"),
                    limit: limit(&arguments, 200, MAX_LIST_LIMIT),
                },
            )?)?)
        }
        "list_traces" => {
            let store = open_store(&arguments)?;
            let summaries: Vec<TraceSummary> = store.list_trace_summaries(
                optional_string(&arguments, "sessionId").as_deref(),
                limit(&arguments, 100, MAX_LIST_LIMIT),
            )?;
            Ok(serde_json::to_value(summaries)?)
        }
        "get_trace" => {
            let store = open_store(&arguments)?;
            let trace_id = required_string(&arguments, "traceId")?;
            let session_id = optional_string(&arguments, "sessionId");
            Ok(json!({
                "traceId": trace_id,
                "spans": store.list_spans(&SpanQuery {
                    session_id: session_id.clone(),
                    trace_id: Some(trace_id.clone()),
                    limit: limit(&arguments, 500, MAX_TRACE_COMPONENT_LIMIT),
                })?,
                "logs": store.list_logs(&LogQuery {
                    session_id: session_id.clone(),
                    trace_id: Some(trace_id.clone()),
                    limit: limit(&arguments, 500, MAX_TRACE_COMPONENT_LIMIT),
                })?,
                "frontendErrors": store.list_frontend_errors(&FrontendErrorQuery {
                    session_id: session_id.clone(),
                    trace_id: Some(trace_id.clone()),
                    limit: limit(&arguments, 500, MAX_TRACE_COMPONENT_LIMIT),
                })?,
                "tauriIpcCalls": store.list_tauri_ipc_calls(&TauriIpcQuery {
                    session_id: session_id.clone(),
                    trace_id: Some(trace_id.clone()),
                    limit: limit(&arguments, 500, MAX_TRACE_COMPONENT_LIMIT),
                })?,
                "tauriEvents": store.list_tauri_events(&TauriEventQuery {
                    session_id,
                    trace_id: Some(trace_id),
                    limit: limit(&arguments, 500, MAX_TRACE_COMPONENT_LIMIT),
                })?,
            }))
        }
        "list_apps" => Ok(serde_json::to_value(crate::discovery::list_apps()?)?),
        "list_ipc_calls" => {
            let store = open_store(&arguments)?;
            Ok(serde_json::to_value(store.list_tauri_ipc_calls(
                &TauriIpcQuery {
                    session_id: optional_string(&arguments, "sessionId"),
                    trace_id: optional_string(&arguments, "traceId"),
                    limit: limit(&arguments, 200, MAX_LIST_LIMIT),
                },
            )?)?)
        }
        "list_events" => {
            let store = open_store(&arguments)?;
            Ok(serde_json::to_value(store.list_tauri_events(
                &TauriEventQuery {
                    session_id: optional_string(&arguments, "sessionId"),
                    trace_id: optional_string(&arguments, "traceId"),
                    limit: limit(&arguments, 200, MAX_LIST_LIMIT),
                },
            )?)?)
        }
        "list_windows" => {
            let store = open_store(&arguments)?;
            Ok(serde_json::to_value(store.list_tauri_windows(
                &TauriWindowQuery {
                    session_id: optional_string(&arguments, "sessionId"),
                    latest_only: true,
                    limit: limit(&arguments, 200, MAX_LIST_LIMIT),
                },
            )?)?)
        }
        _ => anyhow::bail!("Unknown tool {}", call.name),
    }
}

fn open_store(arguments: &Value) -> Result<SqliteStore> {
    let db = match optional_string(arguments, "db") {
        Some(db) => crate::discovery::resolve_db(Some(PathBuf::from(db)))?,
        None => crate::discovery::resolve_db(None)?,
    };
    let store = SqliteStore::open(db)?;
    store.migrate()?;
    store.validate_schema()?;
    Ok(store)
}

fn required_string(arguments: &Value, name: &str) -> Result<String> {
    optional_string(arguments, name)
        .ok_or_else(|| anyhow::anyhow!("Missing required argument {name}"))
}

fn optional_string(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn optional_path(arguments: &Value, name: &str) -> Result<Option<PathBuf>> {
    Ok(optional_string(arguments, name).map(PathBuf::from))
}

fn limit(arguments: &Value, default: usize, max: usize) -> Option<usize> {
    Some(
        arguments
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(default)
            .min(max),
    )
}

fn tool_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    if text.len() > MAX_TOOL_RESPONSE_BYTES {
        return tool_text_result(
            format!(
                "Tool response exceeded {MAX_TOOL_RESPONSE_BYTES} bytes. Narrow the query with sessionId, traceId, or limit."
            ),
            true,
        );
    }
    tool_text_result(text, is_error)
}

fn tool_text_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

fn json_rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn json_rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "doctor",
            "Run Auditaur diagnostics for an optional SQLite DB path.",
            &[],
        ),
        tool(
            "list_sessions",
            "List sessions in a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
        tool(
            "list_logs",
            "List logs in a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
        tool(
            "list_errors",
            "List frontend errors in a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
        tool(
            "list_traces",
            "List trace summaries in a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
        tool(
            "get_trace",
            "Get spans, logs, frontend errors, IPC calls, and events for a trace. Requires traceId; uses discovery when db is omitted.",
            &["traceId"],
        ),
        tool(
            "list_apps",
            "List active and stale apps discovered locally.",
            &[],
        ),
        tool(
            "list_ipc_calls",
            "List Tauri IPC calls from a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
        tool(
            "list_events",
            "List Tauri events from a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
        tool(
            "list_windows",
            "List latest Tauri window states from a SQLite DB. Uses discovery when db is omitted.",
            &[],
        ),
    ]
}

fn tool(name: &str, description: &str, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "db": { "type": "string" },
                "sessionId": { "type": "string" },
                "traceId": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1 }
            },
            "required": required,
            "additionalProperties": true
        }
    })
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolCall {
    name: String,
    arguments: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::{handle_line, MAX_LIST_LIMIT};
    use auditaur_collector::exporter_sqlite::{SqliteStore, SQLITE_SCHEMA_VERSION};
    use auditaur_core::model::{Session, TauriIpcCall};
    use serde_json::{json, Value};
    use tempfile::NamedTempFile;

    #[test]
    fn lists_tools() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert_eq!(response["result"]["tools"][0]["name"], "doctor");
        assert_eq!(
            response["result"]["tools"][5]["inputSchema"]["required"],
            json!(["traceId"])
        );
    }

    #[test]
    fn rejects_unknown_method() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#).unwrap();
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn ignores_notifications() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn calls_doctor() {
        let response = handle_line(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": "doctor", "arguments": {} }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], false);
    }

    #[test]
    fn calls_list_sessions_against_fixture_db() {
        let db = NamedTempFile::new().unwrap();
        let store = SqliteStore::open(db.path()).unwrap();
        store.migrate().unwrap();
        store
            .create_session(&Session {
                id: "session-mcp".to_string(),
                session_name: None,
                service_name: "mcp-test".to_string(),
                service_version: None,
                app_identifier: None,
                pid: None,
                started_at: "2026-05-18T18:00:00Z".to_string(),
                ended_at: None,
                schema_version: SQLITE_SCHEMA_VERSION,
                auditaur_version: None,
            })
            .unwrap();
        drop(store);

        let response = handle_line(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "list_sessions",
                    "arguments": { "db": db.path().to_string_lossy() }
                }
            })
            .to_string(),
        )
        .unwrap();
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let sessions: Value = serde_json::from_str(text).unwrap();

        assert_eq!(sessions[0]["serviceName"], "mcp-test");
    }

    #[test]
    fn calls_list_ipc_against_fixture_db() {
        let db = NamedTempFile::new().unwrap();
        let store = SqliteStore::open(db.path()).unwrap();
        store.migrate().unwrap();
        store
            .create_session(&Session {
                id: "session-mcp".to_string(),
                session_name: None,
                service_name: "mcp-test".to_string(),
                service_version: None,
                app_identifier: None,
                pid: None,
                started_at: "2026-05-18T18:00:00Z".to_string(),
                ended_at: None,
                schema_version: SQLITE_SCHEMA_VERSION,
                auditaur_version: None,
            })
            .unwrap();
        store
            .insert_tauri_ipc_call(&TauriIpcCall {
                session_id: "session-mcp".to_string(),
                timestamp_unix_nanos: 1,
                duration_ms: Some(1.5),
                command: "failing_command".to_string(),
                status: "ERROR".to_string(),
                error_message: Some("boom".to_string()),
                trace_id: Some("trace-mcp".to_string()),
                span_id: Some("span-mcp".to_string()),
                window_label: Some("main".to_string()),
                args_json: Some(json!({ "reason": "test" })),
                args_redacted: true,
                result_summary: None,
            })
            .unwrap();
        drop(store);

        let response = handle_line(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "list_ipc_calls",
                    "arguments": { "db": db.path().to_string_lossy() }
                }
            })
            .to_string(),
        )
        .unwrap();
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let calls: Value = serde_json::from_str(text).unwrap();

        assert_eq!(calls[0]["command"], "failing_command");
    }

    #[test]
    fn clamps_untrusted_limits() {
        let db = NamedTempFile::new().unwrap();
        let store = SqliteStore::open(db.path()).unwrap();
        store.migrate().unwrap();
        for index in 0..=MAX_LIST_LIMIT {
            store
                .create_session(&Session {
                    id: format!("session-{index}"),
                    session_name: None,
                    service_name: "mcp-test".to_string(),
                    service_version: None,
                    app_identifier: None,
                    pid: None,
                    started_at: "2026-05-18T18:00:00Z".to_string(),
                    ended_at: None,
                    schema_version: SQLITE_SCHEMA_VERSION,
                    auditaur_version: None,
                })
                .unwrap();
        }
        drop(store);

        let response = handle_line(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "list_sessions",
                    "arguments": {
                        "db": db.path().to_string_lossy(),
                        "limit": MAX_LIST_LIMIT + 100
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let sessions: Value = serde_json::from_str(text).unwrap();

        assert_eq!(sessions.as_array().unwrap().len(), MAX_LIST_LIMIT);
    }
}
