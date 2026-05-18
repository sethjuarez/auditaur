use crate::model::{FrontendError, LogRecord, Session, SpanRecord};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage backend error: {0}")]
    Backend(String),
}

pub trait TelemetryStore {
    fn create_session(&self, session: &Session) -> Result<(), StorageError>;
    fn insert_log(&self, log: &LogRecord) -> Result<(), StorageError>;
    fn insert_span(&self, span: &SpanRecord) -> Result<(), StorageError>;
    fn insert_frontend_error(&self, error: &FrontendError) -> Result<(), StorageError>;
}
