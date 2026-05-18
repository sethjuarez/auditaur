use crate::model::{
    FrontendError, LogRecord, Session, SpanRecord, TauriEventRecord, TauriIpcCall, TauriWindowState,
};
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
    fn insert_tauri_ipc_call(&self, call: &TauriIpcCall) -> Result<(), StorageError>;
    fn insert_tauri_event(&self, event: &TauriEventRecord) -> Result<(), StorageError>;
    fn insert_tauri_window_state(&self, window: &TauriWindowState) -> Result<(), StorageError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanQuery {
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontendErrorQuery {
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TauriIpcQuery {
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TauriEventQuery {
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TauriWindowQuery {
    pub session_id: Option<String>,
    pub latest_only: bool,
    pub limit: Option<usize>,
}
