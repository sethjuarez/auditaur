use std::path::Path;

use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::model::TelemetrySource;
use auditaur_core::storage::{RelatedTelemetry, RelatedTelemetryQuery};

/// Asserts that a frontend `tauri.invoke <command>` span is stitched to a backend child span.
///
/// This helper is intended for app integration tests that generate telemetry into an Auditaur
/// SQLite database and want to prove frontend-to-backend trace continuity.
pub fn assert_trace_stitched(db_path: impl AsRef<Path>, command: &str) {
    let store = SqliteStore::open(db_path.as_ref()).unwrap_or_else(|error| {
        panic!(
            "failed to open Auditaur database {}: {error}",
            db_path.as_ref().display()
        )
    });
    store
        .validate_schema()
        .unwrap_or_else(|error| panic!("Auditaur database schema is invalid: {error}"));
    let related = store
        .related_telemetry(&RelatedTelemetryQuery {
            session_id: None,
            trace_id: None,
            window_label: None,
            start_time_unix_nanos: None,
            end_time_unix_nanos: None,
            limit: Some(10_000),
        })
        .unwrap_or_else(|error| panic!("failed to read Auditaur telemetry: {error}"));

    assert_trace_stitched_in_related(&related, command);
}

fn assert_trace_stitched_in_related(related: &RelatedTelemetry, command: &str) {
    let matching_calls: Vec<_> = related
        .tauri_ipc_calls
        .iter()
        .filter(|call| call.command == command)
        .collect();
    assert!(
        !matching_calls.is_empty(),
        "expected at least one Tauri IPC call for command `{command}`"
    );

    for call in matching_calls {
        let (Some(trace_id), Some(frontend_span_id)) =
            (call.trace_id.as_deref(), call.span_id.as_deref())
        else {
            continue;
        };
        let expected_frontend_name = format!("tauri.invoke {command}");
        let has_frontend_span = related.spans.iter().any(|span| {
            span.trace_id == trace_id
                && span.span_id == frontend_span_id
                && span.name == expected_frontend_name
        });
        if !has_frontend_span {
            continue;
        }
        let has_backend_child = related.spans.iter().any(|span| {
            span.trace_id == trace_id
                && span.source == TelemetrySource::Backend
                && span.parent_span_id.as_deref() == Some(frontend_span_id)
        });
        if has_backend_child {
            return;
        }
    }

    panic!(
        "expected command `{command}` to have a backend span parented to its frontend tauri.invoke span"
    );
}

#[cfg(test)]
mod tests {
    use super::assert_trace_stitched;
    use auditaur_collector::exporter_sqlite::{SqliteStore, SQLITE_SCHEMA_VERSION};
    use auditaur_core::model::{Session, SpanRecord, TauriIpcCall, TelemetrySource};
    use serde_json::json;
    use tempfile::NamedTempFile;

    #[test]
    fn accepts_stitched_trace() {
        let db = trace_fixture(true);

        assert_trace_stitched(db.path(), "load_user");
    }

    #[test]
    #[should_panic(expected = "backend span parented to its frontend tauri.invoke span")]
    fn rejects_unstitched_trace() {
        let db = trace_fixture(false);

        assert_trace_stitched(db.path(), "load_user");
    }

    fn trace_fixture(include_backend_child: bool) -> NamedTempFile {
        let db = NamedTempFile::new().unwrap();
        let store = SqliteStore::open(db.path()).unwrap();
        store.migrate().unwrap();
        let session = Session {
            id: "session-test".to_string(),
            session_name: Some("test".to_string()),
            service_name: "test-app".to_string(),
            service_version: None,
            app_identifier: Some("dev.test".to_string()),
            pid: Some(1),
            started_at: "2026-06-09T00:00:00Z".to_string(),
            ended_at: None,
            schema_version: SQLITE_SCHEMA_VERSION,
            auditaur_version: Some("0.1.0".to_string()),
        };
        store.create_session(&session).unwrap();
        store
            .insert_span(&SpanRecord {
                session_id: session.id.clone(),
                trace_id: "trace-test".to_string(),
                span_id: "frontend-span".to_string(),
                parent_span_id: None,
                name: "tauri.invoke load_user".to_string(),
                kind: Some("client".to_string()),
                start_time_unix_nanos: 100,
                end_time_unix_nanos: Some(200),
                status_code: Some("OK".to_string()),
                status_message: None,
                scope_name: Some("@auditaur/api".to_string()),
                scope_version: None,
                attributes: json!({ "tauri.command": "load_user" }),
                source: TelemetrySource::Frontend,
            })
            .unwrap();
        if include_backend_child {
            store
                .insert_span(&SpanRecord {
                    session_id: session.id.clone(),
                    trace_id: "trace-test".to_string(),
                    span_id: "backend-span".to_string(),
                    parent_span_id: Some("frontend-span".to_string()),
                    name: "load_user".to_string(),
                    kind: Some("internal".to_string()),
                    start_time_unix_nanos: 120,
                    end_time_unix_nanos: Some(180),
                    status_code: Some("OK".to_string()),
                    status_message: None,
                    scope_name: Some("backend".to_string()),
                    scope_version: None,
                    attributes: json!({ "traceparent": "test" }),
                    source: TelemetrySource::Backend,
                })
                .unwrap();
        } else {
            store
                .insert_span(&SpanRecord {
                    session_id: session.id.clone(),
                    trace_id: "trace-test".to_string(),
                    span_id: "frontend-child-span".to_string(),
                    parent_span_id: Some("frontend-span".to_string()),
                    name: "frontend child".to_string(),
                    kind: Some("internal".to_string()),
                    start_time_unix_nanos: 130,
                    end_time_unix_nanos: Some(150),
                    status_code: Some("OK".to_string()),
                    status_message: None,
                    scope_name: Some("@auditaur/api".to_string()),
                    scope_version: None,
                    attributes: json!({ "auditaur.source": "frontend" }),
                    source: TelemetrySource::Frontend,
                })
                .unwrap();
        }
        store
            .insert_tauri_ipc_call(&TauriIpcCall {
                session_id: session.id,
                timestamp_unix_nanos: 110,
                duration_ms: Some(10.0),
                command: "load_user".to_string(),
                status: "OK".to_string(),
                error_message: None,
                trace_id: Some("trace-test".to_string()),
                span_id: Some("frontend-span".to_string()),
                window_label: Some("main".to_string()),
                args_json: None,
                args_redacted: false,
                result_summary: None,
            })
            .unwrap();
        drop(store);
        db
    }
}
