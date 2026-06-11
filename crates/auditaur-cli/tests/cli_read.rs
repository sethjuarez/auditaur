use auditaur_collector::exporter_sqlite::{SqliteStore, SQLITE_SCHEMA_VERSION};
use auditaur_core::{
    discovery::DiscoveryFile,
    model::{
        FrontendError, LogRecord, Session, SpanEventRecord, SpanRecord, TauriEventRecord,
        TauriIpcCall, TauriWindowState, TelemetrySource,
    },
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
};
use tempfile::{NamedTempFile, TempDir};
use tungstenite::{accept, Message};

#[test]
fn reads_fixture_database_as_json() {
    let db = fixture_database();

    let sessions = run_json(["sessions", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(sessions[0]["serviceName"], "auditaur-fixture");

    let logs = run_json(["logs", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(logs[0]["body"], "fixture log");
    assert_eq!(logs[0]["source"], "third_party_otel");

    let traces = run_json(["traces", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(traces[0]["traceId"], "trace-fixture");
    assert_eq!(traces[0]["spanCount"], 1);

    let trace = run_json([
        "trace",
        "trace-fixture",
        "--db",
        db.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(trace["spans"][0]["name"], "fixture span");
    assert_eq!(trace["logs"][0]["body"], "fixture log");
    assert_eq!(trace["frontendErrors"][0]["message"], "fixture error");
    assert_eq!(trace["tauriWindows"][0]["windowLabel"], "main");

    let ipc = run_json(["ipc", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(ipc[0]["command"], "fixture_command");

    let events = run_json(["events", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(events[0]["eventName"], "fixture:event");

    let windows = run_json(["windows", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(windows[0]["windowLabel"], "main");

    let failed_ipc = run_json([
        "ipc",
        "--db",
        db.path().to_str().unwrap(),
        "--failed",
        "--json",
    ]);
    assert_eq!(failed_ipc[0]["command"], "fixture_command");

    let timeline = run_json(["timeline", "--db", db.path().to_str().unwrap(), "--json"]);
    assert!(timeline.as_array().unwrap().len() >= 6);
    assert_eq!(timeline[0]["kind"], "span");

    let related = run_json([
        "related",
        "--db",
        db.path().to_str().unwrap(),
        "--trace",
        "trace-fixture",
        "--json",
    ]);
    assert_eq!(related["spans"][0]["name"], "fixture span");
    assert_eq!(related["spanEvents"].as_array().unwrap().len(), 0);
    assert_eq!(related["logs"][0]["body"], "fixture log");
    assert_eq!(related["tauriIpcCalls"][0]["command"], "fixture_command");
    assert_eq!(related["tauriWindows"][0]["windowLabel"], "main");

    let explain = run_json(["explain", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(explain["failedIpcCount"], 1);
    assert!(explain["findings"].as_array().unwrap().len() >= 1);

    let exceptions = run_json(["exceptions", "--db", db.path().to_str().unwrap(), "--json"]);
    let exception_reports = exceptions.as_array().unwrap();
    let frontend_report = exception_reports
        .iter()
        .find(|report| report["source"] == "frontend_error")
        .unwrap();
    let ipc_report = exception_reports
        .iter()
        .find(|report| report["source"] == "failed_ipc")
        .unwrap();
    let panic_report = exception_reports
        .iter()
        .find(|report| report["source"] == "rust_panic")
        .unwrap();
    assert_eq!(frontend_report["message"], "fixture error");
    assert_eq!(ipc_report["message"], "fixture failure");
    assert_eq!(panic_report["message"], "fixture panic");
    assert!(frontend_report["issueBodyMarkdown"]
        .as_str()
        .unwrap()
        .contains("Auditaur exception report"));

    let exception_markdown = run_stdout([
        "exceptions",
        "--db",
        db.path().to_str().unwrap(),
        "--markdown",
    ]);
    assert!(exception_markdown.contains("# Error: fixture error"));
    assert!(exception_markdown.contains("# Tauri IPC fixture_command: fixture failure"));
    assert!(exception_markdown.contains("Privacy note"));

    let fingerprint = frontend_report["fingerprint"].as_str().unwrap();
    let focused_exception = run_json([
        "exceptions",
        "--db",
        db.path().to_str().unwrap(),
        "--fingerprint",
        fingerprint,
        "--json",
    ]);
    assert_eq!(focused_exception.as_array().unwrap().len(), 1);
    assert_eq!(focused_exception[0]["fingerprint"], fingerprint);

    let output_dir = TempDir::new().unwrap();
    let output = output_dir.path().join("exception.md");
    run_stdout([
        "exceptions",
        "--db",
        db.path().to_str().unwrap(),
        "--fingerprint",
        fingerprint,
        "--markdown",
        "--output",
        output.to_str().unwrap(),
    ]);
    let exported = fs::read_to_string(output).unwrap();
    assert!(exported.contains("# Error: fixture error"));

    let json_output = output_dir.path().join("exception.json");
    run_stdout([
        "exceptions",
        "--db",
        db.path().to_str().unwrap(),
        "--fingerprint",
        fingerprint,
        "--output",
        json_output.to_str().unwrap(),
    ]);
    let exported_json: Value =
        serde_json::from_str(&fs::read_to_string(json_output).unwrap()).unwrap();
    assert_eq!(exported_json[0]["fingerprint"], fingerprint);

    let bundle = run_json(["bundle", "--db", db.path().to_str().unwrap(), "--redacted"]);
    assert_eq!(bundle["redacted"], true);
    assert_eq!(bundle["tauriIpcCalls"][0]["argsJson"], "[redacted]");

    let tail = run_stdout([
        "tail",
        "--db",
        db.path().to_str().unwrap(),
        "--replay",
        "--duration-seconds",
        "0",
        "--json",
    ]);
    assert!(tail.contains("\"kind\":\"ipc\""));
}

#[test]
fn agentive_runs_group_model_tool_events_and_related_by_run_id() {
    let db = fixture_database();
    let store = SqliteStore::open(db.path()).unwrap();
    insert_agentive_fixture(&store);
    drop(store);

    let runs = run_json(["agent-runs", "--db", db.path().to_str().unwrap(), "--json"]);
    let run = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["runId"] == "10ed86a4-d559-4b6d-8319-9bdba2c0ff78")
        .unwrap();
    assert_eq!(run["traceId"], "a9ba86b6ef5906a2b7af3c8423ea9001");
    assert_eq!(run["rootCommand"], "agent_chat_with_tools");
    assert_eq!(run["modelCallCount"], 3);
    assert_eq!(run["toolCallCount"], 2);
    assert_eq!(run["agentEventCount"], 2);
    assert_eq!(run["provider"], "openai");
    assert_eq!(run["model"], "gpt-4.1-mini");
    assert!(run["finalSummary"]
        .as_str()
        .unwrap()
        .contains("Wrote sketch"));

    let detail = run_json([
        "agent-run",
        "10ed86a4-d559-4b6d-8319-9bdba2c0ff78",
        "--db",
        db.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(detail["run"]["modelCallCount"], 3);
    assert_eq!(detail["modelCalls"].as_array().unwrap().len(), 3);
    assert_eq!(detail["toolCalls"].as_array().unwrap().len(), 2);
    assert_eq!(detail["toolCalls"][0]["toolName"], "list_project_files");
    assert_eq!(detail["toolCalls"][1]["toolName"], "write_sketch");
    assert_eq!(
        detail["agentEvents"][1]["summary"],
        "Wrote sketch successfully"
    );

    let related = run_json([
        "related",
        "--db",
        db.path().to_str().unwrap(),
        "--run-id",
        "10ed86a4-d559-4b6d-8319-9bdba2c0ff78",
        "--json",
    ]);
    assert!(related["spans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|span| span["name"] == "tauri.invoke agent_chat_with_tools"));
    assert_eq!(related["spanEvents"].as_array().unwrap().len(), 2);
}

#[test]
fn explain_flags_missing_backend_trace_continuation() {
    let db = fixture_database();
    let store = SqliteStore::open(db.path()).unwrap();
    store
        .insert_span(&SpanRecord {
            session_id: "session-fixture".to_string(),
            trace_id: "trace-missing-backend".to_string(),
            span_id: "frontend-span".to_string(),
            parent_span_id: None,
            name: "tauri.invoke missing_backend_command".to_string(),
            kind: Some("client".to_string()),
            start_time_unix_nanos: 300,
            end_time_unix_nanos: Some(400),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("@auditaur/api".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "tauri.command": "missing_backend_command" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_span(&SpanRecord {
            session_id: "session-fixture".to_string(),
            trace_id: "trace-missing-backend".to_string(),
            span_id: "frontend-child-span".to_string(),
            parent_span_id: Some("frontend-span".to_string()),
            name: "frontend child".to_string(),
            kind: Some("internal".to_string()),
            start_time_unix_nanos: 320,
            end_time_unix_nanos: Some(330),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("@auditaur/api".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "auditaur.source": "frontend" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_tauri_ipc_call(&TauriIpcCall {
            session_id: "session-fixture".to_string(),
            timestamp_unix_nanos: 310,
            duration_ms: Some(5.0),
            command: "missing_backend_command".to_string(),
            status: "OK".to_string(),
            error_message: None,
            trace_id: Some("trace-missing-backend".to_string()),
            span_id: Some("frontend-span".to_string()),
            window_label: Some("main".to_string()),
            args_json: None,
            args_redacted: false,
            result_summary: Some("\"ok\"".to_string()),
        })
        .unwrap();
    drop(store);

    let explain = run_json([
        "explain",
        "--db",
        db.path().to_str().unwrap(),
        "--trace",
        "trace-missing-backend",
        "--json",
    ]);
    let findings = explain["findings"].as_array().unwrap();
    assert!(findings.iter().any(|finding| {
        finding
            .as_str()
            .unwrap()
            .contains("Missing backend trace continuation for tauri.invoke missing_backend_command")
    }));
}

#[test]
fn explain_does_not_flag_stitched_backend_trace_continuation() {
    let db = fixture_database();
    let store = SqliteStore::open(db.path()).unwrap();
    store
        .insert_span(&SpanRecord {
            session_id: "session-fixture".to_string(),
            trace_id: "trace-stitched-backend".to_string(),
            span_id: "frontend-span".to_string(),
            parent_span_id: None,
            name: "tauri.invoke stitched_backend_command".to_string(),
            kind: Some("client".to_string()),
            start_time_unix_nanos: 300,
            end_time_unix_nanos: Some(350),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("@auditaur/api".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "tauri.command": "stitched_backend_command" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_span(&SpanRecord {
            session_id: "session-fixture".to_string(),
            trace_id: "trace-stitched-backend".to_string(),
            span_id: "backend-span".to_string(),
            parent_span_id: Some("frontend-span".to_string()),
            name: "stitched_backend_command".to_string(),
            kind: Some("internal".to_string()),
            start_time_unix_nanos: 320,
            end_time_unix_nanos: Some(340),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("backend".to_string()),
            scope_version: None,
            attributes: json!({ "traceparent": "00-trace-stitched-backend-frontend-span-01" }),
            source: TelemetrySource::Backend,
        })
        .unwrap();
    store
        .insert_tauri_ipc_call(&TauriIpcCall {
            session_id: "session-fixture".to_string(),
            timestamp_unix_nanos: 310,
            duration_ms: Some(5.0),
            command: "stitched_backend_command".to_string(),
            status: "OK".to_string(),
            error_message: None,
            trace_id: Some("trace-stitched-backend".to_string()),
            span_id: Some("frontend-span".to_string()),
            window_label: Some("main".to_string()),
            args_json: None,
            args_redacted: false,
            result_summary: Some("\"ok\"".to_string()),
        })
        .unwrap();
    drop(store);

    let explain = run_json([
        "explain",
        "--db",
        db.path().to_str().unwrap(),
        "--trace",
        "trace-stitched-backend",
        "--json",
    ]);
    let findings = explain["findings"].as_array().unwrap();
    assert!(!findings.iter().any(|finding| {
        finding
            .as_str()
            .unwrap()
            .contains("Missing backend trace continuation")
    }));
}

#[test]
fn discovers_apps_and_reads_default_database() {
    let temp = TempDir::new().unwrap();
    let db_path = temp
        .path()
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let store = create_fixture_database_at(&db_path);
    drop(store);

    let apps_dir = temp.path().join("apps");
    fs::create_dir_all(&apps_dir).unwrap();
    fs::write(
        apps_dir.join("instance-fixture.json"),
        serde_json::to_vec_pretty(&DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-fixture".to_string(),
            session_id: "session-fixture".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: db_path.to_string_lossy().to_string(),
            capabilities: vec![
                "logs".to_string(),
                "traces".to_string(),
                "frontend_errors".to_string(),
                "ipc".to_string(),
                "events".to_string(),
                "windows".to_string(),
            ],
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        })
        .unwrap(),
    )
    .unwrap();

    let apps = run_json_with_env(["apps", "--json"], temp.path().to_str().unwrap());
    assert_eq!(apps[0]["status"], "active");
    assert_eq!(apps[0]["schemaValid"], true);

    let health = run_json_with_env(["health", "--json"], temp.path().to_str().unwrap());
    assert_eq!(health["ok"], true);
    assert_eq!(health["apps"][0]["checks"][0]["name"], "heartbeat");

    let logs = run_json_with_env(["logs", "--json"], temp.path().to_str().unwrap());
    assert_eq!(logs[0]["body"], "fixture log");
}

#[test]
fn drive_reports_attach_info_and_cdp_endpoint() {
    let temp = TempDir::new().unwrap();
    let db_path = temp
        .path()
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&db_path));
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-drive".to_string(),
            session_id: "session-drive".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        },
    );
    let (port, server) = start_fake_cdp_endpoint();
    let port_arg = port.to_string();

    let attach = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(attach["serviceName"], "auditaur-fixture");
    assert_eq!(attach["pid"], 42);
    assert_eq!(attach["sessionId"], "session-drive");
    assert_eq!(attach["dbPath"], db_path.to_string_lossy().to_string());
    assert_eq!(attach["cdp"]["status"], "available");
    assert_eq!(attach["cdp"]["port"], port);
    assert_eq!(attach["cdp"]["product"], "Chrome/125.0.0.0");
    assert_eq!(attach["cdp"]["targetBindingStatus"], "unverified");
    assert_eq!(attach["cdp"]["targets"][0]["id"], "target-fixture");
    assert!(attach["futureActions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["name"] == "click"));
    assert!(attach["requiredActionTelemetry"]
        .as_array()
        .unwrap()
        .contains(&json!("auditaur.test_id")));
}

#[test]
fn drive_wait_requires_explicit_cdp_port() {
    let failure = run_failure(["drive", "wait", "--selector", "[data-testid=ready]"]);
    assert!(failure.contains("requires --cdp-port"));
}

#[test]
fn drive_wait_rejects_ambiguous_cdp_targets_without_target_id() {
    let temp = TempDir::new().unwrap();
    let db_path = temp
        .path()
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&db_path));
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-drive-ambiguous".to_string(),
            session_id: "session-drive-ambiguous".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        },
    );
    let (port, server) = start_fake_cdp_multi_target_endpoint();
    let port_arg = port.to_string();

    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "wait",
            "--selector",
            "[data-testid=ready]",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert!(failure.contains("Multiple driveable CDP targets found"));
}

#[test]
fn drive_wait_uses_cdp_runtime_evaluate_without_mutating_observability_store() {
    let temp = TempDir::new().unwrap();
    let db_path = temp
        .path()
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&db_path));
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-drive-wait".to_string(),
            session_id: "session-drive-wait".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        },
    );
    let (port, server) = start_fake_cdp_wait_endpoint();
    let port_arg = port.to_string();

    let wait = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
            "wait",
            "--selector",
            "[data-testid=ready]",
            "--test-id",
            "test-1",
            "--step-id",
            "step-1",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(wait["ok"], true);
    assert_eq!(wait["action"], "wait");
    assert_eq!(wait["selector"], "[data-testid=ready]");
    assert_eq!(wait["sessionId"], "session-drive-wait");
    assert_eq!(wait["targetId"], "target-fixture");
    assert_eq!(
        wait["telemetryAttributes"]["auditaur.driver.action"],
        "wait"
    );
    assert_eq!(wait["telemetryAttributes"]["auditaur.test_id"], "test-1");
}

#[test]
fn drive_wait_timeout_prints_structured_json_and_exits_nonzero() {
    let temp = TempDir::new().unwrap();
    let db_path = temp
        .path()
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&db_path));
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-drive-timeout".to_string(),
            session_id: "session-drive-timeout".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        },
    );
    let (port, server) = start_fake_cdp_timeout_endpoint();
    let port_arg = port.to_string();

    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
            "wait",
            "--selector",
            "[data-testid=never]",
            "--timeout-ms",
            "200",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert!(failure.contains("\"ok\": false"));
    assert!(failure.contains("\"matched\": false"));
    assert!(failure.contains("Timed out after 200ms"));
}

#[test]
fn health_ignores_stale_apps_but_fails_unhealthy_active_apps() {
    let stale_temp = TempDir::new().unwrap();
    write_discovery_file(
        stale_temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-stale".to_string(),
            session_id: "session-stale".to_string(),
            service_name: "stale-app".to_string(),
            service_version: None,
            app_identifier: None,
            pid: 1,
            started_at: "2000-01-01T00:00:00Z".to_string(),
            database_path: stale_temp
                .path()
                .join("missing.sqlite")
                .to_string_lossy()
                .to_string(),
            capabilities: Vec::new(),
            last_heartbeat_at: "2000-01-01T00:00:00Z".to_string(),
        },
    );

    let stale_health = run_json_with_env(["health", "--json"], stale_temp.path().to_str().unwrap());
    assert_eq!(stale_health["ok"], true);
    assert_eq!(stale_health["apps"][0]["ok"], false);
    assert_eq!(stale_health["apps"][0]["status"], "stale");

    let active_temp = TempDir::new().unwrap();
    write_discovery_file(
        active_temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-active".to_string(),
            session_id: "session-active".to_string(),
            service_name: "active-bad-app".to_string(),
            service_version: None,
            app_identifier: None,
            pid: 1,
            started_at: "2099-01-01T00:00:00Z".to_string(),
            database_path: active_temp
                .path()
                .join("missing.sqlite")
                .to_string_lossy()
                .to_string(),
            capabilities: vec!["logs".to_string()],
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        },
    );

    let active_health =
        run_json_with_env(["health", "--json"], active_temp.path().to_str().unwrap());
    assert_eq!(active_health["ok"], false);
    assert_eq!(active_health["apps"][0]["ok"], false);
    assert_eq!(active_health["apps"][0]["status"], "active");
}

#[test]
fn apps_explain_stale_sessions_superseded_by_newer_active_sessions() {
    let temp = TempDir::new().unwrap();
    let stale_db_path = temp
        .path()
        .join("sessions")
        .join("session-stale")
        .join("telemetry.sqlite");
    fs::create_dir_all(stale_db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&stale_db_path));
    let active_db_path = temp
        .path()
        .join("sessions")
        .join("session-active")
        .join("telemetry.sqlite");
    fs::create_dir_all(active_db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&active_db_path));

    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-stale".to_string(),
            session_id: "session-stale".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 41,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: stale_db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2000-01-01T00:00:00Z".to_string(),
        },
    );
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-active".to_string(),
            session_id: "session-active".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:12Z".to_string(),
            database_path: active_db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        },
    );

    let apps = run_json_with_env(["apps", "--json"], temp.path().to_str().unwrap());
    let stale = apps
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["sessionId"] == "session-stale")
        .unwrap();
    assert_eq!(stale["supersededBySessionId"], "session-active");
    assert_eq!(stale["secondsUntilNextStart"], 12);
    assert!(stale["churnHint"]
        .as_str()
        .unwrap()
        .contains("Tauri dev watcher rebuild"));

    let health = run_json_with_env(["health", "--json"], temp.path().to_str().unwrap());
    let stale_health = health["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["sessionId"] == "session-stale")
        .unwrap();
    assert!(stale_health["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "session-churn"));
}

#[test]
fn doctor_tauri_reports_dogfood_setup() {
    let dogfood_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("dogfood");
    let report = run_json([
        "doctor",
        "tauri",
        "--path",
        dogfood_path.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(report["ok"], true);
}

fn run_json<const N: usize>(args: [&str; N]) -> Value {
    serde_json::from_str(&run_stdout(args)).unwrap()
}

fn run_stdout<const N: usize>(args: [&str; N]) -> String {
    run_command(Command::new(env!("CARGO_BIN_EXE_auditaur")).args(args))
}

fn run_failure<const N: usize>(args: [&str; N]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_auditaur"));
    command.args(args);
    run_failure_command(&mut command)
}

fn run_json_with_env<const N: usize>(args: [&str; N], data_dir: &str) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_auditaur"));
    command.args(args).env("AUDITAUR_DATA_DIR", data_dir);
    run_json_command(&mut command)
}

fn run_failure_with_env<const N: usize>(args: [&str; N], data_dir: &str) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_auditaur"));
    command.args(args).env("AUDITAUR_DATA_DIR", data_dir);
    run_failure_command(&mut command)
}

fn write_discovery_file(root: &std::path::Path, discovery: DiscoveryFile) {
    let apps_dir = root.join("apps");
    fs::create_dir_all(&apps_dir).unwrap();
    fs::write(
        apps_dir.join(format!("{}.json", discovery.instance_id)),
        serde_json::to_vec_pretty(&discovery).unwrap(),
    )
    .unwrap();
}

fn expected_capabilities() -> Vec<String> {
    vec![
        "logs".to_string(),
        "traces".to_string(),
        "frontend_errors".to_string(),
        "ipc".to_string(),
        "events".to_string(),
        "windows".to_string(),
    ]
}

fn start_fake_cdp_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &target_list_json(port));
    });
    (port, handle)
}

fn start_fake_cdp_wait_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &target_list_json(port));
        let (stream, _) = listener.accept().unwrap();
        let mut websocket = accept(stream).unwrap();
        loop {
            let message = websocket.read().unwrap();
            if !message.is_text() {
                continue;
            }
            let request: Value = serde_json::from_str(&message.into_text().unwrap()).unwrap();
            let id = request["id"].as_u64().unwrap();
            let method = request["method"].as_str().unwrap();
            let response = match method {
                "Runtime.enable" => json!({ "id": id, "result": {} }),
                "Runtime.evaluate" => json!({
                    "id": id,
                    "result": {
                        "result": {
                            "type": "boolean",
                            "value": true
                        }
                    }
                }),
                _ => json!({ "id": id, "error": { "message": "unexpected method" } }),
            };
            websocket
                .send(Message::Text(response.to_string().into()))
                .unwrap();
            if method == "Runtime.evaluate" {
                break;
            }
        }
    });
    (port, handle)
}

fn start_fake_cdp_multi_target_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(
            &listener,
            &json!([
                {
                    "id": "target-one",
                    "type": "page",
                    "title": "One",
                    "url": "tauri://localhost/one",
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/one")
                },
                {
                    "id": "target-two",
                    "type": "page",
                    "title": "Two",
                    "url": "tauri://localhost/two",
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/two")
                }
            ])
            .to_string(),
        );
    });
    (port, handle)
}

fn start_fake_cdp_timeout_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &target_list_json(port));
        let (stream, _) = listener.accept().unwrap();
        let mut websocket = accept(stream).unwrap();
        let enable = websocket.read().unwrap();
        let enable: Value = serde_json::from_str(&enable.into_text().unwrap()).unwrap();
        websocket
            .send(Message::Text(
                json!({ "id": enable["id"].as_u64().unwrap(), "result": {} })
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let _evaluate = websocket.read().unwrap();
        thread::sleep(std::time::Duration::from_millis(400));
    });
    (port, handle)
}

fn target_list_json(port: u16) -> String {
    json!([
        {
            "id": "target-fixture",
            "type": "page",
            "title": "Fixture",
            "url": "tauri://localhost/",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/target-fixture")
        }
    ])
    .to_string()
}

fn respond_http_json(listener: &TcpListener, body: &str) {
    let (mut stream, _) = listener.accept().unwrap();
    let mut request = [0_u8; 512];
    let _ = stream.read(&mut request).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}

fn run_json_command(command: &mut Command) -> Value {
    serde_json::from_str(&run_command(command)).unwrap()
}

fn run_command(command: &mut Command) -> String {
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap()
}

fn run_failure_command(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn fixture_database() -> NamedTempFile {
    let db = NamedTempFile::new().unwrap();
    let store = create_fixture_database_at(db.path());
    drop(store);
    db
}

fn create_fixture_database_at(path: &std::path::Path) -> SqliteStore {
    let store = SqliteStore::open(path).unwrap();
    store.migrate().unwrap();

    let session = Session {
        id: "session-fixture".to_string(),
        session_name: Some("fixture".to_string()),
        service_name: "auditaur-fixture".to_string(),
        service_version: Some("0.1.0".to_string()),
        app_identifier: Some("dev.auditaur.fixture".to_string()),
        pid: Some(42),
        started_at: "2026-05-18T18:00:00Z".to_string(),
        ended_at: None,
        schema_version: SQLITE_SCHEMA_VERSION,
        auditaur_version: Some("0.1.0".to_string()),
    };
    store.create_session(&session).unwrap();

    store
        .insert_span(&SpanRecord {
            session_id: session.id.clone(),
            trace_id: "trace-fixture".to_string(),
            span_id: "span-fixture".to_string(),
            parent_span_id: None,
            name: "fixture span".to_string(),
            kind: Some("internal".to_string()),
            start_time_unix_nanos: 100,
            end_time_unix_nanos: Some(200),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("fixture".to_string()),
            scope_version: Some("1.0.0".to_string()),
            attributes: json!({ "fixture": true }),
            source: TelemetrySource::ThirdPartyOtel,
        })
        .unwrap();

    store
        .insert_log(&LogRecord {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 150,
            observed_timestamp_unix_nanos: Some(155),
            severity_text: Some("INFO".to_string()),
            severity_number: Some(9),
            body: Some("fixture log".to_string()),
            body_json: None,
            trace_id: Some("trace-fixture".to_string()),
            span_id: Some("span-fixture".to_string()),
            scope_name: Some("fixture".to_string()),
            scope_version: Some("1.0.0".to_string()),
            attributes: json!({ "fixture": true }),
            source: TelemetrySource::ThirdPartyOtel,
        })
        .unwrap();

    store
        .insert_frontend_error(&FrontendError {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 175,
            message: "fixture error".to_string(),
            stack: None,
            filename: Some("main.ts".to_string()),
            line_number: Some(1),
            column_number: Some(2),
            error_type: Some("Error".to_string()),
            trace_id: Some("trace-fixture".to_string()),
            span_id: Some("span-fixture".to_string()),
            window_label: Some("main".to_string()),
            attributes: json!({ "fixture": true }),
        })
        .unwrap();
    store
        .insert_frontend_error(&FrontendError {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 176,
            message: "fixture panic".to_string(),
            stack: Some("src/main.rs:10:2".to_string()),
            filename: Some("src/main.rs".to_string()),
            line_number: Some(10),
            column_number: Some(2),
            error_type: Some("RustPanic".to_string()),
            trace_id: None,
            span_id: None,
            window_label: None,
            attributes: json!({ "auditaur.source": "panic_hook" }),
        })
        .unwrap();
    store
        .insert_tauri_ipc_call(&TauriIpcCall {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 180,
            duration_ms: Some(3.0),
            command: "fixture_command".to_string(),
            status: "ERROR".to_string(),
            error_message: Some("fixture failure".to_string()),
            trace_id: Some("trace-fixture".to_string()),
            span_id: Some("span-fixture".to_string()),
            window_label: Some("main".to_string()),
            args_json: Some(json!({ "ok": true })),
            args_redacted: true,
            result_summary: None,
        })
        .unwrap();
    store
        .insert_tauri_event(&TauriEventRecord {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 181,
            event_name: "fixture:event".to_string(),
            direction: "emit".to_string(),
            target: Some("main".to_string()),
            trace_id: Some("trace-fixture".to_string()),
            span_id: Some("span-fixture".to_string()),
            window_label: Some("main".to_string()),
            payload_summary: Some("{\"ok\":true}".to_string()),
            payload_json: Some(json!({ "ok": true })),
            payload_redacted: true,
        })
        .unwrap();
    store
        .insert_tauri_window_state(&TauriWindowState {
            session_id: session.id,
            timestamp_unix_nanos: 182,
            window_label: "main".to_string(),
            webview_label: None,
            url: None,
            title: Some("Fixture".to_string()),
            focused: Some(true),
            visible: Some(true),
            width: Some(800.0),
            height: Some(600.0),
            scale_factor: Some(1.0),
            attributes: json!({}),
        })
        .unwrap();

    store
}

fn insert_agentive_fixture(store: &SqliteStore) {
    let session_id = "session-fixture";
    let trace_id = "a9ba86b6ef5906a2b7af3c8423ea9001";
    let run_id = "10ed86a4-d559-4b6d-8319-9bdba2c0ff78";
    store
        .insert_span(&SpanRecord {
            session_id: session_id.to_string(),
            trace_id: trace_id.to_string(),
            span_id: "root-tauri".to_string(),
            parent_span_id: None,
            name: "tauri.invoke agent_chat_with_tools".to_string(),
            kind: Some("client".to_string()),
            start_time_unix_nanos: 1_000,
            end_time_unix_nanos: Some(2_000),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("@auditaur/api".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "tauri.command": "agent_chat_with_tools" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_tauri_ipc_call(&TauriIpcCall {
            session_id: session_id.to_string(),
            timestamp_unix_nanos: 1_000,
            duration_ms: Some(1.0),
            command: "agent_chat_with_tools".to_string(),
            status: "OK".to_string(),
            error_message: None,
            trace_id: Some(trace_id.to_string()),
            span_id: Some("root-tauri".to_string()),
            window_label: Some("main".to_string()),
            args_json: None,
            args_redacted: true,
            result_summary: Some("Wrote sketch successfully".to_string()),
        })
        .unwrap();
    insert_agentive_span(
        store,
        "agent-run",
        Some("root-tauri"),
        "agentive.run",
        1_050,
        1_950,
        json!({ "agentive.run_id": run_id, "agentive.status": "done" }),
    );
    for (index, (span_id, model)) in [
        ("model-1", ""),
        ("model-2", "gpt-4.1-mini"),
        ("model-3", "gpt-4.1-mini"),
    ]
    .iter()
    .enumerate()
    {
        insert_agentive_span(
            store,
            span_id,
            Some("agent-run"),
            "agentive.model_call",
            1_100 + i64::try_from(index).unwrap() * 100,
            1_150 + i64::try_from(index).unwrap() * 100,
            json!({
                "agentive.run_id": run_id,
                "agentive.iteration": index + 1,
                "gen_ai.system": "openai",
                "gen_ai.request.model": model,
                "gen_ai.response.model": "gpt-4.1-mini",
                "gen_ai.usage.input_tokens": 10 + index,
                "gen_ai.usage.output_tokens": 20 + index,
                "gen_ai.usage.total_tokens": 30 + index
            }),
        );
    }
    insert_agentive_span(
        store,
        "tool-1",
        Some("agent-run"),
        "agentive.tool_call",
        1_300,
        1_325,
        json!({
            "agentive.run_id": run_id,
            "agentive.iteration": 1,
            "agentive.tool_name": "list_project_files",
            "agentive.tool_call_id": "call-list"
        }),
    );
    insert_agentive_span(
        store,
        "tool-2",
        Some("agent-run"),
        "agentive.tool_call",
        1_500,
        1_550,
        json!({
            "agentive.run_id": run_id,
            "agentive.iteration": 2,
            "agentive.tool_name": "write_sketch",
            "agentive.tool_call_id": "call-write"
        }),
    );
    for (timestamp, summary) in [
        (1_200, "Listed project files"),
        (1_900, "Wrote sketch successfully"),
    ] {
        store
            .insert_span_event(&SpanEventRecord {
                session_id: session_id.to_string(),
                trace_id: trace_id.to_string(),
                span_id: "agent-run".to_string(),
                name: "agent-event".to_string(),
                timestamp_unix_nanos: timestamp,
                attributes: json!({ "summary": summary }),
            })
            .unwrap();
    }
    store
        .insert_log(&LogRecord {
            session_id: session_id.to_string(),
            timestamp_unix_nanos: 1_910,
            observed_timestamp_unix_nanos: None,
            severity_text: Some("INFO".to_string()),
            severity_number: Some(9),
            body: Some("agent run done".to_string()),
            body_json: None,
            trace_id: Some(trace_id.to_string()),
            span_id: Some("agent-run".to_string()),
            scope_name: Some("agentive".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "agentive.run_id": run_id }),
            source: TelemetrySource::ThirdPartyOtel,
        })
        .unwrap();
}

fn insert_agentive_span(
    store: &SqliteStore,
    span_id: &str,
    parent_span_id: Option<&str>,
    name: &str,
    start_time_unix_nanos: i64,
    end_time_unix_nanos: i64,
    attributes: Value,
) {
    store
        .insert_span(&SpanRecord {
            session_id: "session-fixture".to_string(),
            trace_id: "a9ba86b6ef5906a2b7af3c8423ea9001".to_string(),
            span_id: span_id.to_string(),
            parent_span_id: parent_span_id.map(ToString::to_string),
            name: name.to_string(),
            kind: Some("internal".to_string()),
            start_time_unix_nanos,
            end_time_unix_nanos: Some(end_time_unix_nanos),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("agentive".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes,
            source: TelemetrySource::ThirdPartyOtel,
        })
        .unwrap();
}
