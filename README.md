# Auditaur

Runtime observability for Tauri apps and AI agents.

Auditaur is a local-first development toolkit for inspecting Tauri app logs, traces, frontend errors, IPC calls, and events through a CLI and MCP server. It is development-first and writes telemetry to a local SQLite database discovered from per-app heartbeat files.

Docs are configured for GitHub Pages at `auditaur.dev`; the domain resolves once DNS points at GitHub Pages.

## Copy/paste quick start

Build the CLI and run the dogfood app:

```powershell
cd D:\projects\auditaur
cargo build -p auditaur-cli

cd examples\dogfood
npm install
npm run tauri dev
```

Click every dogfood button, then inspect telemetry from another shell:

```powershell
cd D:\projects\auditaur
$env:AUDITAUR_DATA_DIR = "$env:LOCALAPPDATA\auditaur"
cargo run -p auditaur-cli -- apps --json
cargo run -p auditaur-cli -- logs --json
cargo run -p auditaur-cli -- errors --json
cargo run -p auditaur-cli -- ipc --json
cargo run -p auditaur-cli -- events --json
cargo run -p auditaur-cli -- traces --json
```

If more than one app session is active, copy `databasePath` from `apps --json` and pass it explicitly:

```powershell
cargo run -p auditaur-cli -- trace <traceId> --db "<databasePath>" --json
```

Run the MCP server:

```powershell
cargo run -p auditaur-cli -- mcp
```

MCP clients should point at the built binary or the cargo command above. The tools include `list_apps`, `list_logs`, `list_errors`, `list_ipc_calls`, `list_events`, `list_traces`, `get_trace`, and `list_windows`.

## What works today

- SQLite session store with WAL and schema validation.
- Discovery files under the local Auditaur data directory.
- CLI reads for apps, sessions, logs, errors, IPC, events, traces, trace detail, and stored window rows.
- MCP reads over stdio for the same data.
- Tauri plugin collector for frontend batches and Rust `tracing`.
- Frontend console/error/invoke/event wrappers.
- OpenTelemetry JS span exporter shim for existing tracer providers.
- Recursive redaction and best-effort retention.

Planned: OTLP receiver/sidecar, automatic runtime window capture, screenshots/devtools actions, metrics ingestion, and OpenTelemetry logs SDK support.

## Local package status

Auditaur is not published yet. First usage should be local path dependencies or git dependencies from this repository:

- Rust crates: `auditaur-core`, `auditaur-collector`, `auditaur-cli`, `tauri-plugin-auditaur`.
- CLI binary: `auditaur` from `crates/auditaur-cli`.
- Frontend package: `@auditaur/api`.

See the docs in `docs\src\content\docs` for installation, MCP, and CutReady integration details.
