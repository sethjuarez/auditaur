pub mod commands;
pub mod desktop;
pub mod error;
pub mod state;
pub mod tracing;

use auditaur_core::model::TauriWindowState;
pub use auditaur_core::AuditaurConfig;
use serde_json::json;
use tauri::{
    plugin::{Builder as TauriPluginBuilder, TauriPlugin},
    Manager, Runtime, WebviewWindow,
};
pub use tracing::tracing_layer;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static GLOBAL_STATE_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn global_state_lock() -> MutexGuard<'static, ()> {
        GLOBAL_STATE_LOCK.lock().unwrap()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Builder {
    config: AuditaurConfig,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn service_name(mut self, service_name: impl Into<String>) -> Self {
        self.config.service_name = Some(service_name.into());
        self
    }

    pub fn session_name(mut self, session_name: impl Into<String>) -> Self {
        self.config.session_name = Some(session_name.into());
        self
    }

    pub fn redact_defaults(mut self, redact_defaults: bool) -> Self {
        self.config.redact_defaults = redact_defaults;
        self
    }

    pub fn max_session_bytes(mut self, max_session_bytes: u64) -> Self {
        self.config.max_session_bytes = max_session_bytes;
        self
    }

    pub fn allow_release_builds(mut self, allow_release_builds: bool) -> Self {
        self.config.allow_release_builds = allow_release_builds;
        self
    }

    pub fn build<R: Runtime>(self) -> TauriPlugin<R> {
        let config = self.config;
        TauriPluginBuilder::new("auditaur")
            .invoke_handler(tauri::generate_handler![commands::export_otel_batch])
            .setup(move |app, _api| {
                let app_identifier = Some(app.config().identifier.clone());
                let state = state::AuditaurState::initialize(
                    config.clone(),
                    std::process::id(),
                    app_identifier,
                )?;
                capture_initial_windows(app, &state);
                app.manage(state);
                Ok(())
            })
            .build()
    }
}

fn capture_initial_windows<R: Runtime>(app: &tauri::AppHandle<R>, state: &state::AuditaurState) {
    let Some(session_id) = state.session_id.as_ref() else {
        return;
    };
    let Some(store) = state.store() else {
        return;
    };
    let Ok(store) = store.lock() else {
        return;
    };
    for window in app.webview_windows().values() {
        let record = window_state(session_id, window);
        let _ = store.insert_tauri_window_state(&record);
    }
}

fn window_state<R: Runtime>(session_id: &str, window: &WebviewWindow<R>) -> TauriWindowState {
    let size = window.inner_size().ok();
    TauriWindowState {
        session_id: session_id.to_string(),
        timestamp_unix_nanos: now_unix_nanos(),
        window_label: window.label().to_string(),
        webview_label: Some(window.label().to_string()),
        url: None,
        title: window.title().ok(),
        focused: window.is_focused().ok(),
        visible: window.is_visible().ok(),
        width: size.as_ref().map(|size| f64::from(size.width)),
        height: size.as_ref().map(|size| f64::from(size.height)),
        scale_factor: window.scale_factor().ok(),
        attributes: json!({ "auditaur.capture": "initial_window_state" }),
    }
}

fn now_unix_nanos() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_nanos()).unwrap_or(i64::MAX)
}
