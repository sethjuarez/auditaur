pub mod commands;
pub mod desktop;
pub mod error;
pub mod ipc;
pub mod state;
pub mod test_helpers;
pub mod tracing;

use auditaur_core::model::TauriWindowState;
pub use auditaur_core::AuditaurConfig;
pub use ipc::{
    ipc_traceparent, ipc_traceparent_from_request, ipc_traceparent_from_request_or_context,
    IpcTraceContext, IPC_CONTEXT_ARG, IPC_TRACEPARENT_HEADER,
};
use serde_json::{json, Map, Value};
use tauri::{
    plugin::{Builder as TauriPluginBuilder, TauriPlugin},
    Manager, Runtime, WebviewWindow, Window, WindowEvent,
};
pub use tauri_plugin_auditaur_macros::{auditaur_command, instrument_ipc};
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
            .on_window_ready(|window| {
                record_window_ready(&window);
                register_window_lifecycle(window);
            })
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

fn register_window_lifecycle<R: Runtime>(window: Window<R>) {
    let listener_window = window.clone();
    window.on_window_event(move |event| record_window_event(&listener_window, event));
}

fn record_window_ready<R: Runtime>(window: &Window<R>) {
    record_window_state(window, "window_ready", None);
}

fn record_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    record_window_state(window, "window_event", Some(event));
}

fn record_window_state<R: Runtime>(window: &Window<R>, capture: &str, event: Option<&WindowEvent>) {
    let Some(state) = window.try_state::<state::AuditaurState>() else {
        return;
    };
    let Some(session_id) = state.session_id.as_ref() else {
        return;
    };
    let Some(store) = state.store() else {
        return;
    };
    let Ok(store) = store.lock() else {
        return;
    };
    let size = window.inner_size().ok();
    let attributes = window_attributes(capture, event);
    let record = TauriWindowState {
        session_id: session_id.to_string(),
        timestamp_unix_nanos: now_unix_nanos(),
        window_label: window.label().to_string(),
        webview_label: None,
        url: None,
        title: window.title().ok(),
        focused: window.is_focused().ok(),
        visible: window.is_visible().ok(),
        width: size.as_ref().map(|size| f64::from(size.width)),
        height: size.as_ref().map(|size| f64::from(size.height)),
        scale_factor: window.scale_factor().ok(),
        attributes,
    };
    let _ = store.insert_tauri_window_state(&record);
}

fn window_attributes(capture: &str, event: Option<&WindowEvent>) -> Value {
    let mut attributes = Map::new();
    attributes.insert("auditaur.capture".to_string(), json!(capture));

    if let Some(event) = event {
        attributes.extend(window_event_attributes(event));
    }

    Value::Object(attributes)
}

fn window_event_attributes(event: &WindowEvent) -> Map<String, Value> {
    let mut attributes = Map::new();
    attributes.insert(
        "tauri.window.event".to_string(),
        json!(window_event_kind(event)),
    );
    attributes.insert(
        "tauri.window.event_debug".to_string(),
        json!(format!("{event:?}")),
    );

    match event {
        WindowEvent::Resized(size) => {
            attributes.insert("tauri.window.event.width".to_string(), json!(size.width));
            attributes.insert("tauri.window.event.height".to_string(), json!(size.height));
        }
        WindowEvent::Moved(position) => {
            attributes.insert("tauri.window.event.x".to_string(), json!(position.x));
            attributes.insert("tauri.window.event.y".to_string(), json!(position.y));
        }
        WindowEvent::Focused(focused) => {
            attributes.insert("tauri.window.event.focused".to_string(), json!(focused));
        }
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            attributes.insert(
                "tauri.window.event.scale_factor".to_string(),
                json!(scale_factor),
            );
        }
        WindowEvent::ThemeChanged(theme) => {
            attributes.insert(
                "tauri.window.event.theme".to_string(),
                json!(format!("{theme:?}")),
            );
        }
        _ => {}
    }

    attributes
}

fn window_event_kind(event: &WindowEvent) -> &'static str {
    match event {
        WindowEvent::Resized(_) => "resized",
        WindowEvent::Moved(_) => "moved",
        WindowEvent::CloseRequested { .. } => "close_requested",
        WindowEvent::Destroyed => "destroyed",
        WindowEvent::Focused(true) => "focused",
        WindowEvent::Focused(false) => "blurred",
        WindowEvent::ScaleFactorChanged { .. } => "scale_factor_changed",
        WindowEvent::DragDrop(_) => "drag_drop",
        WindowEvent::ThemeChanged(_) => "theme_changed",
        _ => "unknown",
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

#[cfg(test)]
mod window_tests {
    use super::{window_attributes, window_event_attributes};

    #[test]
    fn focused_window_events_record_authoritative_event_state() {
        let attributes = window_event_attributes(&tauri::WindowEvent::Focused(false));

        assert_eq!(attributes["tauri.window.event"], "blurred");
        assert_eq!(attributes["tauri.window.event.focused"], false);
    }

    #[test]
    fn resize_window_events_record_authoritative_event_size() {
        let attributes = window_event_attributes(&tauri::WindowEvent::Resized(
            tauri::PhysicalSize::new(800, 600),
        ));

        assert_eq!(attributes["tauri.window.event"], "resized");
        assert_eq!(attributes["tauri.window.event.width"], 800);
        assert_eq!(attributes["tauri.window.event.height"], 600);
    }

    #[test]
    fn moved_window_events_record_authoritative_event_position() {
        let attributes = window_event_attributes(&tauri::WindowEvent::Moved(
            tauri::PhysicalPosition::new(12, 34),
        ));

        assert_eq!(attributes["tauri.window.event"], "moved");
        assert_eq!(attributes["tauri.window.event.x"], 12);
        assert_eq!(attributes["tauri.window.event.y"], 34);
    }

    #[test]
    fn capture_only_window_attributes_do_not_claim_an_event() {
        let attributes = window_attributes("window_ready", None);

        assert_eq!(attributes["auditaur.capture"], "window_ready");
        assert!(attributes.get("tauri.window.event").is_none());
    }
}
