# auditaur-collector

Local SQLite telemetry collector and query store for Auditaur.

This crate implements Auditaur's local session database writes and reads. It is used by `tauri-plugin-auditaur` to persist telemetry and by `auditaur-cli` to inspect sessions.

## What it stores

- OpenTelemetry-shaped spans and logs.
- Frontend errors.
- Tauri IPC call records.
- Tauri event records.
- Tauri window state and lifecycle rows.
- Session and resource metadata.

## Usage

Most consumers should use `tauri-plugin-auditaur` in a Tauri app or `auditaur-cli` for inspection rather than using this crate directly. Use this crate directly when building custom local collectors or query tools on top of Auditaur's SQLite session format.
