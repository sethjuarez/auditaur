# Auditaur implementation plan

Auditaur is a local-first observability and debugging toolkit for Tauri apps. It should let humans and AI agents inspect a running Tauri app without asking the developer to copy/paste terminal output, browser console logs, or screenshots.

The product should feel like "Aspire MCP for Tauri", but with OpenTelemetry as the telemetry model and SQLite as the local debug-session store.

## Product summary

**Name:** Auditaur

**Domain:** auditaur.dev

**Tagline:** Runtime observability for Tauri apps and AI agents.

**Primary use case:** A developer enables Auditaur in a Tauri app during development. The app emits OpenTelemetry-compatible logs, traces, metrics, frontend errors, Tauri command calls, Tauri events, and window/webview state into a local SQLite database. The `auditaur` CLI and `auditaur mcp` server query that local data for humans and coding agents.

## Goals

1. Provide a cross-platform local debugging loop for Tauri apps on Windows, macOS, and Linux.
2. Reuse OpenTelemetry concepts and data shapes as much as practical.
3. Preserve telemetry emitted by other OpenTelemetry-instrumented libraries, not only spans/logs created by Auditaur wrappers.
4. Persist debug data to local SQLite when Auditaur is explicitly enabled.
5. Expose the same data through both:
   - a human CLI
   - an MCP server for AI coding agents
6. Keep the security model local-first, development-only by default, and explicit.
7. Make adoption easy for Tauri v2 apps with:
   - a Rust plugin
   - a TypeScript frontend API package
   - a single Rust CLI binary
8. Provide an example Tauri app that demonstrates frontend and backend telemetry.

## Non-goals for the MVP

1. Do not build a cloud telemetry service.
2. Do not replace the official OpenTelemetry Collector.
3. Do not open unauthenticated network ports by default.
4. Do not collect source code, environment variables, secrets, raw request bodies, or arbitrary filesystem contents.
5. Do not require a daemon for the first version.
6. Do not try to support every OpenTelemetry signal perfectly in v1. Start with traces and logs, then add metrics.
7. Do not make production telemetry the initial target. Auditaur is a development/debugging tool first.

## Target repository layout

```text
auditaur/
├── .github/
│   └── workflows/
│       └── publish-docs.yml
├── Cargo.toml
├── README.md
├── docs/
│   ├── package.json
│   ├── package-lock.json
│   ├── astro.config.mjs
│   ├── tsconfig.json
│   ├── public/
│   │   ├── CNAME
│   │   ├── auditaur.svg
│   │   └── favicon.svg
│   ├── src/
│   │   ├── content.config.ts
│   │   ├── styles/
│   │   │   └── custom.css
│   │   └── content/
│   │       └── docs/
│   │           ├── index.mdx
│   │           ├── welcome.mdx
│   │           ├── getting-started/
│   │           ├── concepts/
│   │           ├── reference/
│   │           └── roadmap/
│   └── reference/
│       ├── IMPLEMENTATION_PLAN.md
│       ├── ARCHITECTURE.md
│       ├── SECURITY.md
│       ├── MCP_TOOLS.md
│       └── SQLITE_SCHEMA.md
├── crates/
│   ├── auditaur-cli/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── commands/
│   │       ├── mcp/
│   │       └── output.rs
│   ├── auditaur-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs
│   │       ├── model/
│   │       ├── otel/
│   │       ├── redaction.rs
│   │       ├── storage/
│   │       └── protocol/
│   ├── auditaur-collector/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── embedded.rs
│   │       ├── receiver.rs
│   │       ├── exporter_sqlite.rs
│   │       └── retention.rs
│   └── tauri-plugin-auditaur/
│       ├── Cargo.toml
│       ├── permissions/
│       └── src/
│           ├── lib.rs
│           ├── commands.rs
│           ├── desktop.rs
│           ├── error.rs
│           └── state.rs
├── packages/
│   └── api/
│       ├── package.json
│       ├── tsconfig.json
│       └── src/
│           ├── index.ts
│           ├── init.ts
│           ├── exporter.ts
│           ├── console.ts
│           ├── errors.ts
│           ├── invoke.ts
│           └── events.ts
└── examples/
    └── basic-tauri-app/
```

The workspace should be Rust-first. TypeScript exists only where needed for frontend instrumentation.

The documentation site should follow the same pattern as CutReady: an Astro/Starlight site under `docs/`, with long-form planning/reference documents kept under `docs/reference/`, and a GitHub Pages deployment workflow at `.github/workflows/publish-docs.yml`.

## Components

### `auditaur-core`

Shared library for data models, configuration, redaction, storage interfaces, and protocol types.

Responsibilities:

1. Define Auditaur configuration structs.
2. Define normalized models for logs, spans, metrics, resources, Tauri windows, Tauri IPC calls, events, frontend errors, screenshots, and sessions.
3. Define OpenTelemetry mapping helpers.
4. Define SQLite storage traits and query structs.
5. Define redaction helpers.
6. Define local discovery file structs.
7. Define MCP response DTOs that are safe and bounded.
8. Preserve generic OpenTelemetry spans/logs/attributes emitted by third-party libraries even when no Auditaur-specific Tauri fields are present.

Suggested dependencies:

```toml
anyhow = "1"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
time = { version = "0.3", features = ["serde", "formatting", "parsing"] }
uuid = { version = "1", features = ["v4", "serde"] }
opentelemetry = "0.30"
```

Use exact current versions when implementing; the versions above are directional.

### `auditaur-collector`

The local collector library. For the MVP, this should support an embedded collector that runs inside the Tauri process through the plugin.

Responsibilities:

1. Receive telemetry batches from:
   - frontend TypeScript exporter through a Tauri command
   - Rust `tracing` layer/exporter
   - any OpenTelemetry-compatible library instrumentation used by the app
   - direct plugin lifecycle/window/event instrumentation
2. Normalize data into Auditaur storage records.
3. Write records to SQLite.
4. Apply redaction before persistence.
5. Enforce retention limits.
6. Optionally expose a query handle to the plugin for live inspection.

Important design point: the collector can be "Auditaur-specific", but its inputs and schema should remain OpenTelemetry-aligned. Auditaur-specific convenience records are additive; generic OTEL spans, logs, resource attributes, scope names, span events, and links from third-party libraries must still be stored and queryable.

### `tauri-plugin-auditaur`

Tauri v2 plugin used by app developers.

Responsibilities:

1. Initialize Auditaur for a Tauri app.
2. Create or open a local SQLite debug session.
3. Register commands for frontend telemetry export.
4. Capture Tauri app/window/webview lifecycle state where possible.
5. Provide dev-only commands for:
   - listing windows
   - capturing a window screenshot, if practical
   - opening devtools, if enabled by build mode
6. Install or expose Rust `tracing` integration.
7. Write a local discovery file so the CLI can find active sessions.
8. Clean up stale discovery files on shutdown.

Suggested plugin API:

```rust
use tauri_plugin_auditaur::{AuditaurConfig, AuditaurExt};

tauri::Builder::default()
    .plugin(
        tauri_plugin_auditaur::Builder::new()
            .service_name("my-tauri-app")
            .session_name("dev")
            .redact_defaults(true)
            .max_session_bytes(256 * 1024 * 1024)
            .build(),
    )
    .run(tauri::generate_context!())?;
```

The builder should default to development/debug only. If the app tries to enable Auditaur in a release build, require explicit opt-in:

```rust
.allow_release_builds(false)
```

Default behavior:

1. Enabled automatically only when `cfg!(debug_assertions)` is true or `AUDITAUR=1` is set.
2. Writes to the OS app-data directory under `auditaur`.
3. Redacts common sensitive keys.
4. Uses bounded retention.

### `@auditaur/api`

TypeScript package for frontend instrumentation.

Responsibilities:

1. Initialize OpenTelemetry JS.
2. Provide an Auditaur exporter that sends OTEL-shaped batches to the Rust plugin with `invoke("plugin:auditaur|export_otel_batch", ...)`.
3. Instrument `console` calls.
4. Capture `window.onerror` and `unhandledrejection`.
5. Wrap Tauri `invoke` calls with spans.
6. Wrap Tauri `emit`, `emitTo`, and `listen` calls with events/spans.
7. Add Tauri-specific resource attributes:
   - `service.name`
   - `service.version`
   - `tauri.app.identifier`
   - `tauri.window.label`
   - `tauri.webview.label`
   - `auditaur.session.id`

Suggested API:

```ts
import { initAuditaur } from '@auditaur/api';

const auditaur = await initAuditaur({
  serviceName: 'cutready',
  serviceVersion: __APP_VERSION__,
  instrumentConsole: true,
  instrumentErrors: true,
  instrumentTauriInvoke: true,
  instrumentTauriEvents: true,
  batchIntervalMs: 1000,
});

export const invoke = auditaur.invoke;
export const emit = auditaur.emit;
export const listen = auditaur.listen;
```

Also support a lower-friction mode:

```ts
await initAuditaur({ serviceName: 'my-app' });
```

Default frontend instrumentation should be conservative and avoid capturing large payloads.

### `auditaur-cli`

Cross-platform command-line application and MCP server.

Responsibilities:

1. Discover local Auditaur sessions.
2. Query SQLite stores.
3. Print human-readable tables and JSON.
4. Run the stdio MCP server.
5. Provide `doctor` diagnostics.
6. Provide future collection modes.

Suggested CLI commands:

```text
auditaur doctor
auditaur init
auditaur sessions
auditaur apps
auditaur logs [--app <name>] [--session <id>] [--since <duration>] [--level <level>] [--json]
auditaur errors [--app <name>] [--session <id>] [--since <duration>] [--json]
auditaur traces [--app <name>] [--failed] [--since <duration>] [--json]
auditaur trace <trace-id> [--json]
auditaur ipc [--command <name>] [--failed] [--json]
auditaur events [--event <name>] [--json]
auditaur windows [--app <name>] [--json]
auditaur sql [--session <id>] --query "<read-only SQL>"
auditaur mcp
```

`auditaur sql` should be read-only and should reject multiple statements. It is useful for power users and agents, but keep it safe.

Future sidecar command:

```text
auditaur collect
```

Do not implement the sidecar collector until the embedded flow works.

### Documentation website

Auditaur should ship with a public documentation website at `https://auditaur.dev`.

Use the CutReady docs setup as the reference implementation:

1. Use Astro with Starlight.
2. Keep the site app in `docs/`.
3. Keep source docs in `docs/src/content/docs/`.
4. Keep implementation/reference planning documents in `docs/reference/`.
5. Add `docs/public/CNAME` with:

```text
auditaur.dev
```

6. Configure `docs/astro.config.mjs` with:

```js
site: "https://auditaur.dev"
```

7. Publish through GitHub Pages with `.github/workflows/publish-docs.yml`.

Recommended initial docs IA:

```text
/
/welcome/
/getting-started/installation/
/getting-started/quick-start/
/getting-started/tauri-plugin/
/getting-started/frontend-api/
/concepts/local-first-observability/
/concepts/opentelemetry-model/
/concepts/sqlite-session-store/
/reference/cli/
/reference/mcp-tools/
/reference/configuration/
/reference/sqlite-schema/
/reference/security/
/roadmap/
```

Initial docs content should explain the product, installation status, development-only security posture, local SQLite data locations, redaction defaults, CLI usage, MCP usage, and Tauri integration examples. The docs should clearly label incomplete or planned functionality while the MVP is being built.

GitHub Pages workflow:

```yaml
name: Deploy Docs to GitHub Pages

on:
  push:
    branches: [main]
    paths:
      - 'docs/**'
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout your repository using git
        uses: actions/checkout@v6

      - name: Install, build, and upload your site
        uses: withastro/action@v6
        with:
          path: ./docs
          node-version: 24

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v5
```

Domain setup:

1. Configure GitHub Pages for the repository to deploy from GitHub Actions.
2. Add the custom domain `auditaur.dev` in the repository Pages settings.
3. Keep `docs/public/CNAME` committed so deployments preserve the custom domain.
4. Point DNS for `auditaur.dev` at GitHub Pages according to GitHub's current custom-domain guidance.
5. Prefer adding `www.auditaur.dev` as a redirect/alias to the apex domain if the DNS provider supports it.

## Architecture

### MVP architecture

```text
Tauri frontend
  - OpenTelemetry JS
  - Auditaur exporter
  - console/error/invoke/event instrumentation
        |
        | Tauri invoke
        v
tauri-plugin-auditaur
  - receives frontend OTEL batches
  - captures Rust/Tauri lifecycle data
  - receives Rust tracing events/spans
  - redacts sensitive data
        |
        | local SQLite writes
        v
Auditaur SQLite session database
        ^
        | direct file query
        |
auditaur CLI / auditaur mcp
```

### Future architecture with sidecar collector

```text
Tauri app(s)
  - OTEL exporters
  - Auditaur plugin
        |
        | named pipe / Unix domain socket / localhost OTLP
        v
auditaur collect
  - local collector process
  - multi-app sessions
  - SQLite storage
        ^
        |
auditaur CLI / auditaur mcp
```

The future sidecar should support standard OTLP/HTTP on `127.0.0.1` only when explicitly enabled. It must validate origin/host behavior if any HTTP surface is introduced.

## Storage model

Use SQLite by default when Auditaur is enabled.

Default locations:

```text
Windows: %LOCALAPPDATA%\auditaur\sessions\<session-id>\telemetry.sqlite
macOS: ~/Library/Application Support/auditaur/sessions/<session-id>/telemetry.sqlite
Linux: ~/.local/share/auditaur/sessions/<session-id>/telemetry.sqlite
```

Discovery files:

```text
Windows: %LOCALAPPDATA%\auditaur\apps\<instance-id>.json
macOS: ~/Library/Application Support/auditaur/apps/<instance-id>.json
Linux: ~/.local/share/auditaur/apps/<instance-id>.json
```

Discovery file shape:

```json
{
  "schemaVersion": 1,
  "instanceId": "uuid",
  "sessionId": "uuid",
  "serviceName": "cutready",
  "serviceVersion": "0.9.0",
  "appIdentifier": "com.example.cutready",
  "pid": 12345,
  "startedAt": "2026-05-18T18:00:00Z",
  "databasePath": "absolute path to telemetry.sqlite",
  "capabilities": ["logs", "traces", "windows", "ipc", "events"],
  "lastHeartbeatAt": "2026-05-18T18:01:00Z"
}
```

The CLI should ignore stale discovery files when:

1. the process no longer exists, or
2. the heartbeat is older than a conservative timeout, for example 30 seconds.

For the MVP, querying the SQLite file directly is enough. Local IPC can come later for live commands such as screenshots and opening devtools.

## SQLite schema

Keep the schema OTEL-aligned, but do not try to copy the protobuf model exactly if it makes queries painful.

Initial tables:

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  service_name TEXT NOT NULL,
  service_version TEXT,
  app_identifier TEXT,
  pid INTEGER,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  schema_version INTEGER NOT NULL,
  auditaur_version TEXT
);

CREATE TABLE resources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  service_name TEXT,
  service_version TEXT,
  attributes_json TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE TABLE logs (
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
  source TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(resource_id) REFERENCES resources(id)
);

CREATE INDEX idx_logs_session_time ON logs(session_id, timestamp_unix_nanos DESC);
CREATE INDEX idx_logs_trace ON logs(trace_id);
CREATE INDEX idx_logs_level ON logs(session_id, severity_number);

CREATE TABLE spans (
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
  source TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(resource_id) REFERENCES resources(id)
);

CREATE UNIQUE INDEX idx_spans_identity ON spans(session_id, trace_id, span_id);
CREATE INDEX idx_spans_session_time ON spans(session_id, start_time_unix_nanos DESC);
CREATE INDEX idx_spans_trace ON spans(session_id, trace_id);
CREATE INDEX idx_spans_status ON spans(session_id, status_code);

CREATE TABLE span_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  span_id TEXT NOT NULL,
  name TEXT NOT NULL,
  timestamp_unix_nanos INTEGER NOT NULL,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE INDEX idx_span_events_span ON span_events(session_id, trace_id, span_id);

CREATE TABLE span_links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  span_id TEXT NOT NULL,
  linked_trace_id TEXT NOT NULL,
  linked_span_id TEXT NOT NULL,
  attributes_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE TABLE frontend_errors (
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

CREATE INDEX idx_frontend_errors_time ON frontend_errors(session_id, timestamp_unix_nanos DESC);

CREATE TABLE tauri_ipc_calls (
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

CREATE INDEX idx_ipc_session_time ON tauri_ipc_calls(session_id, timestamp_unix_nanos DESC);
CREATE INDEX idx_ipc_command ON tauri_ipc_calls(session_id, command);
CREATE INDEX idx_ipc_status ON tauri_ipc_calls(session_id, status);

CREATE TABLE tauri_events (
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

CREATE INDEX idx_events_session_time ON tauri_events(session_id, timestamp_unix_nanos DESC);
CREATE INDEX idx_events_name ON tauri_events(session_id, event_name);

CREATE TABLE tauri_windows (
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

CREATE INDEX idx_windows_session_time ON tauri_windows(session_id, timestamp_unix_nanos DESC);

CREATE TABLE metrics (
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
  source TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(resource_id) REFERENCES resources(id)
);

CREATE INDEX idx_metrics_session_time ON metrics(session_id, timestamp_unix_nanos DESC);
CREATE INDEX idx_metrics_name ON metrics(session_id, name);

CREATE TABLE screenshots (
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
```

Store JSON attributes as text initially. Consider SQLite JSON indexes later only if needed.

Schema migrations:

1. Use a simple `schema_migrations` table.
2. Run migrations at plugin startup.
3. Never silently delete old data.
4. If schema is incompatible, surface a clear error and suggest `auditaur clean` or a migration.

## OpenTelemetry mapping

Use OTEL semantics wherever possible.

Auditaur must support both:

1. Auditaur-created telemetry, such as wrapped Tauri invokes and frontend errors.
2. Generic OpenTelemetry telemetry emitted by app code or third-party libraries.

Third-party spans should be persisted even when they do not include Tauri-specific attributes. The CLI and MCP tools should show them in trace views, preserve their resource/scope metadata, and avoid assuming every span maps to a Tauri command, event, or window.

### Resource attributes

Recommended attributes:

```text
service.name
service.version
service.instance.id
telemetry.sdk.name
telemetry.sdk.language
telemetry.sdk.version
os.type
process.pid
process.executable.name
tauri.app.identifier
tauri.app.version
tauri.window.label
tauri.webview.label
auditaur.session.id
auditaur.enabled
```

### Span conventions

Frontend Tauri command invocation:

```text
name: tauri.invoke <command>
kind: client
attributes:
  tauri.command: <command>
  tauri.window.label: <window>
  auditaur.source: frontend
```

Backend Tauri command execution, if instrumented:

```text
name: tauri.command <command>
kind: server/internal
attributes:
  tauri.command: <command>
  auditaur.source: backend
```

Tauri event:

```text
name: tauri.event <event-name>
attributes:
  tauri.event.name
  tauri.event.direction: emit|listen|receive
  tauri.event.target
  tauri.window.label
```

Frontend error:

```text
span event: exception
attributes:
  exception.type
  exception.message
  exception.stacktrace
  auditaur.source: frontend
```

### Logs

Console logs and Rust logs should become OTEL-like log records:

```text
timestamp
severity_text
severity_number
body
trace_id
span_id
attributes
```

Frontend `console.error` should also create a frontend error record when it includes an Error object.

### Metrics

Metrics can be later than traces/logs. Candidate metrics:

1. command duration histogram
2. command error count
3. frontend error count
4. active windows gauge
5. telemetry dropped count
6. telemetry export batch size
7. SQLite write latency

## Frontend telemetry flow

`@auditaur/api` should use OpenTelemetry JS where practical.

The package should define a custom exporter:

```ts
class AuditaurSpanExporter implements SpanExporter {
  export(spans: ReadableSpan[], resultCallback: (result: ExportResult) => void): void {
    // Convert spans to JSON-safe OTEL-shaped records.
    // Call Tauri plugin command.
  }

  shutdown(): Promise<void> {
    // Flush pending batches.
  }
}
```

Equivalent exporters or batch paths should exist for logs. If the current OTEL JS logs signal is too cumbersome, use an OTEL-shaped internal log record and keep the conversion isolated.

Plugin command:

```rust
#[tauri::command]
async fn export_otel_batch(
    app: tauri::AppHandle,
    window: tauri::Window,
    state: tauri::State<'_, AuditaurState>,
    batch: OTelBatch,
) -> Result<(), AuditaurError>
```

Batch shape:

```json
{
  "resource": {
    "attributes": {
      "service.name": "cutready",
      "service.version": "0.9.0"
    }
  },
  "spans": [],
  "logs": [],
  "metrics": [],
  "tauriIpcCalls": [],
  "tauriEvents": [],
  "frontendErrors": []
}
```

Keep this shape intentionally close to OTEL, but allow Auditaur-specific arrays for high-value Tauri records that need simpler queries.

## Rust backend telemetry flow

Support both `log` and `tracing`.

Recommended MVP:

1. Provide a `tracing_subscriber::Layer` that writes events/spans to Auditaur.
2. Optionally bridge `log` into `tracing` using existing ecosystem support.
3. Use `tracing-opentelemetry` only where it helps preserve OTEL conventions.

Example target API:

```rust
let auditaur_layer = tauri_plugin_auditaur::tracing_layer();

tracing_subscriber::registry()
    .with(tracing_subscriber::EnvFilter::from_default_env())
    .with(tracing_subscriber::fmt::layer())
    .with(auditaur_layer)
    .init();
```

If Tauri plugin initialization order makes a global layer hard, document the recommended app setup clearly.

## MCP server

The MCP server should run over stdio:

```text
auditaur mcp
```

It must not write non-MCP data to stdout. Diagnostic logs go to stderr.

MCP tools for MVP:

```text
list_sessions
list_apps
list_logs
list_errors
list_traces
get_trace
list_ipc_calls
list_events
list_windows
doctor
```

Future tools:

```text
capture_window
open_devtools
query_metrics
list_screenshots
read_screenshot
```

Tool details:

### `list_sessions`

Input:

```json
{
  "activeOnly": false,
  "limit": 20
}
```

Returns recent Auditaur sessions with app name, version, start/end time, database path, active/stale status, and event counts.

### `list_apps`

Returns active apps discovered through discovery files. Include PID, service name, app identifier, session id, capabilities, and last heartbeat.

### `list_logs`

Input:

```json
{
  "sessionId": "optional",
  "serviceName": "optional",
  "since": "10m",
  "level": "warn",
  "contains": "optional substring",
  "limit": 200
}
```

Returns bounded log records. Default limit should be small enough for agent context.

### `list_errors`

Returns frontend errors, panic logs if captured, failed command summaries, and high-severity logs.

### `list_traces`

Input supports:

```json
{
  "failedOnly": true,
  "since": "30m",
  "limit": 100
}
```

Returns trace summaries: trace id, root span, duration, status, span count, error count.

### `get_trace`

Returns one trace with spans, span events, linked logs, frontend errors, IPC calls, and Tauri events.

### `list_ipc_calls`

Filter by command, failed only, since, window label.

### `list_events`

Filter by event name, direction, target, since.

### `list_windows`

Return latest known state for each window/webview in a session.

### `doctor`

Checks:

1. Auditaur data directory exists.
2. Discovery files are parseable.
3. Active sessions point to readable SQLite DBs.
4. SQLite schema version is supported.
5. Stale discovery files are detected.
6. MCP server can enumerate tools.

## CLI UX

Human output should be concise by default and support `--json` everywhere.

Examples:

```text
auditaur apps
auditaur logs --since 5m --level error
auditaur traces --failed
auditaur trace 4bf92f3577b34da6a3ce929d0e0e4736
auditaur ipc --command save_project --failed
auditaur mcp
```

Add `--db <path>` for testing and advanced use.

Do not make the CLI depend on a running app for read-only inspection. It should be able to inspect a completed session database.

## Security and privacy

Security posture:

1. Local-only by default.
2. Explicit enablement.
3. Development/debug only by default.
4. SQLite data is stored on the developer machine.
5. No source code collection.
6. No environment variable collection.
7. No secrets collection.
8. No network listener in the MVP.
9. MCP uses stdio only.
10. Command execution tools are either absent in MVP or heavily gated.

Redaction:

Default redact keys matching:

```text
password
passwd
pwd
secret
token
access_token
refresh_token
id_token
authorization
api_key
apikey
key
cookie
set-cookie
session
credential
connection_string
```

Redaction should apply recursively to JSON objects.

Payload policy:

1. Store summaries by default for command args, command results, and event payloads.
2. Allow full payload capture only through explicit config.
3. Enforce max payload size.
4. Mark rows with `*_redacted = 1` when redaction occurred.

Retention:

Default limits:

```text
max_session_bytes: 256 MB
max_session_age_days: 7
max_log_rows: 100000
max_span_rows: 100000
max_error_rows: 10000
```

When retention limits are hit, delete oldest records first and record a telemetry log stating that data was dropped.

## Configuration

Rust plugin config:

```rust
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
```

Environment variables:

```text
AUDITAUR=1
AUDITAUR_SESSION_NAME=...
AUDITAUR_DATA_DIR=...
AUDITAUR_CAPTURE_FULL_PAYLOADS=0|1
AUDITAUR_MAX_SESSION_BYTES=...
```

Frontend config:

```ts
export interface AuditaurFrontendConfig {
  serviceName: string;
  serviceVersion?: string;
  instrumentConsole?: boolean;
  instrumentErrors?: boolean;
  instrumentTauriInvoke?: boolean;
  instrumentTauriEvents?: boolean;
  captureFullPayloads?: boolean;
  maxPayloadBytes?: number;
  batchIntervalMs?: number;
  maxBatchSize?: number;
}
```

## Implementation milestones

### Milestone 0: repo scaffolding

Deliverables:

1. Rust workspace.
2. Crates:
   - `auditaur-core`
   - `auditaur-collector`
   - `auditaur-cli`
   - `tauri-plugin-auditaur`
3. TypeScript package:
   - `packages/api`
4. Astro/Starlight docs site:
   - `docs/package.json`
   - `docs/astro.config.mjs`
   - `docs/public/CNAME`
   - initial docs IA and landing page
5. Basic CI for:
   - `cargo test --workspace`
   - TypeScript build/test
6. GitHub Pages docs workflow:
   - `.github/workflows/publish-docs.yml`
7. README with install, docs URL, and project status.

Acceptance checks:

```text
cargo test --workspace
cargo run -p auditaur-cli -- doctor
cd docs && npm run build
```

The first docs deployment should publish to GitHub Pages and preserve the `auditaur.dev` custom domain through `docs/public/CNAME`.

### Milestone 1: SQLite session store

Deliverables:

1. SQLite schema and migrations.
2. Session creation/opening.
3. Insert/query logs.
4. Insert/query spans.
5. Insert/query frontend errors.
6. Storage tests with temp databases.

Acceptance checks:

1. Tests create a session DB, insert sample telemetry, query it back.
2. Indexes exist for common query paths.
3. `auditaur doctor --db <path>` validates schema.

### Milestone 2: CLI read path

Deliverables:

1. CLI argument parser.
2. `doctor`
3. `sessions`
4. `logs`
5. `errors`
6. `traces`
7. `trace <trace-id>`
8. `--json` support.

Acceptance checks:

1. CLI can query a fixture SQLite DB.
2. JSON output is stable enough for tests.
3. Human output is readable and bounded.

### Milestone 3: Tauri plugin embedded collector

Deliverables:

1. Plugin builder.
2. Config handling.
3. Session DB creation at startup.
4. Discovery file writing and heartbeat.
5. Frontend command `export_otel_batch`.
6. Redaction before persistence.
7. Plugin permissions.

Acceptance checks:

1. Example app starts with plugin enabled.
2. Discovery file appears.
3. SQLite DB appears.
4. `auditaur apps` sees the running app.
5. `auditaur logs` can read frontend-exported logs.

### Milestone 4: Frontend API package

Deliverables:

1. `initAuditaur`.
2. Console instrumentation.
3. Error instrumentation.
4. Tauri `invoke` wrapper.
5. Tauri event wrapper.
6. OTEL-compatible span/log export batching.

Acceptance checks:

1. Example app console messages appear in SQLite.
2. Unhandled errors appear in `frontend_errors`.
3. Wrapped `invoke` calls create spans and IPC rows.
4. Failed `invoke` calls are marked failed and include redacted error summaries.

### Milestone 5: Rust tracing integration

Deliverables:

1. `tracing_subscriber::Layer` or equivalent exporter.
2. Capture Rust logs/events.
3. Capture spans where feasible.
4. Correlate trace/span IDs when present.

Acceptance checks:

1. `tracing::info!` appears in `logs`.
2. Instrumented spans appear in `spans`.
3. Failed spans are queryable through CLI.

### Milestone 6: MCP server

Deliverables:

1. stdio MCP server in `auditaur mcp`.
2. Tool list:
   - `list_sessions`
   - `list_apps`
   - `list_logs`
   - `list_errors`
   - `list_traces`
   - `get_trace`
   - `list_ipc_calls`
   - `list_events`
   - `list_windows`
   - `doctor`
3. Bounded responses.
4. Errors reported as MCP errors, not panics.

Acceptance checks:

1. MCP server starts without writing non-MCP content to stdout.
2. Tools work against a fixture DB.
3. Tool responses are redacted and bounded.

### Milestone 7: example app and dogfood

Deliverables:

1. Basic Tauri v2 example app.
2. Buttons to:
   - emit console logs
   - throw frontend error
   - call successful command
   - call failing command
   - emit/listen to event
3. README walkthrough.

Acceptance checks:

1. A developer can run the example and inspect telemetry with CLI.
2. A coding agent can connect through MCP and answer what failed.

### Milestone 8: public docs polish

Deliverables:

1. Complete Starlight docs for:
   - installation
   - quick start
   - Tauri plugin setup
   - frontend API setup
   - CLI reference
   - MCP tools reference
   - SQLite schema reference
   - security and redaction
2. Copy useful reference material from `docs/reference/` into user-facing docs pages.
3. Add screenshots or diagrams once the example app is available.
4. Verify the GitHub Pages deployment from `main`.
5. Verify `https://auditaur.dev` serves the docs site.
6. Verify `www.auditaur.dev` redirects or aliases to `https://auditaur.dev` if configured.

Acceptance checks:

1. `cd docs && npm run build` succeeds.
2. GitHub Pages deploys from the `publish-docs.yml` workflow.
3. The generated site includes the committed `CNAME`.
4. The docs clearly identify MVP, planned, and future functionality.

## Testing strategy

Rust:

1. Unit tests for redaction.
2. Unit tests for OTEL mapping.
3. SQLite migration tests.
4. SQLite query tests with fixtures.
5. CLI integration tests using temp DBs.
6. MCP protocol tests with stdio harness.

TypeScript:

1. Unit tests for payload summarization.
2. Unit tests for redaction hints.
3. Unit tests for console wrappers.
4. Unit tests for invoke wrapper behavior.
5. Exporter batching tests.

Docs:

1. `npm run build` from `docs/`.
2. Verify generated output includes `CNAME`.
3. Keep links valid for core docs routes.
4. Keep pages explicit about MVP versus planned behavior.

Example app:

1. Smoke test that app starts.
2. Smoke test that DB is created.
3. Smoke test that CLI can inspect the generated DB.

Cross-platform:

1. Windows, macOS, Linux CI.
2. Path tests for app-data directories.
3. Discovery file cleanup tests.

## Suggested dependencies

Rust CLI:

```toml
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
time = { version = "0.3", features = ["serde", "formatting", "parsing"] }
uuid = { version = "1", features = ["v4", "serde"] }
rusqlite = { version = "0.32", features = ["bundled"] }
directories = "5"
tracing = "0.1"
tracing-subscriber = "0.3"
opentelemetry = "0.30"
```

MCP:

Evaluate current Rust MCP SDKs before choosing. If a stable SDK is available, use it. Otherwise implement the minimal stdio JSON-RPC server with well-tested framing:

1. Read newline-delimited JSON-RPC messages from stdin.
2. Write only valid JSON-RPC/MCP messages to stdout.
3. Write diagnostics to stderr.
4. Support initialize, tools/list, and tools/call first.

TypeScript:

```json
{
  "@opentelemetry/api": "latest",
  "@opentelemetry/sdk-trace-base": "latest",
  "@opentelemetry/sdk-logs": "latest",
  "@tauri-apps/api": "latest"
}
```

Use pinned versions when implementing.

Docs site:

```json
{
  "@astrojs/starlight": "latest",
  "astro": "latest",
  "sharp": "latest"
}
```

Use pinned versions when implementing. The docs app should be buildable from `docs/` with `npm run build` and deployed by the GitHub Pages workflow.

## Open questions

1. Should `auditaur init` modify Tauri projects automatically, or only print instructions for v1?
2. Should full payload capture exist in v1, or be postponed?
3. How much of the official OTLP protobuf shape should be preserved in SQLite?
4. Which Rust MCP SDK is mature enough to use?
5. Can Tauri plugin lifecycle hooks reliably observe all windows/webviews across platforms?
6. Should screenshots be v1 or v2?
7. Should live actions like `open_devtools` require local IPC, or can they be implemented through database/discovery plus plugin command polling?

Recommended answers for MVP:

1. `auditaur init` should print instructions first; automatic modification can come later.
2. Full payload capture should be off by default and may be postponed.
3. Preserve OTEL semantics, not exact protobuf storage.
4. Choose the smallest reliable MCP path.
5. Capture best-effort window state and document limitations.
6. Screenshots are v2.
7. Live actions are v2; MVP is inspect-only.

## First implementation task for the next agent

Start with the storage and CLI read path before building the Tauri plugin.

Concrete first task:

1. Create the Rust workspace.
2. Implement `auditaur-core` storage models.
3. Implement SQLite migrations in `auditaur-collector`.
4. Implement `auditaur-cli doctor --db <path>`.
5. Scaffold the Astro/Starlight docs site under `docs/`.
6. Add `docs/public/CNAME` for `auditaur.dev`.
7. Add `.github/workflows/publish-docs.yml`.
8. Add tests that create a temp SQLite DB and validate the schema.
9. Verify `cd docs && npm run build`.

Why this first:

1. It establishes the data contract.
2. It lets all later pieces write/query the same format.
3. It avoids getting blocked on Tauri plugin details too early.
4. It gives MCP tools a stable query layer.
5. It gives users and agents a public, versioned place to learn the product while implementation proceeds.

## MVP definition of done

The MVP is complete when:

1. A Tauri v2 example app can enable `tauri-plugin-auditaur`.
2. The frontend can call `initAuditaur`.
3. Console logs, frontend errors, wrapped invokes, and at least basic Rust logs are persisted to SQLite.
4. `auditaur apps` discovers the running example app.
5. `auditaur logs`, `auditaur errors`, `auditaur ipc`, and `auditaur traces` return useful output.
6. `auditaur mcp` exposes those same inspection capabilities to an AI agent.
7. Redaction is enabled by default.
8. No network listener is opened by default.
9. The documentation site is deployed through GitHub Pages at `https://auditaur.dev`.
10. All core tests pass on Windows, macOS, and Linux.

