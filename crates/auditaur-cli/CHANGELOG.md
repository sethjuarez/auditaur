# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.19](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.18...auditaur-cli-v0.4.19) - 2026-10-30

### Fixed

- *(drills)* print run paths with one separator on Windows ([#130](https://github.com/sethjuarez/auditaur/pull/130))

## [0.4.18](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.17...auditaur-cli-v0.4.18) - 2026-10-30

### Added

- *(canvas)* ship a version stamp in every extension copy; get_coverage takes runId ([#126](https://github.com/sethjuarez/auditaur/pull/126))

### Fixed

- *(canvas)* count gate time down to the runner's own deadline ([#125](https://github.com/sethjuarez/auditaur/pull/125))

## [0.4.17](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.16...auditaur-cli-v0.4.17) - 2026-10-30

### Added

- *(drill)* show launch progress and explain slow-build readiness timeouts ([#123](https://github.com/sethjuarez/auditaur/pull/123))

## [0.4.16](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.15...auditaur-cli-v0.4.16) - 2026-10-30

### Added

- *(drills)* name the canvas version and its upgrade fix ([#119](https://github.com/sethjuarez/auditaur/pull/119))
- *(drills)* count backend command spans in coverage ([#116](https://github.com/sethjuarez/auditaur/pull/116))

### Fixed

- *(drills)* stop coverage claiming 100% with nothing to compare against ([#115](https://github.com/sethjuarez/auditaur/pull/115))
- *(drills)* point a step's expectTimeoutMs at timeoutMs ([#112](https://github.com/sethjuarez/auditaur/pull/112))

## [0.4.15](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.14...auditaur-cli-v0.4.15) - 2026-10-30

### Added

- *(drills)* live gate countdown and a "waiting for you" banner on Tests ([#106](https://github.com/sethjuarez/auditaur/pull/106))

### Fixed

- *(drills)* stop the Snapshots page vouching for snapshots it never saw ([#110](https://github.com/sethjuarez/auditaur/pull/110))
- *(drills)* say why a hook failed, in its own output ([#109](https://github.com/sethjuarez/auditaur/pull/109))
- *(drills)* number step artifacts the way reports do ([#108](https://github.com/sethjuarez/auditaur/pull/108))
- *(cli)* wait for a stopped Windows app tree to finish tearing down ([#107](https://github.com/sethjuarez/auditaur/pull/107))
- *(drills)* mark skipped tests in the canvas; name suites in "Last run" ([#104](https://github.com/sethjuarez/auditaur/pull/104))

### Other

- *(cli)* give the slow-test timeout check room on loaded CI runners ([#105](https://github.com/sethjuarez/auditaur/pull/105))

## [0.4.14](https://github.com/sethjuarez/auditaur/compare/auditaur-cli-v0.4.13...auditaur-cli-v0.4.14) - 2026-10-30

### Fixed

- *(drills)* fail fast when the app dies or restarts mid-run ([#100](https://github.com/sethjuarez/auditaur/pull/100))
- *(drills)* report the last observed value when expectValue fails ([#99](https://github.com/sethjuarez/auditaur/pull/99))
- *(plugin)* set command span status from the command's result ([#98](https://github.com/sethjuarez/auditaur/pull/98))

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
