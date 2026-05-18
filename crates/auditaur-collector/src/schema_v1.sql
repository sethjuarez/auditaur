CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  session_name TEXT,
  service_name TEXT NOT NULL,
  service_version TEXT,
  app_identifier TEXT,
  pid INTEGER,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  schema_version INTEGER NOT NULL,
  auditaur_version TEXT
);

CREATE TABLE IF NOT EXISTS resources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  service_name TEXT,
  service_version TEXT,
  attributes_json TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE TABLE IF NOT EXISTS logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  resource_id INTEGER,
  timestamp_unix_nanos INTEGER NOT NULL,
  observed_timestamp_unix_nanos INTEGER,
  severity_text TEXT,
  severity_number INTEGER,
  body TEXT,
  body_json TEXT,
  trace_id TEXT,
  span_id TEXT,
  scope_name TEXT,
  scope_version TEXT,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  source TEXT NOT NULL CHECK(source IN ('frontend', 'backend', 'plugin', 'third_party_otel')),
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(resource_id) REFERENCES resources(id)
);

CREATE INDEX IF NOT EXISTS idx_logs_session_time ON logs(session_id, timestamp_unix_nanos DESC);
CREATE INDEX IF NOT EXISTS idx_logs_trace ON logs(trace_id);
CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(session_id, severity_number);

CREATE TABLE IF NOT EXISTS spans (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  resource_id INTEGER,
  trace_id TEXT NOT NULL,
  span_id TEXT NOT NULL,
  parent_span_id TEXT,
  name TEXT NOT NULL,
  kind TEXT,
  start_time_unix_nanos INTEGER NOT NULL,
  end_time_unix_nanos INTEGER,
  status_code TEXT,
  status_message TEXT,
  scope_name TEXT,
  scope_version TEXT,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  source TEXT NOT NULL CHECK(source IN ('frontend', 'backend', 'plugin', 'third_party_otel')),
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(resource_id) REFERENCES resources(id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_spans_identity ON spans(session_id, trace_id, span_id);
CREATE INDEX IF NOT EXISTS idx_spans_session_time ON spans(session_id, start_time_unix_nanos DESC);
CREATE INDEX IF NOT EXISTS idx_spans_trace ON spans(session_id, trace_id);
CREATE INDEX IF NOT EXISTS idx_spans_trace_only ON spans(trace_id);
CREATE INDEX IF NOT EXISTS idx_spans_status ON spans(session_id, status_code);

CREATE TABLE IF NOT EXISTS span_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  span_id TEXT NOT NULL,
  name TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_span_events_span ON span_events(session_id, trace_id, span_id);

CREATE TABLE IF NOT EXISTS span_links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  span_id TEXT NOT NULL,
  linked_trace_id TEXT NOT NULL,
  linked_span_id TEXT NOT NULL,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE TABLE IF NOT EXISTS frontend_errors (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  message TEXT NOT NULL,
  stack TEXT,
  filename TEXT,
  line_number INTEGER,
  column_number INTEGER,
  error_type TEXT,
  trace_id TEXT,
  span_id TEXT,
  window_label TEXT,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_frontend_errors_time ON frontend_errors(session_id, timestamp_unix_nanos DESC);

CREATE TABLE IF NOT EXISTS tauri_ipc_calls (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  duration_ms REAL,
  command TEXT NOT NULL,
  status TEXT NOT NULL,
  error_message TEXT,
  trace_id TEXT,
  span_id TEXT,
  window_label TEXT,
  args_json TEXT,
  args_redacted INTEGER NOT NULL DEFAULT 1,
  result_summary TEXT,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_ipc_session_time ON tauri_ipc_calls(session_id, timestamp_unix_nanos DESC);
CREATE INDEX IF NOT EXISTS idx_ipc_command ON tauri_ipc_calls(session_id, command);
CREATE INDEX IF NOT EXISTS idx_ipc_status ON tauri_ipc_calls(session_id, status);

CREATE TABLE IF NOT EXISTS tauri_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  event_name TEXT NOT NULL,
  direction TEXT NOT NULL,
  target TEXT,
  trace_id TEXT,
  span_id TEXT,
  window_label TEXT,
  payload_summary TEXT,
  payload_json TEXT,
  payload_redacted INTEGER NOT NULL DEFAULT 1,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_events_session_time ON tauri_events(session_id, timestamp_unix_nanos DESC);
CREATE INDEX IF NOT EXISTS idx_events_name ON tauri_events(session_id, event_name);

CREATE TABLE IF NOT EXISTS tauri_windows (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  window_label TEXT NOT NULL,
  webview_label TEXT,
  url TEXT,
  title TEXT,
  focused INTEGER,
  visible INTEGER,
  width REAL,
  height REAL,
  scale_factor REAL,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_windows_session_time ON tauri_windows(session_id, timestamp_unix_nanos DESC);

CREATE TABLE IF NOT EXISTS metrics (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  resource_id INTEGER,
  timestamp_unix_nanos INTEGER NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  unit TEXT,
  kind TEXT NOT NULL,
  value REAL,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  source TEXT NOT NULL CHECK(source IN ('frontend', 'backend', 'plugin', 'third_party_otel')),
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(resource_id) REFERENCES resources(id)
);

CREATE INDEX IF NOT EXISTS idx_metrics_session_time ON metrics(session_id, timestamp_unix_nanos DESC);
CREATE INDEX IF NOT EXISTS idx_metrics_name ON metrics(session_id, name);

CREATE TABLE IF NOT EXISTS screenshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  window_label TEXT,
  path TEXT NOT NULL,
  width INTEGER,
  height INTEGER,
  mime_type TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);
