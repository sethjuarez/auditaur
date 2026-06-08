# auditaur-cli

Command-line interface and MCP server for inspecting Auditaur local telemetry.

The published binary is named `auditaur`.

## Install

```powershell
cargo install auditaur-cli
```

## Common commands

```powershell
auditaur doctor
auditaur apps --json
auditaur logs --json
auditaur traces --json
auditaur trace <trace-id> --json
auditaur windows --json
auditaur timeline --json
auditaur explain
auditaur mcp
```

Auditaur discovers active local Tauri app sessions through per-app discovery files and reads telemetry from the associated SQLite session database. Pass `--db <path>` when you want to inspect a specific database.

## MCP

Run `auditaur mcp` to expose read-only telemetry tools over stdio for MCP clients and coding agents.
