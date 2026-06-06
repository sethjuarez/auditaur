use auditaur_collector::exporter_sqlite::{SqliteStore, SQLITE_SCHEMA_VERSION};
use auditaur_core::{
    discovery::DiscoveryFile,
    model::{
        FrontendError, LogRecord, Session, SpanRecord, TauriEventRecord, TauriIpcCall,
        TauriWindowState, TelemetrySource,
    },
};
use serde_json::{json, Value};
use std::{fs, process::Command};
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

    let explain = run_json(["explain", "--db", db.path().to_str().unwrap(), "--json"]);
    assert_eq!(explain["failedIpcCount"], 1);
    assert!(explain["findings"].as_array().unwrap().len() >= 1);

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
            capabilities: vec!["logs".to_string(), "traces".to_string()],
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
        })
        .unwrap(),
    )
    .unwrap();

    let apps = run_json_with_env(["apps", "--json"], temp.path().to_str().unwrap());
    assert_eq!(apps[0]["status"], "active");
    assert_eq!(apps[0]["schemaValid"], true);

    let logs = run_json_with_env(["logs", "--json"], temp.path().to_str().unwrap());
    assert_eq!(logs[0]["body"], "fixture log");
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

fn run_json_with_env<const N: usize>(args: [&str; N], data_dir: &str) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_auditaur"));
    command.args(args).env("AUDITAUR_DATA_DIR", data_dir);
    run_json_command(&mut command)
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
