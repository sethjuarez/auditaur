use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Debug, Serialize)]
pub struct AuditaurError {
    message: String,
}

impl AuditaurError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for AuditaurError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AuditaurError {}

impl From<auditaur_collector::exporter_sqlite::SqliteStoreError> for AuditaurError {
    fn from(value: auditaur_collector::exporter_sqlite::SqliteStoreError) -> Self {
        Self::new(value.to_string())
    }
}

impl From<std::io::Error> for AuditaurError {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

impl From<serde_json::Error> for AuditaurError {
    fn from(value: serde_json::Error) -> Self {
        Self::new(value.to_string())
    }
}
