use std::{
    fs, panic,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use auditaur_collector::{
    exporter_sqlite::{SqliteStore, SQLITE_SCHEMA_VERSION, TELEMETRY_DATABASE_FILE},
    receiver::OTelBatch,
    retention::RetentionLimits,
};
use auditaur_core::{
    discovery::DiscoveryFile,
    model::{FrontendError, LogRecord, Session, TelemetrySource},
    AuditaurConfig,
};
use serde_json::json;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::error::AuditaurError;

static PANIC_SINK: Mutex<Option<PanicSink>> = Mutex::new(None);
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct PanicSink {
    session_id: String,
    store: Arc<Mutex<SqliteStore>>,
}

pub struct AuditaurState {
    pub session_id: Option<String>,
    enabled: bool,
    store: Option<Arc<Mutex<SqliteStore>>>,
    discovery_path: Option<PathBuf>,
    heartbeat_alive: Option<Arc<AtomicBool>>,
    redact_defaults: bool,
    extra_redaction_keys: Vec<String>,
    retention_limits: RetentionLimits,
}

impl AuditaurState {
    pub fn initialize(
        config: AuditaurConfig,
        pid: u32,
        app_identifier: Option<String>,
    ) -> Result<Self, AuditaurError> {
        let enabled = config.enabled.unwrap_or_else(default_enabled);
        if !enabled {
            return Ok(Self {
                session_id: None,
                enabled: false,
                store: None,
                discovery_path: None,
                heartbeat_alive: None,
                redact_defaults: config.redact_defaults,
                extra_redaction_keys: config.extra_redaction_keys,
                retention_limits: RetentionLimits::default(),
            });
        }

        if !cfg!(debug_assertions) && !config.allow_release_builds {
            return Err(AuditaurError::new(
                "Auditaur is disabled in release builds unless allow_release_builds is true.",
            ));
        }

        let data_dir = auditaur_core::resolve_data_dir(config.data_dir.as_ref())
            .map_err(|error| AuditaurError::new(error.to_string()))?;
        let session_id = Uuid::new_v4().to_string();
        let instance_id = Uuid::new_v4().to_string();
        let session_dir = data_dir.join("sessions").join(&session_id);
        let apps_dir = data_dir.join("apps");
        fs::create_dir_all(&session_dir)?;
        fs::create_dir_all(&apps_dir)?;

        let database_path = session_dir.join(TELEMETRY_DATABASE_FILE);
        let store = SqliteStore::open(&database_path)?;
        store.migrate()?;

        let service_name = config
            .service_name
            .clone()
            .or_else(|| std::env::var("CARGO_PKG_NAME").ok())
            .unwrap_or_else(|| "tauri-app".to_string());
        let service_version = config.service_version.clone();
        let started_at = now_rfc3339()?;
        store.create_session(&Session {
            id: session_id.clone(),
            session_name: config.session_name.clone(),
            service_name: service_name.clone(),
            service_version: service_version.clone(),
            app_identifier: app_identifier.clone(),
            pid: Some(i64::from(pid)),
            started_at: started_at.clone(),
            ended_at: None,
            schema_version: SQLITE_SCHEMA_VERSION,
            auditaur_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        })?;

        let discovery_path = apps_dir.join(format!("{instance_id}.json"));
        let discovery = DiscoveryFile {
            schema_version: 1,
            instance_id,
            session_id: session_id.clone(),
            service_name,
            service_version,
            app_identifier,
            pid,
            started_at,
            database_path: database_path.to_string_lossy().to_string(),
            capabilities: vec![
                "logs".to_string(),
                "traces".to_string(),
                "frontend_errors".to_string(),
                "ipc".to_string(),
                "events".to_string(),
                "windows".to_string(),
            ],
            last_heartbeat_at: now_rfc3339()?,
        };
        write_discovery(&discovery_path, &discovery)?;

        let heartbeat_alive = Arc::new(AtomicBool::new(true));
        start_heartbeat(
            discovery_path.clone(),
            discovery,
            heartbeat_alive.clone(),
            Duration::from_millis(config.heartbeat_interval_ms.max(1_000)),
        );
        let store = Arc::new(Mutex::new(store));
        crate::tracing::install_sink(session_id.clone(), store.clone());
        install_panic_sink(session_id.clone(), store.clone());

        Ok(Self {
            session_id: Some(session_id),
            enabled: true,
            store: Some(store),
            discovery_path: Some(discovery_path),
            heartbeat_alive: Some(heartbeat_alive),
            redact_defaults: config.redact_defaults,
            extra_redaction_keys: config.extra_redaction_keys,
            retention_limits: RetentionLimits {
                max_session_bytes: config.max_session_bytes,
                ..RetentionLimits::default()
            },
        })
    }

    pub fn export_batch(&self, batch: OTelBatch) -> Result<(), AuditaurError> {
        if !self.enabled {
            return Ok(());
        }

        let Some(store) = &self.store else {
            return Err(AuditaurError::new(
                "Auditaur is enabled without an initialized store.",
            ));
        };
        let session_id = self
            .session_id
            .as_deref()
            .ok_or_else(|| AuditaurError::new("Auditaur is enabled without a session id."))?;
        let store = store
            .lock()
            .map_err(|_| AuditaurError::new("Auditaur SQLite store lock was poisoned."))?;

        for mut span in batch.spans {
            if span.session_id.is_empty() {
                span.session_id = session_id.to_string();
            }
            span.attributes = self.redact_value(&span.attributes);
            store.insert_span(&span)?;
        }

        for mut log in batch.logs {
            if log.session_id.is_empty() {
                log.session_id = session_id.to_string();
            }
            log.attributes = self.redact_value(&log.attributes);
            log.body_json = log.body_json.as_ref().map(|value| self.redact_value(value));
            store.insert_log(&log)?;
        }

        for mut error in batch.frontend_errors {
            if error.session_id.is_empty() {
                error.session_id = session_id.to_string();
            }
            error.attributes = self.redact_value(&error.attributes);
            store.insert_frontend_error(&error)?;
        }

        for mut call in batch.tauri_ipc_calls {
            if call.session_id.is_empty() {
                call.session_id = session_id.to_string();
            }
            if let Some(args_json) = &call.args_json {
                let outcome = auditaur_core::redaction::redact_json_with_options(
                    args_json,
                    self.redact_defaults,
                    &self.extra_redaction_keys,
                );
                call.args_json = Some(outcome.value);
                call.args_redacted = outcome.redacted;
            } else {
                call.args_redacted = false;
            }
            store.insert_tauri_ipc_call(&call)?;
        }

        for mut event in batch.tauri_events {
            if event.session_id.is_empty() {
                event.session_id = session_id.to_string();
            }
            if let Some(payload_json) = &event.payload_json {
                let outcome = auditaur_core::redaction::redact_json_with_options(
                    payload_json,
                    self.redact_defaults,
                    &self.extra_redaction_keys,
                );
                event.payload_json = Some(outcome.value);
                event.payload_redacted = outcome.redacted;
            } else {
                event.payload_redacted = false;
            }
            store.insert_tauri_event(&event)?;
        }

        store.enforce_retention(self.retention_limits)?;

        Ok(())
    }

    pub fn tracing_layer(&self) -> crate::tracing::AuditaurTracingLayer {
        match (&self.session_id, &self.store) {
            (Some(session_id), Some(store)) => {
                crate::tracing::AuditaurTracingLayer::with_sink(session_id.clone(), store.clone())
            }
            _ => crate::tracing::tracing_layer(),
        }
    }

    pub(crate) fn store(&self) -> Option<Arc<Mutex<SqliteStore>>> {
        self.store.clone()
    }

    fn redact_value(&self, value: &serde_json::Value) -> serde_json::Value {
        auditaur_core::redaction::redact_json_with_options(
            value,
            self.redact_defaults,
            &self.extra_redaction_keys,
        )
        .value
    }
}

impl Drop for AuditaurState {
    fn drop(&mut self) {
        if let Some(alive) = &self.heartbeat_alive {
            alive.store(false, Ordering::SeqCst);
        }
        if let Some(session_id) = &self.session_id {
            crate::tracing::clear_sink(session_id);
            clear_panic_sink(session_id);
        }
        if let Some(path) = &self.discovery_path {
            let _ = fs::remove_file(path);
        }
    }
}

fn default_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var("AUDITAUR").ok().as_deref() == Some("1")
}

fn now_rfc3339() -> Result<String, AuditaurError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AuditaurError::new(error.to_string()))
}

fn write_discovery(path: &PathBuf, discovery: &DiscoveryFile) -> Result<(), AuditaurError> {
    fs::write(path, serde_json::to_vec_pretty(discovery)?)?;
    Ok(())
}

fn start_heartbeat(
    discovery_path: PathBuf,
    mut discovery: DiscoveryFile,
    alive: Arc<AtomicBool>,
    interval: Duration,
) {
    thread::spawn(move || {
        while alive.load(Ordering::SeqCst) {
            thread::sleep(interval);
            if !alive.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(timestamp) = now_rfc3339() {
                discovery.last_heartbeat_at = timestamp;
                let _ = write_discovery(&discovery_path, &discovery);
            }
        }
    });
}

fn install_panic_sink(session_id: String, store: Arc<Mutex<SqliteStore>>) {
    if let Ok(mut sink) = PANIC_SINK.lock() {
        *sink = Some(PanicSink { session_id, store });
    }
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        record_panic(info);
        previous(info);
    }));
}

fn clear_panic_sink(session_id: &str) {
    let Ok(mut sink) = PANIC_SINK.lock() else {
        return;
    };
    if sink
        .as_ref()
        .map(|sink| sink.session_id.as_str() == session_id)
        .unwrap_or(false)
    {
        *sink = None;
    }
}

fn active_panic_sink() -> Option<PanicSink> {
    PANIC_SINK.lock().ok().and_then(|sink| sink.clone())
}

fn record_panic(info: &panic::PanicHookInfo<'_>) {
    let Some(sink) = active_panic_sink() else {
        return;
    };
    let Ok(store) = sink.store.try_lock() else {
        return;
    };
    let message = panic_message(info);
    let location = info.location().map(|location| {
        format!(
            "{}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        )
    });
    let timestamp = now_unix_nanos();
    let attributes = json!({
        "auditaur.source": "panic_hook",
        "exception.escaped": true,
        "code.filepath": info.location().map(|location| location.file()),
        "code.lineno": info.location().map(|location| location.line()),
        "code.column": info.location().map(|location| location.column()),
    });
    let _ = store.insert_log(&LogRecord {
        session_id: sink.session_id.clone(),
        timestamp_unix_nanos: timestamp,
        observed_timestamp_unix_nanos: None,
        severity_text: Some("ERROR".to_string()),
        severity_number: Some(17),
        body: Some(format!("Rust panic: {message}")),
        body_json: Some(json!({
            "message": message,
            "location": location,
        })),
        trace_id: None,
        span_id: None,
        scope_name: Some("panic".to_string()),
        scope_version: None,
        attributes: attributes.clone(),
        source: TelemetrySource::Plugin,
    });
    let _ = store.insert_frontend_error(&FrontendError {
        session_id: sink.session_id,
        timestamp_unix_nanos: timestamp,
        message,
        stack: location,
        filename: info.location().map(|location| location.file().to_string()),
        line_number: info.location().map(|location| i64::from(location.line())),
        column_number: info.location().map(|location| i64::from(location.column())),
        error_type: Some("RustPanic".to_string()),
        trace_id: None,
        span_id: None,
        window_label: None,
        attributes,
    });
}

fn panic_message(info: &panic::PanicHookInfo<'_>) -> String {
    info.payload()
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic payload was not a string".to_string())
}

fn now_unix_nanos() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_nanos()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::AuditaurState;
    use auditaur_collector::{exporter_sqlite::SqliteStore, receiver::OTelBatch};
    use auditaur_core::{
        model::{LogRecord, TauriEventRecord, TauriIpcCall},
        storage::FrontendErrorQuery,
        AuditaurConfig,
    };
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn initializes_session_database_and_discovery_file() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("plugin-test".to_string()),
                session_name: Some("plugin-session".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            Some("dev.auditaur.test".to_string()),
        )
        .unwrap();
        if let Some(session_id) = state.session_id.as_deref() {
            crate::tracing::clear_sink(session_id);
        }

        assert!(state.session_id.is_some());
        let db = temp
            .path()
            .join("sessions")
            .join(state.session_id.as_ref().unwrap())
            .join("telemetry.sqlite");
        let store = SqliteStore::open(db).unwrap();
        let session = store
            .get_session(state.session_id.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(session.session_name.as_deref(), Some("plugin-session"));
        assert_eq!(
            temp.path().join("apps").read_dir().unwrap().count(),
            1,
            "discovery file should be written"
        );
    }

    #[test]
    fn exports_redacted_batch_to_sqlite() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("plugin-test".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            None,
        )
        .unwrap();
        let session_id = state.session_id.clone().unwrap();
        crate::tracing::clear_sink(&session_id);

        state
            .export_batch(OTelBatch {
                logs: vec![LogRecord {
                    session_id: String::new(),
                    timestamp_unix_nanos: 1,
                    observed_timestamp_unix_nanos: None,
                    severity_text: Some("INFO".to_string()),
                    severity_number: Some(9),
                    body: Some("hello".to_string()),
                    body_json: Some(json!({ "token": "secret" })),
                    trace_id: None,
                    span_id: None,
                    scope_name: None,
                    scope_version: None,
                    attributes: json!({ "api_key": "secret" }),
                    source: auditaur_core::model::TelemetrySource::Frontend,
                }],
                tauri_ipc_calls: vec![TauriIpcCall {
                    session_id: String::new(),
                    timestamp_unix_nanos: 2,
                    duration_ms: Some(1.0),
                    command: "save".to_string(),
                    status: "OK".to_string(),
                    error_message: None,
                    trace_id: Some("trace".to_string()),
                    span_id: Some("span".to_string()),
                    window_label: Some("main".to_string()),
                    args_json: Some(json!({ "password": "secret" })),
                    args_redacted: true,
                    result_summary: Some("ok".to_string()),
                }],
                tauri_events: vec![TauriEventRecord {
                    session_id: String::new(),
                    timestamp_unix_nanos: 3,
                    event_name: "save".to_string(),
                    direction: "emit".to_string(),
                    target: None,
                    trace_id: Some("trace".to_string()),
                    span_id: Some("event-span".to_string()),
                    window_label: Some("main".to_string()),
                    payload_summary: Some("payload".to_string()),
                    payload_json: Some(json!({ "token": "secret" })),
                    payload_redacted: true,
                }],
                ..OTelBatch::default()
            })
            .unwrap();

        let db = temp
            .path()
            .join("sessions")
            .join(&session_id)
            .join("telemetry.sqlite");
        let store = SqliteStore::open(db).unwrap();
        let logs = store
            .list_logs(&auditaur_core::storage::LogQuery::default())
            .unwrap();

        let log = logs
            .iter()
            .find(|log| log.body.as_deref() == Some("hello"))
            .expect("exported frontend log should be present");
        assert_eq!(log.session_id, session_id);
        assert_eq!(log.attributes["api_key"], "[REDACTED]");
        assert_eq!(log.body_json.as_ref().unwrap()["token"], "[REDACTED]");

        let ipc = store
            .list_tauri_ipc_calls(&auditaur_core::storage::TauriIpcQuery::default())
            .unwrap();
        assert_eq!(ipc[0].session_id, session_id);
        assert_eq!(ipc[0].args_json.as_ref().unwrap()["password"], "[REDACTED]");

        let events = store
            .list_tauri_events(&auditaur_core::storage::TauriEventQuery::default())
            .unwrap();
        assert_eq!(
            events[0].payload_json.as_ref().unwrap()["token"],
            "[REDACTED]"
        );
    }

    #[test]
    fn panic_hook_records_and_clears_with_state() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("panic-test".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            None,
        )
        .unwrap();
        let session_id = state.session_id.clone().unwrap();
        crate::tracing::clear_sink(&session_id);

        let _ = std::panic::catch_unwind(|| panic!("intentional auditaur panic test"));

        let store = store_for(&temp, &session_id);
        let errors = store
            .list_frontend_errors(&FrontendErrorQuery::default())
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_type.as_deref(), Some("RustPanic"));
        assert_eq!(errors[0].message, "intentional auditaur panic test");

        drop(state);
        let _ = std::panic::catch_unwind(|| panic!("after drop"));
        let errors_after_drop = store
            .list_frontend_errors(&FrontendErrorQuery::default())
            .unwrap();
        assert_eq!(errors_after_drop.len(), 1);
    }

    fn store_for(temp: &TempDir, session_id: &str) -> SqliteStore {
        let db = temp
            .path()
            .join("sessions")
            .join(session_id)
            .join("telemetry.sqlite");
        SqliteStore::open(db).unwrap()
    }
}
