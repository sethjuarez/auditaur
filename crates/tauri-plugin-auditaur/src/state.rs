use std::{
    fs,
    io::ErrorKind,
    panic,
    path::{Path, PathBuf},
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
    drive_bridge::{
        DriveBridgeRequest, DriveBridgeResponse, DriveBridgeStatus, DRIVE_BRIDGE_CAPABILITY,
        DRIVE_BRIDGE_DIR, DRIVE_BRIDGE_IN_FLIGHT_DIR, DRIVE_BRIDGE_PROTOCOL_VERSION,
        DRIVE_BRIDGE_REQUESTS_DIR, DRIVE_BRIDGE_RESPONSES_DIR, DRIVE_BRIDGE_STALE_FILE_NANOS,
        DRIVE_BRIDGE_STATUS_FILE,
    },
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
    bridge_dir: Option<PathBuf>,
    heartbeat_alive: Option<Arc<AtomicBool>>,
    bridge_notifier_started: Arc<AtomicBool>,
    bridge_notifier_alive: Arc<AtomicBool>,
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
                bridge_dir: None,
                heartbeat_alive: None,
                bridge_notifier_started: Arc::new(AtomicBool::new(false)),
                bridge_notifier_alive: Arc::new(AtomicBool::new(false)),
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
        let bridge_dir = session_dir.join(DRIVE_BRIDGE_DIR);
        let apps_dir = data_dir.join("apps");
        fs::create_dir_all(&session_dir)?;
        fs::create_dir_all(bridge_dir.join(DRIVE_BRIDGE_REQUESTS_DIR))?;
        fs::create_dir_all(bridge_dir.join(DRIVE_BRIDGE_IN_FLIGHT_DIR))?;
        fs::create_dir_all(bridge_dir.join(DRIVE_BRIDGE_RESPONSES_DIR))?;
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
                DRIVE_BRIDGE_CAPABILITY.to_string(),
            ],
            last_heartbeat_at: now_rfc3339()?,
        };
        write_discovery(&discovery_path, &discovery)?;

        let heartbeat_alive = Arc::new(AtomicBool::new(true));
        let bridge_notifier_started = Arc::new(AtomicBool::new(false));
        let bridge_notifier_alive = Arc::new(AtomicBool::new(true));
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
            bridge_dir: Some(bridge_dir),
            heartbeat_alive: Some(heartbeat_alive),
            bridge_notifier_started,
            bridge_notifier_alive,
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

        for mut event in batch.span_events {
            if event.session_id.is_empty() {
                event.session_id = session_id.to_string();
            }
            event.attributes = self.redact_value(&event.attributes);
            store.insert_span_event(&event)?;
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

    pub fn ensure_drive_bridge_available(&self) -> Result<(), AuditaurError> {
        if !self.enabled {
            return Err(AuditaurError::new(
                "Auditaur is disabled; native drive screenshots require an enabled Auditaur session.",
            ));
        }
        self.session_id.as_ref().ok_or_else(|| {
            AuditaurError::new(
                "Auditaur is enabled without a session id; native drive screenshots are unavailable.",
            )
        })?;
        self.bridge_dir.as_ref().ok_or_else(|| {
            AuditaurError::new(
                "Auditaur is enabled without a drive bridge directory; native drive screenshots are unavailable.",
            )
        })?;
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

    pub(crate) fn bridge_dir_path(&self) -> Option<PathBuf> {
        self.bridge_dir.clone()
    }

    pub(crate) fn start_bridge_notifier_if_needed(&self) -> Option<(PathBuf, Arc<AtomicBool>)> {
        let bridge_dir = self.bridge_dir_path()?;
        if self.bridge_notifier_started.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some((bridge_dir, self.bridge_notifier_alive.clone()))
    }

    pub fn register_drive_bridge(
        &self,
        window_label: Option<String>,
    ) -> Result<DriveBridgeStatus, AuditaurError> {
        let bridge_dir = self.bridge_dir()?;
        ensure_bridge_dirs(&bridge_dir)?;
        sweep_stale_bridge_files(&bridge_dir)?;
        let now = now_unix_nanos();
        let registered_at_unix_nanos = read_drive_bridge_status(&bridge_dir)
            .ok()
            .flatten()
            .map(|status| status.registered_at_unix_nanos)
            .unwrap_or(now);
        let status = DriveBridgeStatus {
            schema_version: 1,
            protocol_version: DRIVE_BRIDGE_PROTOCOL_VERSION,
            active: true,
            window_label: window_label.clone(),
            registered_at_unix_nanos,
            last_heartbeat_unix_nanos: now,
            targets: vec![auditaur_core::drive_bridge::DriveBridgeTarget {
                target_id: "auditaur-bridge".to_string(),
                title: "Auditaur in-app drive bridge".to_string(),
                window_label,
                active: true,
                last_heartbeat_unix_nanos: now,
            }],
        };
        write_drive_bridge_status(&bridge_dir, &status)?;
        Ok(status)
    }

    pub fn poll_drive_bridge_request(
        &self,
        window_label: Option<String>,
    ) -> Result<Option<DriveBridgeRequest>, AuditaurError> {
        let bridge_dir = self.bridge_dir()?;
        ensure_bridge_dirs(&bridge_dir)?;
        sweep_stale_bridge_files(&bridge_dir)?;
        let requests_dir = bridge_dir.join(DRIVE_BRIDGE_REQUESTS_DIR);
        let in_flight_dir = bridge_dir.join(DRIVE_BRIDGE_IN_FLIGHT_DIR);
        tracing::debug!(
            window_label = window_label.as_deref(),
            requests_dir = %requests_dir.display(),
            "Auditaur drive bridge poll started"
        );
        let Some(request_path) =
            first_matching_request_file(&requests_dir, window_label.as_deref())?
        else {
            return Ok(None);
        };
        tracing::debug!(
            window_label = window_label.as_deref(),
            request_path = %request_path.display(),
            "Auditaur drive bridge poll matched request"
        );
        let file_name = request_path
            .file_name()
            .ok_or_else(|| AuditaurError::new("drive bridge request path had no file name"))?;
        let in_flight_path = in_flight_dir.join(file_name);
        match fs::rename(&request_path, &in_flight_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                tracing::debug!(
                    request_path = %request_path.display(),
                    "Auditaur drive bridge request was already claimed"
                );
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        }
        let request = fs::read(&in_flight_path)?;
        let request: DriveBridgeRequest = serde_json::from_slice(&request)?;
        tracing::debug!(
            action = request.action.as_str(),
            request_id = request.request_id.as_str(),
            selector = request.selector.as_deref(),
            "Auditaur drive bridge poll returning request"
        );
        Ok(Some(request))
    }

    pub fn complete_drive_bridge_request(
        &self,
        response: DriveBridgeResponse,
    ) -> Result<(), AuditaurError> {
        let bridge_dir = self.bridge_dir()?;
        ensure_bridge_dirs(&bridge_dir)?;
        let response_path = bridge_dir
            .join(DRIVE_BRIDGE_RESPONSES_DIR)
            .join(format!("{}.json", safe_bridge_id(&response.request_id)?));
        atomic_write_json(&response_path, &response)?;
        let in_flight_path = bridge_dir
            .join(DRIVE_BRIDGE_IN_FLIGHT_DIR)
            .join(format!("{}.json", safe_bridge_id(&response.request_id)?));
        if in_flight_path.exists() {
            fs::remove_file(in_flight_path)?;
        }
        Ok(())
    }

    fn redact_value(&self, value: &serde_json::Value) -> serde_json::Value {
        auditaur_core::redaction::redact_json_with_options(
            value,
            self.redact_defaults,
            &self.extra_redaction_keys,
        )
        .value
    }

    fn bridge_dir(&self) -> Result<PathBuf, AuditaurError> {
        if !self.enabled {
            return Err(AuditaurError::new("Auditaur drive bridge is disabled."));
        }
        self.bridge_dir
            .clone()
            .ok_or_else(|| AuditaurError::new("Auditaur drive bridge has no session directory."))
    }
}

impl Drop for AuditaurState {
    fn drop(&mut self) {
        if let Some(alive) = &self.heartbeat_alive {
            alive.store(false, Ordering::SeqCst);
        }
        self.bridge_notifier_alive.store(false, Ordering::SeqCst);
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
    atomic_write_json(path, discovery)?;
    Ok(())
}

fn ensure_bridge_dirs(bridge_dir: &Path) -> Result<(), AuditaurError> {
    fs::create_dir_all(bridge_dir.join(DRIVE_BRIDGE_REQUESTS_DIR))?;
    fs::create_dir_all(bridge_dir.join(DRIVE_BRIDGE_IN_FLIGHT_DIR))?;
    fs::create_dir_all(bridge_dir.join(DRIVE_BRIDGE_RESPONSES_DIR))?;
    Ok(())
}

fn first_matching_request_file(
    dir: &Path,
    window_label: Option<&str>,
) -> Result<Option<PathBuf>, AuditaurError> {
    let mut paths = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let Ok(request) = serde_json::from_slice::<DriveBridgeRequest>(&bytes) else {
            continue;
        };
        if request
            .window_label
            .as_deref()
            .is_none_or(|requested| Some(requested) == window_label)
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn sweep_stale_bridge_files(bridge_dir: &Path) -> Result<(), AuditaurError> {
    let now = now_unix_nanos();
    for dirname in [
        DRIVE_BRIDGE_REQUESTS_DIR,
        DRIVE_BRIDGE_IN_FLIGHT_DIR,
        DRIVE_BRIDGE_RESPONSES_DIR,
    ] {
        let dir = bridge_dir.join(dirname);
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
            let path = entry.path();
            if !is_bridge_json_or_temp_file(&path) {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
                .unwrap_or(now);
            if now.saturating_sub(modified) > DRIVE_BRIDGE_STALE_FILE_NANOS {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

fn is_bridge_json_or_temp_file(path: &Path) -> bool {
    if path.extension().and_then(|value| value.to_str()) == Some("json") {
        return true;
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.contains(".json.") && name.ends_with(".tmp"))
}

fn safe_bridge_id(id: &str) -> Result<&str, AuditaurError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(AuditaurError::new(
            "drive bridge request id contains invalid characters.",
        ));
    }
    Ok(id)
}

fn read_drive_bridge_status(bridge_dir: &Path) -> Result<Option<DriveBridgeStatus>, AuditaurError> {
    let status_path = bridge_dir.join(DRIVE_BRIDGE_STATUS_FILE);
    if !status_path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(status_path)?;
    serde_json::from_slice(&bytes).map(Some).map_err(Into::into)
}

fn write_drive_bridge_status(
    bridge_dir: &Path,
    status: &DriveBridgeStatus,
) -> Result<(), AuditaurError> {
    atomic_write_json(&bridge_dir.join(DRIVE_BRIDGE_STATUS_FILE), status)?;
    Ok(())
}

fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), AuditaurError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write(path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AuditaurError> {
    let parent = path
        .parent()
        .ok_or_else(|| AuditaurError::new("atomic write target has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AuditaurError::new("atomic write target has no UTF-8 file name"))?;
    let temp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        now_unix_nanos().saturating_abs()
    ));
    fs::write(&temp_path, bytes)?;
    fs::rename(temp_path, path)?;
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
        drive_bridge::{
            DriveBridgeRequest, DriveBridgeResponse, DRIVE_BRIDGE_IN_FLIGHT_DIR,
            DRIVE_BRIDGE_PROTOCOL_VERSION, DRIVE_BRIDGE_REQUESTS_DIR, DRIVE_BRIDGE_RESPONSES_DIR,
            DRIVE_BRIDGE_STALE_FILE_NANOS,
        },
        model::{LogRecord, SpanEventRecord, SpanRecord, TauriEventRecord, TauriIpcCall},
        storage::{FrontendErrorQuery, SpanEventQuery},
        AuditaurConfig,
    };
    use serde_json::json;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, SystemTime};
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
                spans: vec![SpanRecord {
                    session_id: String::new(),
                    trace_id: "trace".to_string(),
                    span_id: "span".to_string(),
                    parent_span_id: None,
                    name: "agentive.run".to_string(),
                    kind: Some("internal".to_string()),
                    start_time_unix_nanos: 1,
                    end_time_unix_nanos: Some(4),
                    status_code: Some("OK".to_string()),
                    status_message: None,
                    scope_name: Some("agentive".to_string()),
                    scope_version: Some("0.2.1".to_string()),
                    attributes: json!({ "agentive.run_id": "run", "token": "secret" }),
                    source: auditaur_core::model::TelemetrySource::ThirdPartyOtel,
                }],
                span_events: vec![SpanEventRecord {
                    session_id: String::new(),
                    trace_id: "trace".to_string(),
                    span_id: "span".to_string(),
                    name: "agent-event".to_string(),
                    timestamp_unix_nanos: 2,
                    attributes: json!({ "token": "secret", "summary": "done" }),
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

        let span_events = store.list_span_events(&SpanEventQuery::default()).unwrap();
        assert_eq!(span_events[0].session_id, session_id);
        assert_eq!(span_events[0].attributes["token"], "[REDACTED]");

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
    fn drive_bridge_register_poll_and_complete_round_trip() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("bridge-test".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            None,
        )
        .unwrap();
        let session_id = state.session_id.clone().unwrap();
        crate::tracing::clear_sink(&session_id);

        let status = state
            .register_drive_bridge(Some("main".to_string()))
            .unwrap();
        assert!(status.active);
        assert_eq!(status.window_label.as_deref(), Some("main"));

        let bridge_dir = state.bridge_dir().unwrap();
        let request = DriveBridgeRequest {
            schema_version: 1,
            protocol_version: DRIVE_BRIDGE_PROTOCOL_VERSION,
            request_id: "request-1".to_string(),
            action: "exists".to_string(),
            selector: Some("#ready".to_string()),
            value: None,
            values: Vec::new(),
            visible_only: true,
            window_label: Some("main".to_string()),
            test_id: Some("test".to_string()),
            step_id: Some("step".to_string()),
            created_at_unix_nanos: 1,
        };
        std::fs::write(
            bridge_dir
                .join(DRIVE_BRIDGE_REQUESTS_DIR)
                .join("request-1.json"),
            serde_json::to_vec_pretty(&request).unwrap(),
        )
        .unwrap();

        let polled = state
            .poll_drive_bridge_request(Some("main".to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(polled.request_id, "request-1");
        assert!(!bridge_dir
            .join(DRIVE_BRIDGE_REQUESTS_DIR)
            .join("request-1.json")
            .exists());
        assert!(bridge_dir
            .join(DRIVE_BRIDGE_IN_FLIGHT_DIR)
            .join("request-1.json")
            .exists());

        state
            .complete_drive_bridge_request(DriveBridgeResponse {
                schema_version: 1,
                protocol_version: DRIVE_BRIDGE_PROTOCOL_VERSION,
                request_id: "request-1".to_string(),
                action: "exists".to_string(),
                selector: Some("#ready".to_string()),
                visible_only: true,
                ok: true,
                payload: json!({ "exists": true }),
                error: None,
                completed_at_unix_nanos: 2,
            })
            .unwrap();

        assert!(!bridge_dir
            .join(DRIVE_BRIDGE_IN_FLIGHT_DIR)
            .join("request-1.json")
            .exists());
        let response: DriveBridgeResponse = serde_json::from_slice(
            &std::fs::read(
                bridge_dir
                    .join(DRIVE_BRIDGE_RESPONSES_DIR)
                    .join("request-1.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(response.payload["exists"], true);
    }

    #[test]
    fn drive_bridge_rejects_unsafe_response_request_id() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("bridge-test".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            None,
        )
        .unwrap();
        let session_id = state.session_id.clone().unwrap();
        crate::tracing::clear_sink(&session_id);

        let error = state
            .complete_drive_bridge_request(DriveBridgeResponse {
                schema_version: 1,
                protocol_version: DRIVE_BRIDGE_PROTOCOL_VERSION,
                request_id: "../escape".to_string(),
                action: "exists".to_string(),
                selector: None,
                visible_only: false,
                ok: false,
                payload: json!({ "ok": false }),
                error: Some("bad".to_string()),
                completed_at_unix_nanos: 2,
            })
            .unwrap_err();
        assert!(error.to_string().contains("invalid characters"));
    }

    #[test]
    fn drive_bridge_notifier_starts_once_and_stops_with_state() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("bridge-test".to_string()),
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
            .register_drive_bridge(Some("main".to_string()))
            .unwrap();
        let first = state.start_bridge_notifier_if_needed();
        assert!(first.is_some(), "first bridge registration starts notifier");
        let (_, alive) = first.unwrap();
        assert!(alive.load(Ordering::SeqCst));
        assert!(
            state.start_bridge_notifier_if_needed().is_none(),
            "notifier startup is idempotent"
        );
        drop(state);
        assert!(
            !alive.load(Ordering::SeqCst),
            "notifier lifetime flag stops with plugin state"
        );
    }

    #[test]
    fn drive_bridge_sweeps_stale_atomic_temp_files() {
        let _guard = crate::test_support::global_state_lock();
        let temp = TempDir::new().unwrap();
        let state = AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("bridge-test".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            None,
        )
        .unwrap();
        let session_id = state.session_id.clone().unwrap();
        crate::tracing::clear_sink(&session_id);

        let bridge_dir = state.bridge_dir().unwrap();
        let stale_temp = bridge_dir
            .join(DRIVE_BRIDGE_REQUESTS_DIR)
            .join("request-1.json.123.tmp");
        std::fs::write(&stale_temp, b"partial").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&stale_temp)
            .unwrap()
            .set_modified(
                SystemTime::now()
                    - Duration::from_nanos(
                        u64::try_from(DRIVE_BRIDGE_STALE_FILE_NANOS).unwrap() + 1_000_000,
                    ),
            )
            .unwrap();

        state
            .register_drive_bridge(Some("main".to_string()))
            .unwrap();
        assert!(
            !stale_temp.exists(),
            "stale atomic-write temp files should be reclaimed"
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
