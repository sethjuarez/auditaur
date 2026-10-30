# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.13](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.12...auditaur-cli-v0.4.13) - 2026-10-30

### Added

- *(drills)* save app output, aria snapshot, and an evidence manifest on failure ([#95](https://github.com/sethjuarez/auditaur/pull/95))

### Fixed

- *(drills)* wait for the whole app process tree to exit before the next launch ([#96](https://github.com/sethjuarez/auditaur/pull/96))

## [0.4.12](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.11...auditaur-cli-v0.4.12) - 2026-10-29

### Fixed

- *(drills)* api 0.7.1 bridge fixes and CutReady follow-ups (AR-7/8/9) ([#89](https://github.com/sethjuarez/auditaur/pull/89))

## [0.4.11](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.10...auditaur-cli-v0.4.11) - 2026-10-29

### Added

- *(drills)* CutReady drill requests (run step, suite env, value/telemetry assertions) ([#87](https://github.com/sethjuarez/auditaur/pull/87))

## [0.4.10](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.9...auditaur-cli-v0.4.10) - 2026-10-29

### Fixed

- *(drills)* stop finished runs reading as interrupted ([#84](https://github.com/sethjuarez/auditaur/pull/84))

## [0.4.9](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.8...auditaur-cli-v0.4.9) - 2026-10-29

### Added

- *(drills)* unit-test-style drills, a routed drills canvas, and the Auditaur Copilot plugin ([#82](https://github.com/sethjuarez/auditaur/pull/82))

## [0.4.5](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.4...auditaur-cli-v0.4.5) - 2026-08-12

### Fixed

- *(cli)* align diagnostics privacy starter config

## [0.4.4](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.3...auditaur-cli-v0.4.4) - 2026-08-12

### Fixed

- *(cli)* polish generic diagnostics follow-ups ([#66](https://github.com/sethjuarez/auditaur/pull/66))

## [0.4.3](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.2...auditaur-cli-v0.4.3) - 2026-08-12

### Added

- *(cli)* add generic diagnostics workflow

## [0.4.2](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.1...auditaur-cli-v0.4.2) - 2026-08-12

### Added

- *(cli)* add no-config tauri observation workflow
- add Apple diagnostics observe bridge [skip ci]

## [0.4.1](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.0...auditaur-cli-v0.4.1) - 2026-07-03

### Added

- add human gate drill support
- add agent startup workflow

### Fixed

- restore unix debug run build

## [0.4.0](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.3.2...auditaur-cli-v0.4.0) - 2026-06-28

### Added

- add drill script hooks
- add auditaur drill run
- add targeted native webview screenshots

### Fixed

- clean up drill process groups on macOS

## [0.3.2](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.3.1...auditaur-cli-v0.3.2) - 2026-06-26

### Fixed

- drive through the Tauri-native bridge

## [0.3.1](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.3.0...auditaur-cli-v0.3.1) - 2026-06-26

### Fixed

- polish native drive bridge release readiness

## [0.3.0](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.2.5...auditaur-cli-v0.3.0) - 2026-06-25

### Added

- add native Tauri drive bridge

## [0.2.5](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.2.4...auditaur-cli-v0.2.5) - 2026-06-18

### Added

- add in-app drive bridge

### Fixed

- avoid Windows CLI stack overflow

### Other

- bound fake CDP server waits
- test Rust workspace across OSes

## [0.2.4](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.2.3...auditaur-cli-v0.2.4) - 2026-06-17

### Fixed

- *(drive)* clarify macos wkwebview drive support

## [0.2.3](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.2.2...auditaur-cli-v0.2.3) - 2026-06-13

### Other

- Polish Auditaur drive input helpers

## [0.2.2](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.2.1...auditaur-cli-v0.2.2) - 2026-06-12

### Other

- Document Auditaur skill startup modes
- Harden Auditaur debug workflows
- Add Auditaur debug skill scaffolding
- Isolate debug run JSON child output
- Add debug readiness commands
- Add opt-in screenshot failure artifacts
- Improve session churn diagnostics
- Document window lifecycle coverage
- Add dogfood telemetry smoke coverage

## [0.2.1](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.2.0...auditaur-cli-v0.2.1) - 2026-06-11

### Added

- *(cli)* add driver actions
- *(cli)* bind driver targets to observed windows
- *(cli)* add CDP driver inspect and wait
- *(cli)* add app driver attach diagnostics

### Fixed

- *(cli)* require opt-in for unproven drive mutations
- *(cli)* gate probable drive mutations
- *(cli)* read CDP probe bodies by content length
- *(cli)* improve drive dogfood ergonomics

## [0.2.0](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.1.4...auditaur-cli-v0.2.0) - 2026-06-11

### Added

- *(cli)* add agent run observability pivots

## [0.1.4](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.1.3...auditaur-cli-v0.1.4) - 2026-06-09

### Other

- Improve Tauri IPC trace ergonomics

## [0.1.3](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.1.2...auditaur-cli-v0.1.3) - 2026-06-08

### Added

- add exception triage and churn diagnostics

## [0.1.2](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.1.1...auditaur-cli-v0.1.2) - 2026-06-08

### Added

- improve Auditaur integration ergonomics

### Other

- Add runtime cockpit telemetry tools

## [0.1.1](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.1.0...auditaur-cli-v0.1.1) - 2026-06-06

### Added

- add CLI polish workflows
