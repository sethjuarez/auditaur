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
    path::PathBuf,
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
    let store = SqliteStore::open(db.path()).unwrap();
    store
        .insert_tauri_window_state(&TauriWindowState {
            session_id: "session-fixture".to_string(),
            timestamp_unix_nanos: 183,
            window_label: "main".to_string(),
            webview_label: None,
            url: None,
            title: Some("Fixture".to_string()),
            focused: Some(false),
            visible: Some(true),
            width: Some(800.0),
            height: Some(600.0),
            scale_factor: Some(1.0),
            attributes: json!({
                "auditaur.capture": "window_event",
                "tauri.window.event": "blurred",
                "tauri.window.event.focused": false,
            }),
        })
        .unwrap();
    drop(store);
    let lifecycle_windows = run_json(["windows", "--db", db.path().to_str().unwrap(), "--json"]);
    let blurred_window = lifecycle_windows
        .as_array()
        .unwrap()
        .iter()
        .find(|window| window["attributes"]["tauri.window.event"] == "blurred")
        .unwrap();
    assert_eq!(
        blurred_window["attributes"]["tauri.window.event.focused"],
        false
    );

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
    assert!(timeline
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["kind"] == "window" && item["status"] == "blurred" }));

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
fn debug_status_reports_readiness_for_database_and_discovered_app() {
    let db = fixture_database();
    let status = run_json([
        "debug",
        "--db",
        db.path().to_str().unwrap(),
        "--json",
        "status",
    ]);
    assert_eq!(status["ready"], true);
    assert_eq!(status["telemetry"]["sessions"], 1);
    assert_eq!(status["telemetry"]["windows"], 1);
    assert_eq!(status["telemetry"]["frontendRecords"], 4);
    assert_eq!(
        status["stages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|stage| stage["name"] == "app_discovery")
            .unwrap()["status"],
        "skipped"
    );

    let data_dir = TempDir::new().unwrap();
    write_drive_fixture(data_dir.path(), "debug-instance");
    let discovered = run_json_with_env(
        ["debug", "--app", "fixture", "--json", "status"],
        data_dir.path().to_str().unwrap(),
    );

    assert_eq!(discovered["ready"], true);
    assert_eq!(discovered["app"]["serviceName"], "auditaur-fixture");
    assert_eq!(discovered["cdp"], serde_json::Value::Null);
    assert!(discovered["stages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage["name"] == "frontend_telemetry" && stage["status"] == "ok"));
}

#[test]
fn debug_status_reports_waiting_when_required_readiness_is_missing() {
    let data_dir = TempDir::new().unwrap();
    write_drive_fixture_without_windows(data_dir.path(), "debug-no-window");
    let missing_window = run_json_with_env(
        ["debug", "--app", "fixture", "--json", "status"],
        data_dir.path().to_str().unwrap(),
    );
    assert_eq!(missing_window["ready"], false);
    assert!(missing_window["stages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage["name"] == "window" && stage["status"] == "waiting"));

    let data_dir = TempDir::new().unwrap();
    write_drive_fixture_without_windows(data_dir.path(), "debug-no-frontend");
    let frontend_required = run_json_with_env(
        [
            "debug",
            "--app",
            "fixture",
            "--require-frontend",
            "--json",
            "status",
        ],
        data_dir.path().to_str().unwrap(),
    );
    assert_eq!(frontend_required["ready"], false);
    assert!(frontend_required["stages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage["name"] == "frontend_telemetry" && stage["status"] == "waiting"));
}

#[test]
fn init_skill_installs_auditaur_debug_skill() {
    let repo = TempDir::new().unwrap();
    let skill_path = repo
        .path()
        .join(".github")
        .join("skills")
        .join("auditaur-debug")
        .join("SKILL.md");

    let installed = run_json([
        "init",
        "skill",
        "--path",
        repo.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(installed["ok"], true);
    assert_eq!(installed["overwritten"], false);
    let skill = fs::read_to_string(&skill_path).unwrap();
    assert!(skill.contains("name: auditaur-debug"));
    assert!(skill.contains("auditaur debug --app <app-name>"));

    let failure = run_failure([
        "init",
        "skill",
        "--path",
        repo.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(failure.contains("already exists"));

    fs::write(&skill_path, "stale").unwrap();
    let overwritten = run_json([
        "init",
        "skill",
        "--path",
        repo.path().to_str().unwrap(),
        "--force",
        "--json",
    ]);
    assert_eq!(overwritten["overwritten"], true);
    assert!(fs::read_to_string(skill_path)
        .unwrap()
        .contains("name: auditaur-debug"));
}

#[test]
fn packaged_init_skill_matches_repo_skill() {
    let repo_skill =
        include_str!("../../../.github/skills/auditaur-debug/SKILL.md").replace("\r\n", "\n");
    let packaged_skill = include_str!("../assets/auditaur-debug-skill.md").replace("\r\n", "\n");
    assert_eq!(packaged_skill, repo_skill);
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
            session_id: "session-fixture".to_string(),
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
    assert_eq!(attach["sessionId"], "session-fixture");
    assert_eq!(attach["dbPath"], db_path.to_string_lossy().to_string());
    assert_eq!(attach["cdp"]["status"], "available");
    assert_eq!(attach["cdp"]["port"], port);
    assert_eq!(attach["cdp"]["product"], "Chrome/125.0.0.0");
    assert_eq!(attach["cdp"]["targetBindingStatus"], "matched");
    assert_eq!(
        attach["cdp"]["targetOwnershipStatus"],
        "matched_window_telemetry"
    );
    assert_eq!(attach["cdp"]["targets"][0]["id"], "target-fixture");
    assert_eq!(
        attach["cdp"]["targets"][0]["bindingStatus"],
        "matched_window_title"
    );
    assert_eq!(attach["cdp"]["targets"][0]["windowLabel"], "main");
    assert_eq!(
        attach["cdp"]["targets"][0]["ownershipStatus"],
        "matched_window_telemetry"
    );
    assert_eq!(attach["cdp"]["targets"][0]["ownershipProven"], false);
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
fn drive_inspect_reports_probable_unproven_target_ownership_guidance() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-probable-ownership");
    let (port, server) = start_fake_cdp_probable_endpoint();
    let port_arg = port.to_string();

    let attach = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "inspect",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(attach["cdp"]["targetBindingStatus"], "probable");
    assert_eq!(attach["cdp"]["targetOwnershipStatus"], "probable_unproven");
    assert!(attach["cdp"]["targetOwnershipNote"]
        .as_str()
        .unwrap()
        .contains("--allow-unproven-target"));
    assert_eq!(
        attach["cdp"]["targets"][0]["ownershipProof"],
        "single_window_single_target"
    );
    assert_eq!(attach["cdp"]["targets"][0]["ownershipProven"], false);
    assert!(attach["cdp"]["targets"][0]["ownershipGuidance"]
        .as_str()
        .unwrap()
        .contains("Mutating actions require --allow-unproven-target"));
}

#[test]
fn drive_resolves_explicit_session_id_when_app_name_is_ambiguous() {
    let temp = TempDir::new().unwrap();
    let first_db_path = temp
        .path()
        .join("sessions")
        .join("session-first")
        .join("telemetry.sqlite");
    fs::create_dir_all(first_db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&first_db_path));
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-first".to_string(),
            session_id: "session-first".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 41,
            started_at: "2026-05-18T18:00:00Z".to_string(),
            database_path: first_db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2000-01-01T00:00:00Z".to_string(),
        },
    );
    let second_db_path = temp
        .path()
        .join("sessions")
        .join("session-second")
        .join("telemetry.sqlite");
    fs::create_dir_all(second_db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&second_db_path));
    write_discovery_file(
        temp.path(),
        DiscoveryFile {
            schema_version: 1,
            instance_id: "instance-second".to_string(),
            session_id: "session-second".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:01:00Z".to_string(),
            database_path: second_db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2000-01-01T00:00:00Z".to_string(),
        },
    );

    let ambiguous =
        run_failure_with_env(["drive", "--app", "fixture"], temp.path().to_str().unwrap());
    assert!(ambiguous.contains("Multiple Auditaur apps matched"));
    assert!(ambiguous.contains("--session-id"));
    assert!(ambiguous.contains("session-first"));
    assert!(ambiguous.contains("session-second"));

    let selected = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--session-id",
            "second",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );
    assert_eq!(selected["sessionId"], "session-second");
    assert_eq!(selected["pid"], 42);
}

#[test]
fn drive_cdp_probe_waits_for_realistic_local_endpoint_latency() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-delayed-cdp");
    let (port, server) = start_fake_cdp_delayed_endpoint();
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
    assert_eq!(attach["cdp"]["status"], "available");
    assert_eq!(attach["cdp"]["targets"][0]["id"], "target-fixture");
}

#[test]
fn drive_cdp_probe_reads_content_length_without_waiting_for_socket_close() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-keepalive-cdp");
    let (port, server) = start_fake_cdp_keep_alive_endpoint();
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
    assert_eq!(attach["cdp"]["status"], "available");
    assert_eq!(attach["cdp"]["targets"][0]["id"], "target-fixture");
}

#[test]
fn drive_cdp_probe_reports_explicit_endpoint_errors() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-bad-cdp");
    let (port, server) = start_fake_cdp_invalid_version_endpoint();
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
    assert_eq!(attach["cdp"]["status"], "unavailable");
    assert!(attach["cdp"]["reason"]
        .as_str()
        .unwrap()
        .contains("invalid JSON"));
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
            session_id: "session-fixture".to_string(),
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
            session_id: "session-fixture".to_string(),
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
    assert_eq!(wait["sessionId"], "session-fixture");
    assert_eq!(wait["targetId"], "target-fixture");
    assert_eq!(wait["windowLabel"], "main");
    assert_eq!(
        wait["telemetryAttributes"]["auditaur.driver.action"],
        "wait"
    );
    assert_eq!(wait["telemetryAttributes"]["tauri.window.label"], "main");
    assert_eq!(wait["telemetryAttributes"]["auditaur.test_id"], "test-1");
}

#[test]
fn drive_wait_auto_selects_single_target_bound_to_observed_window() {
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
            instance_id: "instance-drive-bound".to_string(),
            session_id: "session-fixture".to_string(),
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
    let (port, server) = start_fake_cdp_one_bound_target_endpoint();
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
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(wait["ok"], true);
    assert_eq!(wait["targetId"], "target-fixture");
    assert_eq!(wait["windowLabel"], "main");
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
            session_id: "session-fixture".to_string(),
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
fn drive_exists_and_text_report_dom_values() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-actions");

    let (exists_port, exists_server) = start_fake_cdp_runtime_value_endpoint(json!(true));
    let exists_port_arg = exists_port.to_string();
    let exists = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &exists_port_arg,
            "--json",
            "exists",
            "--selector",
            "[data-testid=ready]",
        ],
        temp.path().to_str().unwrap(),
    );
    exists_server.join().unwrap();

    assert_eq!(exists["ok"], true);
    assert_eq!(exists["payload"]["exists"], true);
    assert_eq!(exists["selector"], "[data-testid=ready]");
    assert_eq!(exists["action"], "exists");
    assert_eq!(exists["mutatesApp"], false);

    let (text_port, text_server) =
        start_fake_cdp_runtime_value_endpoint(json!({ "found": true, "text": "Ready" }));
    let text_port_arg = text_port.to_string();
    let text = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &text_port_arg,
            "--json",
            "text",
            "--selector",
            "[data-testid=status]",
        ],
        temp.path().to_str().unwrap(),
    );
    text_server.join().unwrap();

    assert_eq!(text["ok"], true);
    assert_eq!(text["payload"]["text"], "Ready");
    assert_eq!(text["action"], "text");
    assert_eq!(text["mutatesApp"], false);
}

#[test]
fn drive_action_accepts_json_after_subcommand() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-subcommand-json");
    let (port, server) = start_fake_cdp_runtime_value_endpoint(json!(true));
    let port_arg = port.to_string();

    let exists = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "exists",
            "--selector",
            "[data-testid=ready]",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(exists["ok"], true);
    assert_eq!(exists["payload"]["exists"], true);
}

#[test]
fn drive_read_action_allows_probable_target_without_mutation_opt_in() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-probable-read");
    let (port, server) = start_fake_cdp_probable_runtime_value_endpoint(json!(true));
    let port_arg = port.to_string();

    let exists = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "exists",
            "--selector",
            "[data-testid=ready]",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(exists["ok"], true);
    assert_eq!(exists["targetOwnershipStatus"], "probable_unproven");
    assert_eq!(exists["ownershipProven"], false);
}

#[test]
fn drive_read_actions_report_not_found_as_json_failure() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-read-failures");

    let (exists_port, exists_server) = start_fake_cdp_runtime_value_endpoint(json!(false));
    let exists_port_arg = exists_port.to_string();
    let exists = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &exists_port_arg,
            "--json",
            "exists",
            "--selector",
            "[data-testid=missing]",
        ],
        temp.path().to_str().unwrap(),
    );
    exists_server.join().unwrap();
    assert!(exists.contains("\"ok\": false"));
    assert!(exists.contains("\"exists\": false"));
    assert!(exists.contains("Selector `[data-testid=missing]` was not found"));

    let (text_port, text_server) =
        start_fake_cdp_runtime_value_endpoint(json!({ "found": false, "text": null }));
    let text_port_arg = text_port.to_string();
    let text = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &text_port_arg,
            "--json",
            "text",
            "--selector",
            "[data-testid=missing]",
        ],
        temp.path().to_str().unwrap(),
    );
    text_server.join().unwrap();
    assert!(text.contains("\"ok\": false"));
    assert!(text.contains("\"found\": false"));
    assert!(text.contains("Selector `[data-testid=missing]` was not found"));
}

#[test]
fn drive_click_fill_and_press_report_action_telemetry() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-mutating-actions");

    let (click_port, click_server) = start_fake_cdp_runtime_value_endpoint(json!({
        "ok": true,
        "matched": true
    }));
    let click_port_arg = click_port.to_string();
    let click = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &click_port_arg,
            "--json",
            "click",
            "--selector",
            "button.save",
            "--allow-unproven-target",
            "--test-id",
            "cutready-smoke",
            "--step-id",
            "save",
        ],
        temp.path().to_str().unwrap(),
    );
    click_server.join().unwrap();
    assert_eq!(click["ok"], true);
    assert_eq!(click["action"], "click");
    assert_eq!(click["testId"], "cutready-smoke");
    assert_eq!(click["stepId"], "save");
    assert_eq!(click["selector"], "button.save");
    assert_eq!(click["mutatesApp"], true);

    let (fill_port, fill_server) = start_fake_cdp_runtime_value_endpoint(json!({
        "ok": true,
        "matched": true
    }));
    let fill_port_arg = fill_port.to_string();
    let fill = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &fill_port_arg,
            "--json",
            "fill",
            "--selector",
            "input[name=q]",
            "--value",
            "auditaur",
            "--allow-unproven-target",
        ],
        temp.path().to_str().unwrap(),
    );
    fill_server.join().unwrap();
    assert_eq!(fill["ok"], true);
    assert_eq!(fill["action"], "fill");
    assert_eq!(fill["selector"], "input[name=q]");

    let (press_port, press_server) = start_fake_cdp_runtime_value_endpoint(json!({
        "ok": true,
        "matched": true
    }));
    let press_port_arg = press_port.to_string();
    let press = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &press_port_arg,
            "--json",
            "press",
            "--key",
            "Enter",
            "--allow-unproven-target",
        ],
        temp.path().to_str().unwrap(),
    );
    press_server.join().unwrap();
    assert_eq!(press["ok"], true);
    assert_eq!(press["action"], "press");
    assert_eq!(press["selector"], "<active-element>");
}

#[test]
fn drive_mutating_action_requires_opt_in_for_probable_target() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-probable-mutation-blocked");
    let (port, server) = start_fake_cdp_probable_endpoint();
    let port_arg = port.to_string();

    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "click",
            "--selector",
            "button.save",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert!(failure.contains("ownership is not PID/session-proven"));
    assert!(failure.contains("--allow-unproven-target"));
}

#[test]
fn drive_mutating_action_requires_opt_in_for_matched_but_unproven_target() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-matched-mutation-blocked");
    let (port, server) = start_fake_cdp_endpoint();
    let port_arg = port.to_string();

    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "click",
            "--selector",
            "button.save",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert!(failure.contains("ownership is not PID/session-proven"));
    assert!(failure.contains("ownershipStatus=matched_window_telemetry"));
    assert!(failure.contains("--allow-unproven-target"));
}

#[test]
fn drive_mutating_action_requires_opt_in_for_unverified_target() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture_without_windows(temp.path(), "instance-drive-unverified-mutation-blocked");
    let (port, server) = start_fake_cdp_probable_endpoint();
    let port_arg = port.to_string();

    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "click",
            "--selector",
            "button.save",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert!(failure.contains("ownership is not PID/session-proven"));
    assert!(failure.contains("--allow-unproven-target"));
}

#[test]
fn drive_mutating_action_can_opt_into_probable_target() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-probable-mutation-allowed");
    let (port, server) = start_fake_cdp_probable_runtime_value_endpoint(json!({
        "ok": true,
        "matched": true
    }));
    let port_arg = port.to_string();

    let click = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "click",
            "--selector",
            "button.save",
            "--allow-unproven-target",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(click["ok"], true);
    assert_eq!(click["targetOwnershipStatus"], "probable_unproven");
    assert_eq!(click["ownershipProven"], false);
    assert_eq!(
        click["telemetryAttributes"]["auditaur.driver.target_ownership_status"],
        "probable_unproven"
    );
}

#[test]
fn drive_mutating_action_accepts_legacy_probable_opt_in_alias() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-probable-mutation-legacy-alias");
    let (port, server) = start_fake_cdp_probable_runtime_value_endpoint(json!({
        "ok": true,
        "matched": true
    }));
    let port_arg = port.to_string();

    let click = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "click",
            "--selector",
            "button.save",
            "--allow-probable-target",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(click["ok"], true);
    assert_eq!(click["targetOwnershipStatus"], "probable_unproven");
    assert_eq!(click["ownershipProven"], false);
}

#[test]
fn drive_mutating_action_reports_dom_error_as_json_failure() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-mutating-failure");

    let (port, server) = start_fake_cdp_runtime_value_endpoint(json!({
        "ok": false,
        "error": "selector not found"
    }));
    let port_arg = port.to_string();
    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
            "click",
            "--selector",
            "button.missing",
            "--allow-unproven-target",
        ],
        temp.path().to_str().unwrap(),
    );
    server.join().unwrap();

    assert!(failure.contains("\"ok\": false"));
    assert!(failure.contains("\"mutatesApp\": true"));
    assert!(failure.contains("\"error\": \"selector not found\""));
}

#[test]
fn drive_screenshot_writes_png_bytes_and_reports_target_context() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-screenshot");
    let output = temp.path().join("shot.png");
    let output_arg = output.to_string_lossy().to_string();
    let (port, server) = start_fake_cdp_screenshot_endpoint("aGVsbG8=");
    let port_arg = port.to_string();

    let screenshot = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
            "screenshot",
            "--output",
            &output_arg,
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(screenshot["ok"], true);
    assert_eq!(screenshot["action"], "screenshot");
    assert_eq!(screenshot["payload"]["output"], output_arg);
    assert_eq!(fs::read(output).unwrap(), b"hello");
}

#[test]
fn drive_failure_artifacts_write_screenshot_and_bounded_snapshot() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-failure-artifacts");
    let output_dir = temp.path().join("artifacts");
    fs::create_dir_all(&output_dir).unwrap();
    let screenshot = output_dir.join("failure.png");
    let snapshot = output_dir.join("failure.json");
    let screenshot_arg = screenshot.to_string_lossy().to_string();
    let snapshot_arg = snapshot.to_string_lossy().to_string();
    let (port, server) = start_fake_cdp_failure_artifacts_endpoint("aGVsbG8=");
    let port_arg = port.to_string();

    let artifacts = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
            "screenshot",
            "--output",
            &screenshot_arg,
            "--snapshot-output",
            &snapshot_arg,
            "--selector",
            "#failure",
            "--test-id",
            "smoke",
            "--step-id",
            "failed-click",
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(artifacts["ok"], true);
    assert_eq!(artifacts["action"], "screenshot");
    assert_eq!(artifacts["mutatesApp"], false);
    assert_eq!(artifacts["selector"], "#failure");
    assert_eq!(artifacts["payload"]["output"], screenshot_arg);
    assert_eq!(artifacts["payload"]["snapshot"], snapshot_arg);
    assert_eq!(artifacts["payload"]["snapshotTextLimitCharacters"], 65536);
    assert!(artifacts["payload"].get("snapshotError").is_none());
    assert_eq!(fs::read(screenshot).unwrap(), b"hello");
    let manifest: Value = serde_json::from_str(&fs::read_to_string(snapshot).unwrap()).unwrap();
    assert_eq!(manifest["action"], "screenshot");
    assert_eq!(manifest["selector"], "#failure");
    assert_eq!(manifest["testId"], "smoke");
    assert_eq!(manifest["stepId"], "failed-click");
    assert_eq!(manifest["snapshotTextLimitCharacters"], 65536);
    assert!(manifest["snapshotError"].is_null());
    assert_eq!(manifest["snapshot"]["title"], "Failure Page");
    assert_eq!(manifest["snapshot"]["selected"]["selector"], "#failure");
    assert_eq!(
        manifest["snapshot"]["selected"]["text"]["value"],
        "Save failed"
    );
}

#[test]
fn drive_screenshot_keeps_png_when_snapshot_capture_fails() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-snapshot-failure");
    let output_dir = temp.path().join("artifacts");
    fs::create_dir_all(&output_dir).unwrap();
    let screenshot = output_dir.join("failure.png");
    let snapshot = output_dir.join("failure.json");
    let screenshot_arg = screenshot.to_string_lossy().to_string();
    let snapshot_arg = snapshot.to_string_lossy().to_string();
    let (port, server) = start_fake_cdp_snapshot_error_endpoint("aGVsbG8=");
    let port_arg = port.to_string();

    let artifacts = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--cdp-port",
            &port_arg,
            "--json",
            "screenshot",
            "--output",
            &screenshot_arg,
            "--snapshot-output",
            &snapshot_arg,
        ],
        temp.path().to_str().unwrap(),
    );

    server.join().unwrap();
    assert_eq!(artifacts["ok"], true);
    assert_eq!(fs::read(screenshot).unwrap(), b"hello");
    assert!(artifacts["payload"]["snapshotError"]
        .as_str()
        .unwrap()
        .contains("CDP Runtime.evaluate failed"));
    let manifest: Value = serde_json::from_str(&fs::read_to_string(snapshot).unwrap()).unwrap();
    assert!(manifest["snapshot"].is_null());
    assert!(manifest["snapshotError"]
        .as_str()
        .unwrap()
        .contains("CDP Runtime.evaluate failed"));
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
    let middle_db_path = temp
        .path()
        .join("sessions")
        .join("session-middle")
        .join("telemetry.sqlite");
    fs::create_dir_all(middle_db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&middle_db_path));
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
            instance_id: "instance-middle".to_string(),
            session_id: "session-middle".to_string(),
            service_name: "auditaur-fixture".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.fixture".to_string()),
            pid: 42,
            started_at: "2026-05-18T18:00:05Z".to_string(),
            database_path: middle_db_path.to_string_lossy().to_string(),
            capabilities: expected_capabilities(),
            last_heartbeat_at: "2000-01-01T00:00:05Z".to_string(),
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
    assert_eq!(stale["supersededBySessionId"], "session-middle");
    assert_eq!(stale["secondsUntilNextStart"], 5);
    assert_eq!(stale["churnSessionCount"], 3);
    assert_eq!(stale["churnWindowSeconds"], 12);
    assert!(stale["churnHint"]
        .as_str()
        .unwrap()
        .contains("restart burst"));

    let health = run_json_with_env(["health", "--json"], temp.path().to_str().unwrap());
    let stale_health = health["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["sessionId"] == "session-stale")
        .unwrap();
    let churn_check = stale_health["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "session-churn")
        .unwrap();
    assert!(churn_check["message"]
        .as_str()
        .unwrap()
        .contains("3 sessions"));
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

#[test]
fn dogfood_smoke_telemetry_is_visible_through_cli_workflows() {
    let db = NamedTempFile::new().unwrap();
    let store = create_dogfood_smoke_database_at(db.path());
    drop(store);

    let sessions = run_json(["sessions", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(sessions[0]["serviceName"], "auditaur-dogfood-backend");
    assert_eq!(sessions[0]["sessionName"], "dogfood-manual-smoke");

    let logs = run_json(["logs", "--db", db.path().to_str().unwrap(), "--json"]);
    assert!(logs.as_array().unwrap().iter().any(|log| {
        log["body"] == "Dogfood console log" && log["attributes"]["source"] == "frontend"
    }));
    assert!(logs.as_array().unwrap().iter().any(|log| {
        log["body"] == "failing command rejected request"
            && log["attributes"]["error"]
                .as_str()
                .unwrap()
                .contains("Intentional dogfood backend failure")
    }));

    let errors = run_json(["errors", "--db", db.path().to_str().unwrap(), "--json"]);
    assert!(errors.as_array().unwrap().iter().any(|error| {
        error["message"] == "Intentional dogfood frontend error" && error["windowLabel"] == "main"
    }));

    let ipc = run_json(["ipc", "--db", db.path().to_str().unwrap(), "--json"]);
    assert!(ipc
        .as_array()
        .unwrap()
        .iter()
        .any(|call| { call["command"] == "successful_command" && call["status"] == "OK" }));
    let failed_call = ipc
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["command"] == "failing_command")
        .unwrap();
    assert_eq!(failed_call["status"], "ERROR");
    assert!(failed_call["errorMessage"]
        .as_str()
        .unwrap()
        .contains("Intentional dogfood backend failure"));

    let events = run_json(["events", "--db", db.path().to_str().unwrap(), "--json"]);
    assert!(events.as_array().unwrap().iter().any(|event| {
        event["eventName"] == "dogfood:frontend-event" && event["direction"] == "emit"
    }));
    assert!(events.as_array().unwrap().iter().any(|event| {
        event["eventName"] == "dogfood:backend-event" && event["direction"] == "listen"
    }));

    let windows = run_json(["windows", "--db", db.path().to_str().unwrap(), "--json"]);
    assert!(windows.as_array().unwrap().iter().any(|window| {
        window["windowLabel"] == "main" && window["title"] == "Auditaur Dogfood"
    }));

    let traces = run_json(["traces", "--db", db.path().to_str().unwrap(), "--json"]);
    let failed_trace = traces
        .as_array()
        .unwrap()
        .iter()
        .find(|trace| trace["traceId"] == "trace-dogfood-failing")
        .unwrap();
    assert_eq!(failed_trace["errorCount"], 2);
    assert_eq!(failed_trace["spanCount"], 2);

    let trace = run_json([
        "trace",
        "trace-dogfood-failing",
        "--db",
        db.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(trace["spans"].as_array().unwrap().iter().any(|span| {
        span["name"] == "tauri.invoke failing_command" && span["statusCode"] == "ERROR"
    }));
    assert!(trace["spans"].as_array().unwrap().iter().any(|span| {
        span["name"] == "failing_command" && span["parentSpanId"] == "frontend-failing-span"
    }));

    let explain = run_json([
        "explain",
        "--db",
        db.path().to_str().unwrap(),
        "--trace",
        "trace-dogfood-failing",
        "--json",
    ]);
    assert_eq!(explain["failedIpcCount"], 1);
    assert!(!explain["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding
            .as_str()
            .unwrap()
            .contains("Missing backend trace continuation")));

    let bundle = run_json([
        "bundle",
        "--db",
        db.path().to_str().unwrap(),
        "--trace",
        "trace-dogfood-failing",
    ]);
    assert_eq!(bundle["redacted"], true);
    assert_eq!(bundle["tauriIpcCalls"][0]["argsJson"], "[redacted]");
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

fn write_drive_fixture(root: &std::path::Path, instance_id: &str) -> PathBuf {
    let db_path = root
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_at(&db_path));
    write_discovery_file(
        root,
        DiscoveryFile {
            schema_version: 1,
            instance_id: instance_id.to_string(),
            session_id: "session-fixture".to_string(),
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
    db_path
}

fn write_drive_fixture_without_windows(root: &std::path::Path, instance_id: &str) -> PathBuf {
    let db_path = root
        .join("sessions")
        .join("session-fixture")
        .join("telemetry.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    drop(create_fixture_database_without_windows_at(&db_path));
    write_discovery_file(
        root,
        DiscoveryFile {
            schema_version: 1,
            instance_id: instance_id.to_string(),
            session_id: "session-fixture".to_string(),
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
    db_path
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

fn start_fake_cdp_probable_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &probable_target_list_json(port));
    });
    (port, handle)
}

fn start_fake_cdp_delayed_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json_after_delay(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
            std::time::Duration::from_millis(300),
        );
        respond_http_json_after_delay(
            &listener,
            &target_list_json(port),
            std::time::Duration::from_millis(300),
        );
    });
    (port, handle)
}

fn start_fake_cdp_keep_alive_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json_keep_alive(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json_keep_alive(&listener, &target_list_json(port));
    });
    (port, handle)
}

fn start_fake_cdp_invalid_version_endpoint() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(&listener, "{not-json");
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

fn start_fake_cdp_runtime_value_endpoint(value: Value) -> (u16, thread::JoinHandle<()>) {
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
                            "type": cdp_runtime_type(&value),
                            "value": value
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

fn start_fake_cdp_probable_runtime_value_endpoint(value: Value) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &probable_target_list_json(port));
        respond_websocket_runtime_value(&listener, value);
    });
    (port, handle)
}

fn start_fake_cdp_screenshot_endpoint(data: &'static str) -> (u16, thread::JoinHandle<()>) {
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
                "Page.enable" => json!({ "id": id, "result": {} }),
                "Page.captureScreenshot" => json!({
                    "id": id,
                    "result": {
                        "data": data
                    }
                }),
                _ => json!({ "id": id, "error": { "message": "unexpected method" } }),
            };
            websocket
                .send(Message::Text(response.to_string().into()))
                .unwrap();
            if method == "Page.captureScreenshot" {
                break;
            }
        }
    });
    (port, handle)
}

fn start_fake_cdp_failure_artifacts_endpoint(data: &'static str) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &target_list_json(port));
        respond_websocket_screenshot(&listener, data);
        respond_websocket_runtime_value(
            &listener,
            json!({
                "title": "Failure Page",
                "url": "tauri://localhost/failure",
                "bodyText": {
                    "value": "Save failed",
                    "truncated": false,
                    "length": 11
                },
                "html": {
                    "value": "<html><body><button id=\"failure\">Save failed</button></body></html>",
                    "truncated": false,
                    "length": 65
                },
                "selected": {
                    "selector": "#failure",
                    "text": {
                        "value": "Save failed",
                        "truncated": false,
                        "length": 11
                    },
                    "html": {
                        "value": "<button id=\"failure\">Save failed</button>",
                        "truncated": false,
                        "length": 41
                    }
                }
            }),
        );
    });
    (port, handle)
}

fn start_fake_cdp_snapshot_error_endpoint(data: &'static str) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        respond_http_json(
            &listener,
            r#"{"Browser":"Chrome/125.0.0.0","Protocol-Version":"1.3"}"#,
        );
        respond_http_json(&listener, &target_list_json(port));
        respond_websocket_screenshot(&listener, data);
        respond_websocket_runtime_error(&listener, "snapshot failed");
    });
    (port, handle)
}

fn respond_websocket_screenshot(listener: &TcpListener, data: &'static str) {
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
            "Page.enable" => json!({ "id": id, "result": {} }),
            "Page.captureScreenshot" => json!({
                "id": id,
                "result": {
                    "data": data
                }
            }),
            _ => json!({ "id": id, "error": { "message": "unexpected method" } }),
        };
        websocket
            .send(Message::Text(response.to_string().into()))
            .unwrap();
        if method == "Page.captureScreenshot" {
            break;
        }
    }
}

fn respond_websocket_runtime_error(listener: &TcpListener, error_message: &'static str) {
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
                    "exceptionDetails": {
                        "text": error_message
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
}

fn respond_websocket_runtime_value(listener: &TcpListener, value: Value) {
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
                        "type": cdp_runtime_type(&value),
                        "value": value
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
}

fn cdp_runtime_type(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Null => "undefined",
        Value::Array(_) | Value::Object(_) => "object",
    }
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

fn start_fake_cdp_one_bound_target_endpoint() -> (u16, thread::JoinHandle<()>) {
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
                    "id": "target-unrelated",
                    "type": "page",
                    "title": "Other",
                    "url": "tauri://localhost/other",
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/other")
                },
                {
                    "id": "target-fixture",
                    "type": "page",
                    "title": "Fixture",
                    "url": "tauri://localhost/",
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/target-fixture")
                }
            ])
            .to_string(),
        );
        respond_websocket_evaluate_true(&listener);
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

fn respond_websocket_evaluate_true(listener: &TcpListener) {
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

fn probable_target_list_json(port: u16) -> String {
    json!([
        {
            "id": "target-probable",
            "type": "page",
            "title": "Auditaur Drive Test",
            "url": "http://127.0.0.1/driver-test",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/target-probable")
        }
    ])
    .to_string()
}

fn respond_http_json(listener: &TcpListener, body: &str) {
    respond_http_json_after_delay(listener, body, std::time::Duration::ZERO);
}

fn respond_http_json_keep_alive(listener: &TcpListener, body: &str) {
    let (mut stream, _) = listener.accept().unwrap();
    let mut request = [0_u8; 512];
    let _ = stream.read(&mut request).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_secs(3));
        drop(stream);
    });
}

fn respond_http_json_after_delay(listener: &TcpListener, body: &str, delay: std::time::Duration) {
    let (mut stream, _) = listener.accept().unwrap();
    let mut request = [0_u8; 512];
    let _ = stream.read(&mut request).unwrap();
    thread::sleep(delay);
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

fn create_fixture_database_without_windows_at(path: &std::path::Path) -> SqliteStore {
    let store = SqliteStore::open(path).unwrap();
    store.migrate().unwrap();

    store
        .create_session(&Session {
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
        })
        .unwrap();

    store
}

fn create_dogfood_smoke_database_at(path: &std::path::Path) -> SqliteStore {
    let store = SqliteStore::open(path).unwrap();
    store.migrate().unwrap();

    let session = Session {
        id: "session-dogfood".to_string(),
        session_name: Some("dogfood-manual-smoke".to_string()),
        service_name: "auditaur-dogfood-backend".to_string(),
        service_version: Some("0.2.1".to_string()),
        app_identifier: Some("dev.auditaur.dogfood".to_string()),
        pid: Some(4242),
        started_at: "2026-06-12T16:00:00Z".to_string(),
        ended_at: None,
        schema_version: SQLITE_SCHEMA_VERSION,
        auditaur_version: Some("0.2.1".to_string()),
    };
    store.create_session(&session).unwrap();

    store
        .insert_span(&SpanRecord {
            session_id: session.id.clone(),
            trace_id: "trace-dogfood-success".to_string(),
            span_id: "frontend-success-span".to_string(),
            parent_span_id: None,
            name: "tauri.invoke successful_command".to_string(),
            kind: Some("client".to_string()),
            start_time_unix_nanos: 1_000,
            end_time_unix_nanos: Some(2_000),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("@auditaur/api".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "tauri.command": "successful_command" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_span(&SpanRecord {
            session_id: session.id.clone(),
            trace_id: "trace-dogfood-success".to_string(),
            span_id: "backend-success-span".to_string(),
            parent_span_id: Some("frontend-success-span".to_string()),
            name: "successful_command".to_string(),
            kind: Some("internal".to_string()),
            start_time_unix_nanos: 1_250,
            end_time_unix_nanos: Some(1_750),
            status_code: Some("OK".to_string()),
            status_message: None,
            scope_name: Some("dogfood-backend".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "auditaur.example.message": "hello from Auditaur" }),
            source: TelemetrySource::Backend,
        })
        .unwrap();
    store
        .insert_tauri_ipc_call(&TauriIpcCall {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 1_100,
            duration_ms: Some(1.0),
            command: "successful_command".to_string(),
            status: "OK".to_string(),
            error_message: None,
            trace_id: Some("trace-dogfood-success".to_string()),
            span_id: Some("frontend-success-span".to_string()),
            window_label: Some("main".to_string()),
            args_json: Some(json!({ "message": "hello from Auditaur" })),
            args_redacted: true,
            result_summary: Some("\"Backend received: hello from Auditaur\"".to_string()),
        })
        .unwrap();

    store
        .insert_span(&SpanRecord {
            session_id: session.id.clone(),
            trace_id: "trace-dogfood-failing".to_string(),
            span_id: "frontend-failing-span".to_string(),
            parent_span_id: None,
            name: "tauri.invoke failing_command".to_string(),
            kind: Some("client".to_string()),
            start_time_unix_nanos: 3_000,
            end_time_unix_nanos: Some(4_000),
            status_code: Some("ERROR".to_string()),
            status_message: Some(
                "Intentional dogfood backend failure: the dogfood button requested a failure"
                    .to_string(),
            ),
            scope_name: Some("@auditaur/api".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "tauri.command": "failing_command" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_span(&SpanRecord {
            session_id: session.id.clone(),
            trace_id: "trace-dogfood-failing".to_string(),
            span_id: "backend-failing-span".to_string(),
            parent_span_id: Some("frontend-failing-span".to_string()),
            name: "failing_command".to_string(),
            kind: Some("internal".to_string()),
            start_time_unix_nanos: 3_250,
            end_time_unix_nanos: Some(3_750),
            status_code: Some("ERROR".to_string()),
            status_message: Some(
                "Intentional dogfood backend failure: the dogfood button requested a failure"
                    .to_string(),
            ),
            scope_name: Some("dogfood-backend".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "error": "Intentional dogfood backend failure: the dogfood button requested a failure" }),
            source: TelemetrySource::Backend,
        })
        .unwrap();
    store
        .insert_tauri_ipc_call(&TauriIpcCall {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 3_100,
            duration_ms: Some(1.0),
            command: "failing_command".to_string(),
            status: "ERROR".to_string(),
            error_message: Some(
                "Intentional dogfood backend failure: the dogfood button requested a failure"
                    .to_string(),
            ),
            trace_id: Some("trace-dogfood-failing".to_string()),
            span_id: Some("frontend-failing-span".to_string()),
            window_label: Some("main".to_string()),
            args_json: Some(json!({ "reason": "the dogfood button requested a failure" })),
            args_redacted: true,
            result_summary: None,
        })
        .unwrap();

    store
        .insert_log(&LogRecord {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 500,
            observed_timestamp_unix_nanos: Some(505),
            severity_text: Some("INFO".to_string()),
            severity_number: Some(9),
            body: Some("Dogfood console log".to_string()),
            body_json: Some(json!({
                "source": "frontend",
                "secret": "[redacted]",
            })),
            trace_id: None,
            span_id: None,
            scope_name: Some("console".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "source": "frontend" }),
            source: TelemetrySource::Frontend,
        })
        .unwrap();
    store
        .insert_log(&LogRecord {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 3_300,
            observed_timestamp_unix_nanos: Some(3_305),
            severity_text: Some("ERROR".to_string()),
            severity_number: Some(17),
            body: Some("failing command rejected request".to_string()),
            body_json: None,
            trace_id: Some("trace-dogfood-failing".to_string()),
            span_id: Some("backend-failing-span".to_string()),
            scope_name: Some("dogfood-backend".to_string()),
            scope_version: Some("0.2.1".to_string()),
            attributes: json!({ "error": "Intentional dogfood backend failure: the dogfood button requested a failure" }),
            source: TelemetrySource::Backend,
        })
        .unwrap();
    store
        .insert_frontend_error(&FrontendError {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 600,
            message: "Intentional dogfood frontend error".to_string(),
            stack: Some("Error: Intentional dogfood frontend error".to_string()),
            filename: Some("src/main.ts".to_string()),
            line_number: Some(58),
            column_number: Some(13),
            error_type: Some("Error".to_string()),
            trace_id: None,
            span_id: None,
            window_label: Some("main".to_string()),
            attributes: json!({ "auditaur.source": "window.onerror" }),
        })
        .unwrap();
    store
        .insert_tauri_event(&TauriEventRecord {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 700,
            event_name: "dogfood:frontend-event".to_string(),
            direction: "emit".to_string(),
            target: Some("main".to_string()),
            trace_id: None,
            span_id: None,
            window_label: Some("main".to_string()),
            payload_summary: Some(
                "{\"source\":\"frontend\",\"message\":\"hello from the webview\"}".to_string(),
            ),
            payload_json: Some(json!({
                "source": "frontend",
                "message": "hello from the webview",
            })),
            payload_redacted: false,
        })
        .unwrap();
    store
        .insert_tauri_event(&TauriEventRecord {
            session_id: session.id.clone(),
            timestamp_unix_nanos: 800,
            event_name: "dogfood:backend-event".to_string(),
            direction: "listen".to_string(),
            target: Some("main".to_string()),
            trace_id: None,
            span_id: None,
            window_label: Some("main".to_string()),
            payload_summary: Some(
                "{\"source\":\"backend\",\"message\":\"hello from Rust\"}".to_string(),
            ),
            payload_json: Some(json!({
                "source": "backend",
                "message": "hello from Rust",
            })),
            payload_redacted: false,
        })
        .unwrap();
    store
        .insert_tauri_window_state(&TauriWindowState {
            session_id: session.id,
            timestamp_unix_nanos: 400,
            window_label: "main".to_string(),
            webview_label: None,
            url: Some("tauri://localhost".to_string()),
            title: Some("Auditaur Dogfood".to_string()),
            focused: Some(true),
            visible: Some(true),
            width: Some(1024.0),
            height: Some(768.0),
            scale_factor: Some(1.0),
            attributes: json!({ "auditaur.capture_phase": "startup" }),
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
