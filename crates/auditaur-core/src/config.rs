use std::path::PathBuf;

use directories::BaseDirs;
use thiserror::Error;

pub const AUDITAUR_DATA_DIR_ENV: &str = "AUDITAUR_DATA_DIR";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not resolve local data directory")]
    LocalDataDirUnavailable,
}

#[derive(Debug, Clone)]
pub struct AuditaurConfig {
    pub enabled: Option<bool>,
    pub service_name: Option<String>,
    pub service_version: Option<String>,
    pub session_name: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub redact_defaults: bool,
    pub extra_redaction_keys: Vec<String>,
    pub capture_full_payloads: bool,
    pub max_payload_bytes: usize,
    pub max_session_bytes: u64,
    pub heartbeat_interval_ms: u64,
    pub allow_release_builds: bool,
}

impl Default for AuditaurConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            service_name: None,
            service_version: None,
            session_name: None,
            data_dir: None,
            redact_defaults: true,
            extra_redaction_keys: Vec::new(),
            capture_full_payloads: false,
            max_payload_bytes: 16 * 1024,
            max_session_bytes: 256 * 1024 * 1024,
            heartbeat_interval_ms: 5_000,
            allow_release_builds: false,
        }
    }
}

pub fn resolve_data_dir(configured: Option<&PathBuf>) -> Result<PathBuf, ConfigError> {
    if let Some(data_dir) = configured {
        return Ok(data_dir.clone());
    }
    if let Ok(data_dir) = std::env::var(AUDITAUR_DATA_DIR_ENV) {
        return Ok(PathBuf::from(data_dir));
    }
    let base_dirs = BaseDirs::new().ok_or(ConfigError::LocalDataDirUnavailable)?;
    Ok(base_dirs.data_local_dir().join("auditaur"))
}
