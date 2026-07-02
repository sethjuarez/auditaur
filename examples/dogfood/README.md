# Auditaur Dogfood Example

This is a small Tauri v2 app for manually generating telemetry through the Auditaur plugin, frontend SDK, CLI, and MCP server.

## Run it

```powershell
cd C:\path\to\auditaur\examples\dogfood
npm install
npm run build:api
npm run tauri dev
```

Click the buttons to emit a console log, throw a frontend error, call successful and failing commands, and emit/listen to events. The app flushes after each button click and on page hide so records are written before you inspect the database. The dogfood frontend also enables the debug-only Auditaur drive bridge in its single main WebView so macOS/WKWebView selector actions can be smoke-tested through the Tauri-native bridge.

## Smoke test it

From the repository root on Windows, run the repeatable smoke pass:

```powershell
.\scripts\dogfood-smoke.ps1
```

The script builds the CLI and dogfood web bundle, launches the dogfood Tauri app with an isolated `AUDITAUR_DATA_DIR`, waits for `auditaur debug` readiness, drives the app through the Tauri-native bridge, verifies native window screenshot capture, checks frontend-required readiness, then reads `timeline` and `explain`.

The same drive commands work on macOS/WKWebView because they use the Tauri-native in-app bridge:

```powershell
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json inspect
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json exists --selector '#successful-command'
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json click --selector '#successful-command'
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json fill --selector '#drive-input' --value 'filled by bridge'
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json type --selector '#drive-textarea' --value ' typed by bridge'
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json press --selector '#drive-input' --key Enter
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json snapshot --selector body --output dogfood-snapshot.json
cargo run -p auditaur-cli -- drive --app auditaur-dogfood --active --json screenshot --selector body --output dogfood-bridge.png --snapshot-output dogfood-bridge.json
```

## Find the session database

Auditaur writes a discovery file while the app is running. The CLI and MCP server use this automatically when exactly one active readable session is present. You can also open the latest JSON file and copy `databasePath`.

| OS | Discovery directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\auditaur\apps` |
| macOS | `~/Library/Application Support/auditaur/apps` |
| Linux | `~/.local/share/auditaur/apps` |

## Inspect with the CLI

```powershell
cargo run -p auditaur-cli -- sessions --db "<databasePath>" --json
cargo run -p auditaur-cli -- logs --db "<databasePath>" --json
cargo run -p auditaur-cli -- errors --db "<databasePath>" --json
cargo run -p auditaur-cli -- traces --db "<databasePath>" --json
cargo run -p auditaur-cli -- trace --db "<databasePath>" "<traceId>" --json
cargo run -p auditaur-cli -- ipc --db "<databasePath>" --json
cargo run -p auditaur-cli -- events --db "<databasePath>" --json
```

With discovery:

```powershell
cargo run -p auditaur-cli -- apps --json
cargo run -p auditaur-cli -- logs --json
cargo run -p auditaur-cli -- ipc --json
cargo run -p auditaur-cli -- events --json
```

The failing-command button should produce a frontend `tauri.invoke failing_command` span with `ERROR` status and backend tracing records containing `Intentional dogfood backend failure`.

## Inspect through MCP

Start the MCP server with:

```powershell
cargo run -p auditaur-cli -- mcp
```

Then call tools such as `get_health`, `list_apps`, `list_errors`, `list_traces`, `list_ipc_calls`, `list_events`, `get_trace`, `get_related_telemetry`, and `explain_failed_ipc`. An agent should be able to answer what failed by reading the `failing_command` frontend IPC/span records and the backend error log/span in the same SQLite session.
