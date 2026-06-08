# auditaur-core

Shared models, configuration, redaction, and local discovery types for Auditaur.

Most applications should not depend on this crate directly. It is primarily used by the Auditaur collector, CLI, and Tauri plugin crates to share telemetry record shapes and configuration behavior.

## What it contains

- OpenTelemetry-shaped record models for spans, logs, frontend errors, Tauri IPC calls, Tauri events, and Tauri window state.
- Auditaur configuration defaults.
- Redaction helpers for sensitive keys and payload fields.
- Discovery and health data structures for local session databases.

## Related crates

- `auditaur-collector` stores and queries local SQLite telemetry.
- `auditaur-cli` reads Auditaur sessions from the command line and MCP.
- `tauri-plugin-auditaur` embeds the collector in a Tauri app.
