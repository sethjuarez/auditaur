# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.33](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.32...tauri-plugin-auditaur-v0.4.33) - 2026-10-31

### Fixed

- *(drill)* hand the foreground back after a gate ([#187](https://github.com/sethjuarez/auditaur/pull/187))

## [0.4.32](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.31...tauri-plugin-auditaur-v0.4.32) - 2026-10-31

### Added

- *(drill)* keep drilled app windows in the background ([#185](https://github.com/sethjuarez/auditaur/pull/185))
### Changed

- Windows: while the app starts, hands the foreground back to the user's previous window when `AUDITAUR_DRILL_WINDOW=background` (the `drill test` default with CLI 0.4.32). Advertises the `window_background` capability and answers the native `auditaur.window` bridge action. macOS and Linux report background mode as unsupported for now.

## [0.4.28](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.27...tauri-plugin-auditaur-v0.4.28) - 2026-10-30

### Fixed

- *(plugin)* record command spans when they start so coverage counts long-running commands ([#165](https://github.com/sethjuarez/auditaur/pull/165))

## [0.4.24](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.23...tauri-plugin-auditaur-v0.4.24) - 2026-10-30

### Fixed

- *(drive)* keep poll_drive_bridge_request's public signature (AR-29) ([#148](https://github.com/sethjuarez/auditaur/pull/148))
- *(drive)* emulate page focus so blur fires in background windows (AR-29) ([#146](https://github.com/sethjuarez/auditaur/pull/146))

## [0.4.14](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.13...tauri-plugin-auditaur-v0.4.14) - 2026-10-30

### Fixed

- *(plugin)* set command span status from the command's result ([#98](https://github.com/sethjuarez/auditaur/pull/98))

## [0.4.8](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.7...tauri-plugin-auditaur-v0.4.8) - 2026-10-23

### Fixed

- dispatch drive bridge requests exactly once; build against Tauri 2.12 ([#77](https://github.com/sethjuarez/auditaur/pull/77))

## [0.4.1](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.4.0...tauri-plugin-auditaur-v0.4.1) - 2026-07-03

### Other

- improve user documentation

## [0.4.0](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.3.2...tauri-plugin-auditaur-v0.4.0) - 2026-06-28

### Added

- add targeted native webview screenshots

## [0.3.0](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.2.5...tauri-plugin-auditaur-v0.3.0) - 2026-06-25

### Added

- add native Tauri drive bridge

### Other

- refresh current package and tool docs

## [0.2.5](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.2.4...tauri-plugin-auditaur-v0.2.5) - 2026-06-18

### Added

- add in-app drive bridge

### Other

- fix Windows stale bridge temp file test

## [0.2.0](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.1.4...tauri-plugin-auditaur-v0.2.0) - 2026-06-11

### Added

- *(cli)* add agent run observability pivots

## [0.1.4](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.1.3...tauri-plugin-auditaur-v0.1.4) - 2026-06-09

### Other

- Improve Tauri IPC trace ergonomics

## [0.1.2](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.1.1...tauri-plugin-auditaur-v0.1.2) - 2026-06-08

### Added

- improve Auditaur integration ergonomics

### Other

- Bridge Tauri IPC trace context
- Add runtime cockpit telemetry tools

## [0.1.1](https://github.com/sethjuarez/auditaur/compare/tauri-plugin-auditaur-v0.1.0...tauri-plugin-auditaur-v0.1.1) - 2026-06-06

### Added

- add CLI polish workflows
