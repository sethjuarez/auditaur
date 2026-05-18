# Auditaur Dogfood Example

This is a small Tauri v2 app for manually generating telemetry through the Auditaur plugin, frontend SDK, CLI, and MCP server.

## Run it

```powershell
cd D:\projects\auditaur\examples\dogfood
npm install
npm run build:api
npm run tauri dev
```

Click the buttons to emit a console log, throw a frontend error, call successful and failing commands, and emit/listen to events. The app flushes after each button click and on page hide so records are written before you inspect the database.

## Find the session database

Auditaur writes a discovery file while the app is running. Open the latest JSON file and copy `databasePath`.

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
```

The failing-command button should produce a frontend `tauri.invoke failing_command` span with `ERROR` status and backend tracing records containing `Intentional dogfood backend failure`.

## Inspect through MCP

Start the MCP server with:

```powershell
cargo run -p auditaur-cli -- mcp
```

Then call tools with the copied `databasePath`, for example `list_errors`, `list_traces`, and `get_trace`. An agent should be able to answer what failed by reading the `failing_command` frontend span and the backend error log/span in the same SQLite session.
