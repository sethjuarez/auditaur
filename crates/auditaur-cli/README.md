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
auditaur agent guide --json
auditaur observe --app my-tauri-app -- npm run tauri dev
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
auditaur diagnose --session-file .auditaur\session.json
auditaur tail --session-file .auditaur\session.json --signal failures --replay
auditaur init skill
auditaur init diagnostics
auditaur mcp
```

Auditaur discovers active local Tauri app sessions through per-app discovery files and reads telemetry from the associated SQLite session database. Pass `--db <path>` when you want to inspect a specific database, or prefer `--session-file .auditaur\session.json` after `observe`/`start` has pinned a run.

Use `auditaur observe --app <name> -- <dev command>` for no-config Tauri/dev-app observation. It starts the normal command as argv, waits for core readiness, writes `.auditaur\session.json` by default, prints pinned selectors for follow-up commands, and leaves the observed app running until `auditaur stop --session-file .auditaur\session.json`. Read commands such as `logs`, `ipc`, `timeline`, `explain`, `diagnose`, and `tail --signal failures` can consume the same session file, including after the app exits.

For concurrent agent runs, use named ports: `auditaur observe --app <name> --port web -- npm run dev -- --port {{port:web}}` or export one with `--port-env web=VITE_PORT`. Chosen ports are recorded in the session file.

## MCP

Run `auditaur mcp` to expose read-only telemetry tools over stdio for MCP clients and coding agents.

Run `auditaur agent guide` for the concise agent workflow, or `auditaur agent guide --json` for a machine-readable guide.
