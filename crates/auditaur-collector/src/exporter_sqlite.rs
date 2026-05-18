use auditaur_core::{
    model::{FrontendError, LogRecord, Session, SpanRecord, TelemetrySource},
    protocol::TraceSummary,
    storage::{FrontendErrorQuery, LogQuery, SpanQuery, TelemetryStore},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

pub const TELEMETRY_DATABASE_FILE: &str = "telemetry.sqlite";
pub const SQLITE_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum SqliteStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported schema version {found}; expected {expected}")]
    UnsupportedSchemaVersion { found: i64, expected: i64 },
    #[error("missing schema migration or table for schema version {0}")]
    MissingMigration(i64),
}

pub type Result<T> = std::result::Result<T, SqliteStoreError>;

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        configure_connection(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        configure_connection(&conn)?;
        Ok(Self { conn })
    }

    pub fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;
            {MIGRATION_1}
            INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES ({SQLITE_SCHEMA_VERSION}, datetime('now'));
            COMMIT;"
        ))?;
        Ok(())
    }

    pub fn validate_schema(&self) -> Result<()> {
        let version = self
            .conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get::<_, Option<i64>>(0)
            })
            .optional()?
            .flatten()
            .ok_or(SqliteStoreError::MissingMigration(SQLITE_SCHEMA_VERSION))?;

        if version != SQLITE_SCHEMA_VERSION {
            return Err(SqliteStoreError::UnsupportedSchemaVersion {
                found: version,
                expected: SQLITE_SCHEMA_VERSION,
            });
        }

        for table in REQUIRED_TABLES {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                params![table],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(SqliteStoreError::MissingMigration(SQLITE_SCHEMA_VERSION));
            }
        }

        for (table, column) in REQUIRED_COLUMNS {
            if !column_exists(&self.conn, table, column)? {
                return Err(SqliteStoreError::MissingMigration(SQLITE_SCHEMA_VERSION));
            }
        }

        Ok(())
    }

    pub fn create_session(&self, session: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (
                id, service_name, service_version, app_identifier, pid, started_at, ended_at, schema_version, auditaur_version
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                service_name = excluded.service_name,
                service_version = excluded.service_version,
                app_identifier = excluded.app_identifier,
                pid = excluded.pid,
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                schema_version = excluded.schema_version,
                auditaur_version = excluded.auditaur_version",
            params![
                session.id,
                session.service_name,
                session.service_version,
                session.app_identifier,
                session.pid,
                session.started_at,
                session.ended_at,
                session.schema_version,
                session.auditaur_version,
            ],
        )?;
        Ok(())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        self.conn
            .query_row(
                "SELECT id, service_name, service_version, app_identifier, pid, started_at,
                    ended_at, schema_version, auditaur_version
                 FROM sessions
                 WHERE id = ?1",
                params![session_id],
                map_session,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_sessions(&self, limit: Option<usize>) -> Result<Vec<Session>> {
        let limit = bounded_limit(limit, 20);
        let mut stmt = self.conn.prepare(
            "SELECT id, service_name, service_version, app_identifier, pid, started_at,
                ended_at, schema_version, auditaur_version
             FROM sessions
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;
        let sessions = collect_rows(stmt.query_map(params![limit], map_session)?);
        sessions
    }

    pub fn insert_log(&self, log: &LogRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO logs (
                session_id, timestamp_unix_nanos, observed_timestamp_unix_nanos, severity_text,
                severity_number, body, body_json, trace_id, span_id, scope_name, scope_version,
                attributes_json, source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                log.session_id,
                log.timestamp_unix_nanos,
                log.observed_timestamp_unix_nanos,
                log.severity_text,
                log.severity_number,
                log.body,
                optional_json(&log.body_json)?,
                log.trace_id,
                log.span_id,
                log.scope_name,
                log.scope_version,
                serde_json::to_string(&log.attributes)?,
                log.source.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_span(&self, span: &SpanRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO spans (
                session_id, trace_id, span_id, parent_span_id, name, kind, start_time_unix_nanos,
                end_time_unix_nanos, status_code, status_message, scope_name, scope_version,
                attributes_json, source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                span.session_id,
                span.trace_id,
                span.span_id,
                span.parent_span_id,
                span.name,
                span.kind,
                span.start_time_unix_nanos,
                span.end_time_unix_nanos,
                span.status_code,
                span.status_message,
                span.scope_name,
                span.scope_version,
                serde_json::to_string(&span.attributes)?,
                span.source.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_frontend_error(&self, error: &FrontendError) -> Result<()> {
        self.conn.execute(
            "INSERT INTO frontend_errors (
                session_id, timestamp_unix_nanos, message, stack, filename, line_number,
                column_number, error_type, trace_id, span_id, window_label, attributes_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                error.session_id,
                error.timestamp_unix_nanos,
                error.message,
                error.stack,
                error.filename,
                error.line_number,
                error.column_number,
                error.error_type,
                error.trace_id,
                error.span_id,
                error.window_label,
                serde_json::to_string(&error.attributes)?,
            ],
        )?;
        Ok(())
    }

    pub fn list_logs(&self, query: &LogQuery) -> Result<Vec<LogRecord>> {
        let limit = bounded_limit(query.limit, 200);
        match (&query.session_id, &query.trace_id) {
            (Some(session_id), Some(trace_id)) => {
                let mut stmt = self.conn.prepare(LOGS_SELECT_WITH_SESSION_AND_TRACE)?;
                let logs =
                    collect_rows(stmt.query_map(params![session_id, trace_id, limit], map_log)?);
                logs
            }
            (Some(session_id), None) => {
                let mut stmt = self.conn.prepare(LOGS_SELECT_WITH_SESSION)?;
                let logs = collect_rows(stmt.query_map(params![session_id, limit], map_log)?);
                logs
            }
            (None, Some(trace_id)) => {
                let mut stmt = self.conn.prepare(LOGS_SELECT_WITH_TRACE)?;
                let logs = collect_rows(stmt.query_map(params![trace_id, limit], map_log)?);
                logs
            }
            (None, None) => {
                let mut stmt = self.conn.prepare(LOGS_SELECT)?;
                let logs = collect_rows(stmt.query_map(params![limit], map_log)?);
                logs
            }
        }
    }

    pub fn list_spans(&self, query: &SpanQuery) -> Result<Vec<SpanRecord>> {
        let limit = bounded_limit(query.limit, 200);
        match (&query.session_id, &query.trace_id) {
            (Some(session_id), Some(trace_id)) => {
                let mut stmt = self.conn.prepare(SPANS_SELECT_WITH_SESSION_AND_TRACE)?;
                let spans =
                    collect_rows(stmt.query_map(params![session_id, trace_id, limit], map_span)?);
                spans
            }
            (Some(session_id), None) => {
                let mut stmt = self.conn.prepare(SPANS_SELECT_WITH_SESSION)?;
                let spans = collect_rows(stmt.query_map(params![session_id, limit], map_span)?);
                spans
            }
            (None, Some(trace_id)) => {
                let mut stmt = self.conn.prepare(SPANS_SELECT_WITH_TRACE)?;
                let spans = collect_rows(stmt.query_map(params![trace_id, limit], map_span)?);
                spans
            }
            (None, None) => {
                let mut stmt = self.conn.prepare(SPANS_SELECT)?;
                let spans = collect_rows(stmt.query_map(params![limit], map_span)?);
                spans
            }
        }
    }

    pub fn list_trace_summaries(
        &self,
        session_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<TraceSummary>> {
        let limit = bounded_limit(limit, 100);
        if let Some(session_id) = session_id {
            let mut stmt = self.conn.prepare(TRACE_SUMMARIES_WITH_SESSION)?;
            let summaries =
                collect_rows(stmt.query_map(params![session_id, limit], map_trace_summary)?);
            summaries
        } else {
            let mut stmt = self.conn.prepare(TRACE_SUMMARIES)?;
            let summaries = collect_rows(stmt.query_map(params![limit], map_trace_summary)?);
            summaries
        }
    }

    pub fn list_frontend_errors(&self, query: &FrontendErrorQuery) -> Result<Vec<FrontendError>> {
        let limit = bounded_limit(query.limit, 200);
        match (&query.session_id, &query.trace_id) {
            (Some(session_id), Some(trace_id)) => {
                let mut stmt = self
                    .conn
                    .prepare(FRONTEND_ERRORS_SELECT_WITH_SESSION_AND_TRACE)?;
                let errors = collect_rows(
                    stmt.query_map(params![session_id, trace_id, limit], map_frontend_error)?,
                );
                errors
            }
            (Some(session_id), None) => {
                let mut stmt = self.conn.prepare(FRONTEND_ERRORS_SELECT_WITH_SESSION)?;
                let errors =
                    collect_rows(stmt.query_map(params![session_id, limit], map_frontend_error)?);
                errors
            }
            (None, Some(trace_id)) => {
                let mut stmt = self.conn.prepare(FRONTEND_ERRORS_SELECT_WITH_TRACE)?;
                let errors =
                    collect_rows(stmt.query_map(params![trace_id, limit], map_frontend_error)?);
                errors
            }
            (None, None) => {
                let mut stmt = self.conn.prepare(FRONTEND_ERRORS_SELECT)?;
                let errors = collect_rows(stmt.query_map(params![limit], map_frontend_error)?);
                errors
            }
        }
    }
}

impl TelemetryStore for SqliteStore {
    fn create_session(
        &self,
        session: &Session,
    ) -> std::result::Result<(), auditaur_core::storage::StorageError> {
        SqliteStore::create_session(self, session)
            .map_err(|error| auditaur_core::storage::StorageError::Backend(error.to_string()))
    }

    fn insert_log(
        &self,
        log: &LogRecord,
    ) -> std::result::Result<(), auditaur_core::storage::StorageError> {
        SqliteStore::insert_log(self, log)
            .map_err(|error| auditaur_core::storage::StorageError::Backend(error.to_string()))
    }

    fn insert_span(
        &self,
        span: &SpanRecord,
    ) -> std::result::Result<(), auditaur_core::storage::StorageError> {
        SqliteStore::insert_span(self, span)
            .map_err(|error| auditaur_core::storage::StorageError::Backend(error.to_string()))
    }

    fn insert_frontend_error(
        &self,
        error: &FrontendError,
    ) -> std::result::Result<(), auditaur_core::storage::StorageError> {
        SqliteStore::insert_frontend_error(self, error)
            .map_err(|error| auditaur_core::storage::StorageError::Backend(error.to_string()))
    }
}

fn optional_json(value: &Option<Value>) -> Result<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_json(value: String) -> rusqlite::Result<Value> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn parse_optional_json(value: Option<String>) -> rusqlite::Result<Option<Value>> {
    value.map(parse_json).transpose()
}

fn bounded_limit(limit: Option<usize>, default_limit: i64) -> i64 {
    limit
        .map(|limit| i64::try_from(limit).unwrap_or(i64::MAX))
        .unwrap_or(default_limit)
}

fn collect_rows<T, M>(rows: rusqlite::MappedRows<'_, M>) -> Result<Vec<T>>
where
    M: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn map_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<LogRecord> {
    Ok(LogRecord {
        session_id: row.get(0)?,
        timestamp_unix_nanos: row.get(1)?,
        observed_timestamp_unix_nanos: row.get(2)?,
        severity_text: row.get(3)?,
        severity_number: row.get(4)?,
        body: row.get(5)?,
        body_json: parse_optional_json(row.get(6)?)?,
        trace_id: row.get(7)?,
        span_id: row.get(8)?,
        scope_name: row.get(9)?,
        scope_version: row.get(10)?,
        attributes: parse_json(row.get(11)?)?,
        source: TelemetrySource::from_storage(row.get::<_, String>(12)?.as_str()),
    })
}

fn map_span(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpanRecord> {
    Ok(SpanRecord {
        session_id: row.get(0)?,
        trace_id: row.get(1)?,
        span_id: row.get(2)?,
        parent_span_id: row.get(3)?,
        name: row.get(4)?,
        kind: row.get(5)?,
        start_time_unix_nanos: row.get(6)?,
        end_time_unix_nanos: row.get(7)?,
        status_code: row.get(8)?,
        status_message: row.get(9)?,
        scope_name: row.get(10)?,
        scope_version: row.get(11)?,
        attributes: parse_json(row.get(12)?)?,
        source: TelemetrySource::from_storage(row.get::<_, String>(13)?.as_str()),
    })
}

fn map_frontend_error(row: &rusqlite::Row<'_>) -> rusqlite::Result<FrontendError> {
    Ok(FrontendError {
        session_id: row.get(0)?,
        timestamp_unix_nanos: row.get(1)?,
        message: row.get(2)?,
        stack: row.get(3)?,
        filename: row.get(4)?,
        line_number: row.get(5)?,
        column_number: row.get(6)?,
        error_type: row.get(7)?,
        trace_id: row.get(8)?,
        span_id: row.get(9)?,
        window_label: row.get(10)?,
        attributes: parse_json(row.get(11)?)?,
    })
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        service_name: row.get(1)?,
        service_version: row.get(2)?,
        app_identifier: row.get(3)?,
        pid: row.get(4)?,
        started_at: row.get(5)?,
        ended_at: row.get(6)?,
        schema_version: row.get(7)?,
        auditaur_version: row.get(8)?,
    })
}

fn map_trace_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<TraceSummary> {
    let start_time_unix_nanos: Option<i64> = row.get(2)?;
    let end_time_unix_nanos: Option<i64> = row.get(3)?;
    let duration_unix_nanos = start_time_unix_nanos
        .zip(end_time_unix_nanos)
        .map(|(start, end)| end.saturating_sub(start).max(0));
    let span_count: i64 = row.get(4)?;
    let error_count: i64 = row.get(5)?;

    Ok(TraceSummary {
        trace_id: row.get(0)?,
        root_span_name: row.get(1)?,
        start_time_unix_nanos,
        duration_unix_nanos,
        status_code: row.get(6)?,
        span_count: span_count.max(0) as usize,
        error_count: error_count.max(0) as usize,
    })
}

const REQUIRED_TABLES: &[&str] = &[
    "schema_migrations",
    "sessions",
    "resources",
    "logs",
    "spans",
    "span_events",
    "span_links",
    "frontend_errors",
    "tauri_ipc_calls",
    "tauri_events",
    "tauri_windows",
    "metrics",
    "screenshots",
];

const REQUIRED_COLUMNS: &[(&str, &str)] = &[
    ("logs", "source"),
    ("spans", "source"),
    ("frontend_errors", "attributes_json"),
    ("sessions", "schema_version"),
];

const LOGS_SELECT: &str = "SELECT session_id, timestamp_unix_nanos, observed_timestamp_unix_nanos,
    severity_text, severity_number, body, body_json, trace_id, span_id, scope_name, scope_version,
    attributes_json, source FROM logs ORDER BY timestamp_unix_nanos DESC LIMIT ?1";

const LOGS_SELECT_WITH_SESSION: &str = "SELECT session_id, timestamp_unix_nanos,
    observed_timestamp_unix_nanos, severity_text, severity_number, body, body_json, trace_id,
    span_id, scope_name, scope_version, attributes_json, source FROM logs WHERE session_id = ?1
    ORDER BY timestamp_unix_nanos DESC LIMIT ?2";

const LOGS_SELECT_WITH_TRACE: &str = "SELECT session_id, timestamp_unix_nanos,
    observed_timestamp_unix_nanos, severity_text, severity_number, body, body_json, trace_id,
    span_id, scope_name, scope_version, attributes_json, source FROM logs WHERE trace_id = ?1
    ORDER BY timestamp_unix_nanos DESC LIMIT ?2";

const LOGS_SELECT_WITH_SESSION_AND_TRACE: &str = "SELECT session_id, timestamp_unix_nanos,
    observed_timestamp_unix_nanos, severity_text, severity_number, body, body_json, trace_id,
    span_id, scope_name, scope_version, attributes_json, source FROM logs
    WHERE session_id = ?1 AND trace_id = ?2 ORDER BY timestamp_unix_nanos DESC LIMIT ?3";

const SPANS_SELECT: &str = "SELECT session_id, trace_id, span_id, parent_span_id, name, kind,
    start_time_unix_nanos, end_time_unix_nanos, status_code, status_message, scope_name,
    scope_version, attributes_json, source FROM spans ORDER BY start_time_unix_nanos DESC LIMIT ?1";

const SPANS_SELECT_WITH_SESSION: &str = "SELECT session_id, trace_id, span_id, parent_span_id,
    name, kind, start_time_unix_nanos, end_time_unix_nanos, status_code, status_message,
    scope_name, scope_version, attributes_json, source FROM spans WHERE session_id = ?1
    ORDER BY start_time_unix_nanos DESC LIMIT ?2";

const SPANS_SELECT_WITH_TRACE: &str = "SELECT session_id, trace_id, span_id, parent_span_id,
    name, kind, start_time_unix_nanos, end_time_unix_nanos, status_code, status_message,
    scope_name, scope_version, attributes_json, source FROM spans WHERE trace_id = ?1
    ORDER BY start_time_unix_nanos DESC LIMIT ?2";

const SPANS_SELECT_WITH_SESSION_AND_TRACE: &str = "SELECT session_id, trace_id, span_id,
    parent_span_id, name, kind, start_time_unix_nanos, end_time_unix_nanos, status_code,
    status_message, scope_name, scope_version, attributes_json, source FROM spans
    WHERE session_id = ?1 AND trace_id = ?2 ORDER BY start_time_unix_nanos DESC LIMIT ?3";

const FRONTEND_ERRORS_SELECT: &str = "SELECT session_id, timestamp_unix_nanos, message, stack,
    filename, line_number, column_number, error_type, trace_id, span_id, window_label,
    attributes_json FROM frontend_errors ORDER BY timestamp_unix_nanos DESC LIMIT ?1";

const FRONTEND_ERRORS_SELECT_WITH_SESSION: &str = "SELECT session_id, timestamp_unix_nanos,
    message, stack, filename, line_number, column_number, error_type, trace_id, span_id,
    window_label, attributes_json FROM frontend_errors WHERE session_id = ?1
    ORDER BY timestamp_unix_nanos DESC LIMIT ?2";

const FRONTEND_ERRORS_SELECT_WITH_TRACE: &str = "SELECT session_id, timestamp_unix_nanos,
    message, stack, filename, line_number, column_number, error_type, trace_id, span_id,
    window_label, attributes_json FROM frontend_errors WHERE trace_id = ?1
    ORDER BY timestamp_unix_nanos DESC LIMIT ?2";

const FRONTEND_ERRORS_SELECT_WITH_SESSION_AND_TRACE: &str =
    "SELECT session_id, timestamp_unix_nanos,
    message, stack, filename, line_number, column_number, error_type, trace_id, span_id,
    window_label, attributes_json FROM frontend_errors WHERE session_id = ?1 AND trace_id = ?2
    ORDER BY timestamp_unix_nanos DESC LIMIT ?3";

const TRACE_SUMMARIES: &str = "SELECT
    grouped.trace_id,
    root.name,
    grouped.start_time_unix_nanos,
    grouped.end_time_unix_nanos,
    grouped.span_count,
    grouped.error_count,
    CASE WHEN grouped.error_count > 0 THEN 'ERROR' ELSE root.status_code END
FROM (
    SELECT trace_id,
           MIN(start_time_unix_nanos) AS start_time_unix_nanos,
           MAX(COALESCE(end_time_unix_nanos, start_time_unix_nanos)) AS end_time_unix_nanos,
           COUNT(*) AS span_count,
           SUM(CASE WHEN status_code = 'ERROR' THEN 1 ELSE 0 END) AS error_count,
           MAX(start_time_unix_nanos) AS latest_start_time_unix_nanos
    FROM spans
    GROUP BY trace_id
) grouped
LEFT JOIN spans root ON root.id = (
    SELECT id FROM spans candidate
    WHERE candidate.trace_id = grouped.trace_id
    ORDER BY CASE WHEN candidate.parent_span_id IS NULL THEN 0 ELSE 1 END,
             candidate.start_time_unix_nanos ASC,
             candidate.span_id ASC
    LIMIT 1
)
ORDER BY grouped.latest_start_time_unix_nanos DESC, grouped.trace_id ASC
LIMIT ?1";

const TRACE_SUMMARIES_WITH_SESSION: &str = "SELECT
    grouped.trace_id,
    root.name,
    grouped.start_time_unix_nanos,
    grouped.end_time_unix_nanos,
    grouped.span_count,
    grouped.error_count,
    CASE WHEN grouped.error_count > 0 THEN 'ERROR' ELSE root.status_code END
FROM (
    SELECT trace_id,
           MIN(start_time_unix_nanos) AS start_time_unix_nanos,
           MAX(COALESCE(end_time_unix_nanos, start_time_unix_nanos)) AS end_time_unix_nanos,
           COUNT(*) AS span_count,
           SUM(CASE WHEN status_code = 'ERROR' THEN 1 ELSE 0 END) AS error_count,
           MAX(start_time_unix_nanos) AS latest_start_time_unix_nanos
    FROM spans
    WHERE session_id = ?1
    GROUP BY trace_id
) grouped
LEFT JOIN spans root ON root.id = (
    SELECT id FROM spans candidate
    WHERE candidate.session_id = ?1 AND candidate.trace_id = grouped.trace_id
    ORDER BY CASE WHEN candidate.parent_span_id IS NULL THEN 0 ELSE 1 END,
             candidate.start_time_unix_nanos ASC,
             candidate.span_id ASC
    LIMIT 1
)
ORDER BY grouped.latest_start_time_unix_nanos DESC, grouped.trace_id ASC
LIMIT ?2";

const MIGRATION_1: &str = include_str!("schema_v1.sql");

#[cfg(test)]
mod tests {
    use super::{SqliteStore, SQLITE_SCHEMA_VERSION};
    use auditaur_core::{
        model::{FrontendError, LogRecord, Session, SpanRecord, TelemetrySource},
        storage::{FrontendErrorQuery, LogQuery, SpanQuery},
    };
    use serde_json::json;
    use tempfile::NamedTempFile;

    #[test]
    fn migrates_schema_idempotently() {
        let store = SqliteStore::open_in_memory().unwrap();

        store.migrate().unwrap();
        store.migrate().unwrap();
        store.validate_schema().unwrap();
    }

    #[test]
    fn inserts_and_queries_sample_telemetry() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.migrate().unwrap();

        let session = sample_session();
        store.create_session(&session).unwrap();

        store
            .insert_log(&LogRecord {
                session_id: session.id.clone(),
                timestamp_unix_nanos: 100,
                observed_timestamp_unix_nanos: Some(110),
                severity_text: Some("INFO".to_string()),
                severity_number: Some(9),
                body: Some("hello".to_string()),
                body_json: Some(json!({ "message": "hello" })),
                trace_id: Some("trace-1".to_string()),
                span_id: Some("span-1".to_string()),
                scope_name: Some("third.party.logger".to_string()),
                scope_version: Some("1.2.3".to_string()),
                attributes: json!({ "library": "external" }),
                source: TelemetrySource::ThirdPartyOtel,
            })
            .unwrap();

        store
            .insert_span(&SpanRecord {
                session_id: session.id.clone(),
                trace_id: "trace-1".to_string(),
                span_id: "span-1".to_string(),
                parent_span_id: None,
                name: "sql query".to_string(),
                kind: Some("client".to_string()),
                start_time_unix_nanos: 90,
                end_time_unix_nanos: Some(120),
                status_code: Some("OK".to_string()),
                status_message: None,
                scope_name: Some("sqlx".to_string()),
                scope_version: Some("0.8".to_string()),
                attributes: json!({ "db.system": "sqlite" }),
                source: TelemetrySource::ThirdPartyOtel,
            })
            .unwrap();

        store
            .insert_frontend_error(&FrontendError {
                session_id: session.id.clone(),
                timestamp_unix_nanos: 130,
                message: "boom".to_string(),
                stack: Some("Error: boom".to_string()),
                filename: Some("main.ts".to_string()),
                line_number: Some(12),
                column_number: Some(8),
                error_type: Some("Error".to_string()),
                trace_id: Some("trace-1".to_string()),
                span_id: Some("span-1".to_string()),
                window_label: Some("main".to_string()),
                attributes: json!({ "handled": false }),
            })
            .unwrap();

        let logs = store
            .list_logs(&LogQuery {
                session_id: Some(session.id.clone()),
                trace_id: None,
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].source, TelemetrySource::ThirdPartyOtel);
        assert_eq!(logs[0].attributes["library"], "external");

        let spans = store
            .list_spans(&SpanQuery {
                session_id: Some(session.id.clone()),
                trace_id: Some("trace-1".to_string()),
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].scope_name.as_deref(), Some("sqlx"));
        assert_eq!(spans[0].source, TelemetrySource::ThirdPartyOtel);

        let errors = store
            .list_frontend_errors(&FrontendErrorQuery {
                session_id: Some(session.id),
                trace_id: None,
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "boom");
    }

    #[test]
    fn can_reopen_session_after_telemetry_exists() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.migrate().unwrap();

        let mut session = sample_session();
        store.create_session(&session).unwrap();
        store
            .insert_log(&LogRecord {
                session_id: session.id.clone(),
                timestamp_unix_nanos: 100,
                observed_timestamp_unix_nanos: None,
                severity_text: Some("INFO".to_string()),
                severity_number: Some(9),
                body: Some("first".to_string()),
                body_json: None,
                trace_id: None,
                span_id: None,
                scope_name: None,
                scope_version: None,
                attributes: json!({}),
                source: TelemetrySource::Backend,
            })
            .unwrap();

        session.service_version = Some("0.2.0".to_string());
        store.create_session(&session).unwrap();

        let reopened = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(reopened.service_version.as_deref(), Some("0.2.0"));
        assert_eq!(store.list_logs(&LogQuery::default()).unwrap().len(), 1);
    }

    #[test]
    fn supports_multiple_connections_to_session_file() {
        let db = NamedTempFile::new().unwrap();
        let writer = SqliteStore::open(db.path()).unwrap();
        writer.migrate().unwrap();
        writer.create_session(&sample_session()).unwrap();

        let reader = SqliteStore::open(db.path()).unwrap();
        reader.validate_schema().unwrap();

        let sessions = reader.list_sessions(Some(10)).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session-1");
    }

    #[test]
    fn trace_summary_counts_more_than_default_row_limit() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.migrate().unwrap();
        let session = sample_session();
        store.create_session(&session).unwrap();

        for index in 0..1_050 {
            store
                .insert_span(&SpanRecord {
                    session_id: session.id.clone(),
                    trace_id: "large-trace".to_string(),
                    span_id: format!("span-{index}"),
                    parent_span_id: if index == 0 {
                        None
                    } else {
                        Some("span-0".to_string())
                    },
                    name: if index == 0 {
                        "root".to_string()
                    } else {
                        format!("child-{index}")
                    },
                    kind: Some("internal".to_string()),
                    start_time_unix_nanos: index,
                    end_time_unix_nanos: Some(index + 1),
                    status_code: Some("OK".to_string()),
                    status_message: None,
                    scope_name: Some("bulk".to_string()),
                    scope_version: None,
                    attributes: json!({}),
                    source: TelemetrySource::ThirdPartyOtel,
                })
                .unwrap();
        }

        let summaries = store
            .list_trace_summaries(Some(&session.id), Some(10))
            .unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].root_span_name.as_deref(), Some("root"));
        assert_eq!(summaries[0].span_count, 1_050);
    }

    fn sample_session() -> Session {
        Session {
            id: "session-1".to_string(),
            service_name: "auditaur-test".to_string(),
            service_version: Some("0.1.0".to_string()),
            app_identifier: Some("dev.auditaur.test".to_string()),
            pid: Some(123),
            started_at: "2026-05-18T18:00:00Z".to_string(),
            ended_at: None,
            schema_version: SQLITE_SCHEMA_VERSION,
            auditaur_version: Some("0.1.0".to_string()),
        }
    }
}
