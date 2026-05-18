pub mod commands;
pub mod desktop;
pub mod error;
pub mod state;

pub use auditaur_core::AuditaurConfig;
use tauri::{
    plugin::{Builder as TauriPluginBuilder, TauriPlugin},
    Manager, Runtime,
};

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
                app.manage(state);
                Ok(())
            })
            .build()
    }
}
