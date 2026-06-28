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
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::{NamedTempFile, TempDir};

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
    assert!(discovered["stages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage["name"] == "drive_bridge" && stage["status"] == "skipped"));

    let data_dir = TempDir::new().unwrap();
    let db_path = write_drive_fixture(data_dir.path(), "debug-drive-bridge");
    activate_drive_bridge(&db_path, "main");
    let drive_ready = run_json_with_env(
        [
            "debug",
            "--app",
            "fixture",
            "--require-drive-bridge",
            "--json",
            "status",
        ],
        data_dir.path().to_str().unwrap(),
    );
    assert_eq!(drive_ready["ready"], true);
    assert!(drive_ready["stages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage["name"] == "drive_bridge" && stage["status"] == "ok"));
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

    let data_dir = TempDir::new().unwrap();
    write_drive_fixture(data_dir.path(), "debug-no-drive-bridge");
    let drive_bridge_required = run_json_with_env(
        [
            "debug",
            "--app",
            "fixture",
            "--require-drive-bridge",
            "--json",
            "status",
        ],
        data_dir.path().to_str().unwrap(),
    );
    assert_eq!(drive_bridge_required["ready"], false);
    assert!(drive_bridge_required["stages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage["name"] == "drive_bridge" && stage["status"] == "waiting"));
}

#[test]
fn drill_run_help_exposes_first_slice_options() {
    let help = run_stdout(["drill", "run", "--help"]);
    assert!(help.contains("--app"));
    assert!(help.contains("--require-frontend"));
    assert!(help.contains("--require-drive-bridge"));
    assert!(help.contains("--expect-text"));
    assert!(help.contains("--script"));
    assert!(help.contains("--report"));
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

    let agents_skill_path = repo
        .path()
        .join(".agents")
        .join("skills")
        .join("auditaur-debug")
        .join("SKILL.md");
    let agents_installed = run_json([
        "init",
        "skill",
        "--path",
        repo.path().to_str().unwrap(),
        "--agents-path",
        "--json",
    ]);
    assert_eq!(agents_installed["ok"], true);
    assert_eq!(
        agents_installed["path"],
        agents_skill_path.to_string_lossy().to_string()
    );
    assert!(fs::read_to_string(agents_skill_path)
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
fn drive_reports_tauri_native_attach_info_with_legacy_port_flag() {
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

    let attach = run_json_with_env(
        ["drive", "--app", "fixture", "--cdp-port", "65535", "--json"],
        temp.path().to_str().unwrap(),
    );

    assert_eq!(attach["serviceName"], "auditaur-fixture");
    assert_eq!(attach["pid"], 42);
    assert_eq!(attach["sessionId"], "session-fixture");
    assert_eq!(attach["dbPath"], db_path.to_string_lossy().to_string());
    assert_eq!(attach["cdp"]["status"], "unavailable");
    assert!(attach["cdp"]["port"].is_null());
    assert!(attach["cdp"]["reason"]
        .as_str()
        .unwrap()
        .contains("Tauri-native in-app driver"));
    assert!(attach["cdp"]["launchHint"]
        .as_str()
        .unwrap()
        .contains("Tauri-native in-app driver"));
    assert_eq!(
        attach["platformBackend"]["selectorBackend"],
        "tauri_in_app_driver"
    );
    assert_eq!(
        attach["platformBackend"]["status"],
        "supported_with_drive_bridge"
    );
    assert_eq!(attach["platformBackend"]["selectorActionsSupported"], true);
    assert!(attach["futureActions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["name"] == "click"));
    assert!(attach["futureActions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|action| !action["description"].as_str().unwrap().contains("CDP")));
    assert!(attach["requiredActionTelemetry"]
        .as_array()
        .unwrap()
        .contains(&json!("auditaur.test_id")));
}

#[test]
fn drive_inspect_reports_active_in_app_bridge() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-bridge-inspect");
    activate_drive_bridge(&db_path, "main");

    let attach = run_json_with_env(
        ["drive", "--app", "fixture", "inspect", "--json"],
        temp.path().to_str().unwrap(),
    );

    assert_eq!(attach["bridge"]["status"], "active");
    assert_eq!(attach["bridge"]["active"], true);
    assert_eq!(attach["bridge"]["windowLabel"], "main");
    assert_eq!(attach["bridge"]["protocolVersion"], 1);
    assert_eq!(attach["bridge"]["targets"][0]["id"], "auditaur-bridge");
    assert_eq!(
        attach["bridge"]["targets"][0]["ownershipStatus"],
        "proven_session_bridge"
    );
}

#[test]
fn drive_bridge_stale_heartbeat_still_attempts_native_wake_path() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-bridge-stale-heartbeat");
    activate_drive_bridge_with_heartbeat(&db_path, "main", 1);

    let attach = run_json_with_env(
        ["drive", "--app", "fixture", "inspect", "--json"],
        temp.path().to_str().unwrap(),
    );
    assert_eq!(attach["bridge"]["status"], "stale");
    assert_eq!(attach["bridge"]["active"], true);
    assert!(attach["bridge"]["reason"]
        .as_str()
        .unwrap()
        .contains("native request wake path"));
    assert_eq!(attach["bridge"]["targets"][0]["id"], "auditaur-bridge");

    let worker =
        start_fake_drive_bridge_worker(&db_path, json!({ "exists": true, "visibleOnly": false }));
    let exists = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "exists",
            "--selector",
            "[data-testid=ready]",
        ],
        temp.path().to_str().unwrap(),
    );
    let request = worker.join().unwrap();

    assert_eq!(exists["ok"], true);
    assert_eq!(request["windowLabel"], "main");
}

#[test]
fn drive_bridge_exists_fill_and_snapshot_through_bridge() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-bridge-actions");
    activate_drive_bridge(&db_path, "main");

    let exists_worker =
        start_fake_drive_bridge_worker(&db_path, json!({ "exists": true, "visibleOnly": false }));
    let exists = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "exists",
            "--selector",
            "[data-testid=ready]",
        ],
        temp.path().to_str().unwrap(),
    );
    let exists_request = exists_worker.join().unwrap();
    assert_eq!(exists["ok"], true);
    assert_eq!(exists["targetId"], "auditaur-bridge");
    assert_eq!(exists["targetOwnershipStatus"], "proven_session_bridge");
    assert_eq!(exists["payload"]["exists"], true);
    assert_eq!(exists_request["action"], "exists");

    let fill_worker = start_fake_drive_bridge_worker(&db_path, json!({ "ok": true }));
    let fill = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "fill",
            "--selector",
            "input[name=q]",
            "--value",
            "auditaur",
        ],
        temp.path().to_str().unwrap(),
    );
    let fill_request = fill_worker.join().unwrap();
    assert_eq!(fill["ok"], true);
    assert_eq!(fill["mutatesApp"], true);
    assert_eq!(fill_request["action"], "fill");
    assert_eq!(fill_request["value"], "auditaur");

    let snapshot_path = temp.path().join("snapshot.json");
    let snapshot_arg = snapshot_path.to_string_lossy().to_string();
    let snapshot_payload = json!({
        "title": "Dogfood",
        "url": "tauri://localhost/",
        "selected": { "selector": "body", "text": { "value": "Ready" } }
    });
    let snapshot_worker = start_fake_drive_bridge_worker(&db_path, snapshot_payload.clone());
    let snapshot = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "snapshot",
            "--selector",
            "body",
            "--output",
            &snapshot_arg,
        ],
        temp.path().to_str().unwrap(),
    );
    snapshot_worker.join().unwrap();
    assert_eq!(snapshot["ok"], true);
    assert_eq!(snapshot["action"], "snapshot");
    assert_eq!(snapshot["payload"]["title"], "Dogfood");
    let written: Value = serde_json::from_str(&fs::read_to_string(snapshot_path).unwrap()).unwrap();
    assert_eq!(written, snapshot_payload);
}

#[test]
fn drive_bridge_type_press_and_screenshot_through_bridge() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-bridge-parity");
    activate_drive_bridge(&db_path, "main");

    let type_worker = start_fake_drive_bridge_worker(
        &db_path,
        json!({ "ok": true, "visibleOnly": true, "insertedCharacters": 5 }),
    );
    let typed = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "type",
            "--allow-unproven-target",
            "--visible-only",
            "--selector",
            "textarea",
            "--value",
            "hello",
        ],
        temp.path().to_str().unwrap(),
    );
    let type_request = type_worker.join().unwrap();
    assert_eq!(typed["ok"], true);
    assert_eq!(typed["action"], "type");
    assert_eq!(typed["mutatesApp"], true);
    assert_eq!(type_request["action"], "type");
    assert_eq!(type_request["value"], "hello");
    assert_eq!(type_request["visibleOnly"], true);
    assert_eq!(type_request["windowLabel"], "main");

    let press_worker = start_fake_drive_bridge_worker(&db_path, json!({ "ok": true }));
    let pressed = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "press",
            "--selector",
            "textarea",
            "--key",
            "Enter",
        ],
        temp.path().to_str().unwrap(),
    );
    let press_request = press_worker.join().unwrap();
    assert_eq!(pressed["ok"], true);
    assert_eq!(pressed["action"], "press");
    assert_eq!(press_request["action"], "press");
    assert_eq!(press_request["value"], "Enter");

    let active_press_worker = start_fake_drive_bridge_worker(&db_path, json!({ "ok": true }));
    let active_pressed = run_json_with_env(
        [
            "drive", "--app", "fixture", "--json", "press", "--key", "Escape",
        ],
        temp.path().to_str().unwrap(),
    );
    let active_press_request = active_press_worker.join().unwrap();
    assert_eq!(active_pressed["ok"], true);
    assert_eq!(active_pressed["selector"], "<active-element>");
    assert_eq!(active_press_request["action"], "press");
    assert!(active_press_request
        .get("selector")
        .is_none_or(Value::is_null));
    assert_eq!(active_press_request["value"], "Escape");

    let screenshot_path = temp.path().join("bridge.png");
    let manifest_path = temp.path().join("bridge.json");
    let screenshot_arg = screenshot_path.to_string_lossy().to_string();
    let manifest_arg = manifest_path.to_string_lossy().to_string();
    let screenshot_worker = start_fake_drive_bridge_worker(
        &db_path,
        json!({
            "format": "png",
            "pngBase64": "aGVsbG8=",
            "width": 320,
            "height": 240,
            "snapshot": { "title": "Bridge Page" }
        }),
    );
    let screenshot = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "screenshot",
            "--selector",
            "body",
            "--output",
            &screenshot_arg,
            "--snapshot-output",
            &manifest_arg,
        ],
        temp.path().to_str().unwrap(),
    );
    let screenshot_request = screenshot_worker.join().unwrap();
    assert_eq!(screenshot["ok"], true);
    assert_eq!(screenshot["action"], "screenshot");
    assert_eq!(screenshot["targetId"], "auditaur-bridge");
    assert_eq!(screenshot["payload"]["output"], screenshot_arg);
    assert_eq!(screenshot["payload"]["format"], "png");
    assert!(screenshot["payload"].get("pngBase64").is_none());
    assert_eq!(fs::read(screenshot_path).unwrap(), b"hello");
    assert_eq!(screenshot_request["action"], "screenshot");
    assert_eq!(screenshot_request["selector"], "body");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["snapshot"]["title"], "Bridge Page");
    assert_eq!(manifest["targetId"], "auditaur-bridge");
}

#[test]
fn drive_bridge_hover_select_check_and_evaluate_through_bridge() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-bridge-more-actions");
    activate_drive_bridge(&db_path, "main");

    let hover_worker = start_fake_drive_bridge_worker(&db_path, json!({ "ok": true }));
    let hover = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "hover",
            "--allow-probable-target",
            "--selector",
            "button.menu",
        ],
        temp.path().to_str().unwrap(),
    );
    let hover_request = hover_worker.join().unwrap();
    assert_eq!(hover["ok"], true);
    assert_eq!(hover["action"], "hover");
    assert_eq!(hover_request["action"], "hover");
    assert_eq!(hover_request["selector"], "button.menu");

    let select_worker = start_fake_drive_bridge_worker(
        &db_path,
        json!({ "ok": true, "selectedValues": ["one", "two"], "missingValues": [] }),
    );
    let selected = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "select",
            "--selector",
            "select[name=choice]",
            "--value",
            "one",
            "--value",
            "two",
        ],
        temp.path().to_str().unwrap(),
    );
    let select_request = select_worker.join().unwrap();
    assert_eq!(selected["ok"], true);
    assert_eq!(selected["action"], "select");
    assert_eq!(select_request["action"], "select");
    assert_eq!(select_request["value"], "one");
    assert_eq!(select_request["values"], json!(["one", "two"]));

    let check_worker =
        start_fake_drive_bridge_worker(&db_path, json!({ "ok": true, "checked": true }));
    let checked = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "check",
            "--selector",
            "input[type=checkbox]",
        ],
        temp.path().to_str().unwrap(),
    );
    let check_request = check_worker.join().unwrap();
    assert_eq!(checked["ok"], true);
    assert_eq!(checked["action"], "check");
    assert_eq!(check_request["action"], "check");

    let evaluate_worker =
        start_fake_drive_bridge_worker(&db_path, json!({ "ok": true, "value": 42 }));
    let evaluated = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "evaluate",
            "--expression",
            "window.answer",
        ],
        temp.path().to_str().unwrap(),
    );
    let evaluate_request = evaluate_worker.join().unwrap();
    assert_eq!(evaluated["ok"], true);
    assert_eq!(evaluated["action"], "evaluate");
    assert_eq!(evaluated["payload"]["value"], 42);
    assert_eq!(evaluate_request["action"], "evaluate");
    assert_eq!(evaluate_request["value"], "window.answer");
}

#[test]
fn drive_bridge_wait_repeats_until_selector_exists() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-bridge-wait");
    activate_drive_bridge(&db_path, "main");
    let worker = start_fake_drive_bridge_sequence_worker(
        &db_path,
        vec![
            json!({ "exists": false, "visibleOnly": false }),
            json!({ "exists": true, "visibleOnly": false }),
        ],
    );

    let wait = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "--json",
            "wait",
            "--selector",
            "[data-testid=ready]",
            "--timeout-ms",
            "1000",
        ],
        temp.path().to_str().unwrap(),
    );

    let requests = worker.join().unwrap();
    assert_eq!(wait["ok"], true);
    assert_eq!(wait["targetId"], "auditaur-bridge");
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request["action"] == "exists"));
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
fn drive_wait_requires_active_bridge() {
    let temp = TempDir::new().unwrap();
    write_drive_fixture(temp.path(), "instance-drive-wait-no-bridge");
    let failure = run_failure_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "wait",
            "--selector",
            "[data-testid=ready]",
        ],
        temp.path().to_str().unwrap(),
    );
    assert!(failure.contains("drive bridge is not active"));
}

#[test]
fn drive_action_accepts_json_after_subcommand() {
    let temp = TempDir::new().unwrap();
    let db_path = write_drive_fixture(temp.path(), "instance-drive-subcommand-json");
    activate_drive_bridge(&db_path, "main");
    let worker =
        start_fake_drive_bridge_worker(&db_path, json!({ "exists": true, "visibleOnly": false }));

    let exists = run_json_with_env(
        [
            "drive",
            "--app",
            "fixture",
            "exists",
            "--selector",
            "[data-testid=ready]",
            "--json",
        ],
        temp.path().to_str().unwrap(),
    );

    worker.join().unwrap();
    assert_eq!(exists["ok"], true);
    assert_eq!(exists["payload"]["exists"], true);
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
        "drive_bridge".to_string(),
    ]
}

fn activate_drive_bridge(db_path: &std::path::Path, window_label: &str) {
    activate_drive_bridge_with_heartbeat(db_path, window_label, now_unix_nanos());
}

fn activate_drive_bridge_with_heartbeat(
    db_path: &std::path::Path,
    window_label: &str,
    last_heartbeat_unix_nanos: i64,
) {
    let bridge_dir = db_path.parent().unwrap().join("drive-bridge");
    fs::create_dir_all(bridge_dir.join("requests")).unwrap();
    fs::create_dir_all(bridge_dir.join("in-flight")).unwrap();
    fs::create_dir_all(bridge_dir.join("responses")).unwrap();
    fs::write(
        bridge_dir.join("status.json"),
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "protocolVersion": 1,
            "active": true,
            "windowLabel": window_label,
            "registeredAtUnixNanos": now_unix_nanos(),
            "lastHeartbeatUnixNanos": last_heartbeat_unix_nanos
        }))
        .unwrap(),
    )
    .unwrap();
}

fn start_fake_drive_bridge_worker(
    db_path: &std::path::Path,
    payload: Value,
) -> thread::JoinHandle<Value> {
    start_fake_drive_bridge_sequence_worker(db_path, vec![payload])
        .map(|requests| requests.into_iter().next().unwrap())
}

trait JoinMap<T> {
    fn map<U: Send + 'static>(
        self,
        f: impl FnOnce(T) -> U + Send + 'static,
    ) -> thread::JoinHandle<U>;
}

impl<T: Send + 'static> JoinMap<T> for thread::JoinHandle<T> {
    fn map<U: Send + 'static>(
        self,
        f: impl FnOnce(T) -> U + Send + 'static,
    ) -> thread::JoinHandle<U> {
        thread::spawn(move || f(self.join().unwrap()))
    }
}

fn start_fake_drive_bridge_sequence_worker(
    db_path: &std::path::Path,
    payloads: Vec<Value>,
) -> thread::JoinHandle<Vec<Value>> {
    let bridge_dir = db_path.parent().unwrap().join("drive-bridge");
    thread::spawn(move || {
        let mut requests = Vec::new();
        for payload in payloads {
            let request_path = wait_for_bridge_request(&bridge_dir);
            let request: Value =
                serde_json::from_str(&fs::read_to_string(&request_path).unwrap()).unwrap();
            requests.push(request.clone());
            let request_id = request["requestId"].as_str().unwrap();
            fs::remove_file(&request_path).unwrap();
            let response_path = bridge_dir
                .join("responses")
                .join(format!("{request_id}.json"));
            fs::write(
                response_path,
                serde_json::to_vec_pretty(&json!({
                    "schemaVersion": 1,
                    "protocolVersion": 1,
                    "requestId": request_id,
                    "action": request["action"],
                    "selector": request["selector"],
                    "visibleOnly": request["visibleOnly"],
                    "ok": payload.get("ok").and_then(Value::as_bool).unwrap_or_else(|| {
                        payload
                            .get("exists")
                            .and_then(Value::as_bool)
                            .or_else(|| payload.get("found").and_then(Value::as_bool))
                            .unwrap_or(true)
                    }),
                    "payload": payload,
                    "completedAtUnixNanos": now_unix_nanos()
                }))
                .unwrap(),
            )
            .unwrap();
        }
        requests
    })
}

fn wait_for_bridge_request(bridge_dir: &std::path::Path) -> PathBuf {
    let request_dir = bridge_dir.join("requests");
    let in_flight_dir = bridge_dir.join("in-flight");
    for _ in 0..100 {
        if let Some(path) =
            first_json_in_dir(&request_dir).or_else(|| first_json_in_dir(&in_flight_dir))
        {
            return path;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("timed out waiting for bridge request");
}

fn first_json_in_dir(dir: &std::path::Path) -> Option<PathBuf> {
    let mut paths = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next()
}

fn now_unix_nanos() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(now.as_nanos()).unwrap()
}

fn run_json_command(command: &mut Command) -> Value {
    serde_json::from_str(&run_command(command)).unwrap()
}

fn run_command(command: &mut Command) -> String {
    let output = run_bounded_command(command);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap()
}

fn run_failure_command(command: &mut Command) -> String {
    let output = run_bounded_command(command);
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

fn run_bounded_command(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "command timed out after 10s\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
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
