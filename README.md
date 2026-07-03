# Auditaur

Runtime observability for Tauri apps and AI agents.

Auditaur is a local-first development toolkit for inspecting Tauri app logs, traces, frontend errors, IPC calls, and events through a CLI and MCP server. It is development-first and writes telemetry to a local SQLite database discovered from per-app heartbeat files.

Docs are configured for GitHub Pages at `auditaur.dev`; the domain resolves once DNS points at GitHub Pages.

## Copy/paste quick start

Build the CLI and run the dogfood app:

```powershell
cd C:\path\to\auditaur
cargo build -p auditaur-cli

cd examples\dogfood
npm install
npm run build:api
npm run tauri dev
```

Click every dogfood button, then inspect telemetry from another shell:

```powershell
cd C:\path\to\auditaur
$env:AUDITAUR_DATA_DIR = "$env:LOCALAPPDATA\auditaur"
cargo run -p auditaur-cli -- apps --json
cargo run -p auditaur-cli -- logs --json
cargo run -p auditaur-cli -- errors --json
cargo run -p auditaur-cli -- ipc --json
cargo run -p auditaur-cli -- events --json
cargo run -p auditaur-cli -- traces --json
```

For a repeatable local dogfood pass on Windows, run:

```powershell
.\scripts\dogfood-smoke.ps1
```

If more than one app session is active, copy `databasePath` from `apps --json` and pass it explicitly:

```powershell
cargo run -p auditaur-cli -- trace <traceId> --db "<databasePath>" --json
```

Run the MCP server:

```powershell
cargo run -p auditaur-cli -- mcp
```

MCP clients should point at the built binary or the cargo command above. The tools include `doctor`, `get_health`, `list_apps`, `list_sessions`, `list_logs`, `list_errors`, `list_ipc_calls`, `list_events`, `list_traces`, `get_trace`, `get_related_telemetry`, `explain_recent_activity`, `explain_failed_ipc`, and `list_windows`.

## What works today

- SQLite session store with WAL and schema validation.
- Discovery files under the local Auditaur data directory.
- CLI reads for apps, health, sessions, logs, errors, IPC, events, traces, trace detail, related telemetry, stored window rows, timeline, explain, tail, and redacted bundles.
- MCP reads and agent summaries over stdio for the same data.
- Tauri plugin collector for frontend batches, window startup/lifecycle state, Rust `tracing`, and Rust panic diagnostics.
- Frontend console/error/invoke/event wrappers.
- OpenTelemetry JS span exporter shim for existing tracer providers.
- Recursive redaction and best-effort retention.

Planned: OTLP receiver/sidecar, browser devtools actions, metrics ingestion, OpenTelemetry logs SDK support, and a lightweight local dashboard.

## Package status

Auditaur packages are published publicly:

- Rust crates: `auditaur-core`, `auditaur-collector`, `auditaur-cli`, `tauri-plugin-auditaur`.
- CLI crate: install the `auditaur` command with `cargo install auditaur-cli`.
- Frontend package: install with `npm install @auditaur/api`.
- Agent skill: scaffold into a consuming repo with `auditaur init skill`; use `auditaur init skill --agents-path` for `.agents/skills` consumers. After the skill is available on GitHub, install with `gh skill install sethjuarez/auditaur auditaur-debug`. Maintainers can validate publishing with `gh skill publish .github --dry-run`.
- Copilot canvas extension: scaffold the Auditaur manual-gate card into a consuming repo with `auditaur init extension`.

See the docs in `docs\src\content\docs` for installation, MCP, and Tauri integration details.

Before releasing, run:

```powershell
.\scripts\preflight-release.ps1
```
