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
auditaur agent-runs --app cutready --json
auditaur agent-run <run-id> --json
auditaur related --run-id <run-id> --json
auditaur apple observe --destination "platform=iOS Simulator,name=iPhone 16" --report report/apple-observe.json
auditaur apple screenshot --destination "platform=iOS Simulator,name=iPhone 16" --output report/launch.png
auditaur windows --json
auditaur timeline --json
auditaur explain
auditaur init skill
auditaur mcp
```

Auditaur discovers active local Tauri app sessions through per-app discovery files and reads telemetry from the associated SQLite session database. Pass `--db <path>` when you want to inspect a specific database.

## MCP

Run `auditaur mcp` to expose read-only telemetry tools over stdio for MCP clients and coding agents.
