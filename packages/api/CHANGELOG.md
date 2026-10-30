# Changelog

## [0.14.0](https://github.com/sethjuarez/auditaur/compare/api-v0.13.0...api-v0.14.0) (2026-10-30)


### Features

* **api:** add a paste bridge action ([#172](https://github.com/sethjuarez/auditaur/issues/172)) ([edba321](https://github.com/sethjuarez/auditaur/commit/edba321c966528c3aca05327b7ff24a871578a36))

## [0.13.0](https://github.com/sethjuarez/auditaur/compare/api-v0.12.3...api-v0.13.0) (2026-10-30)


### Features

* **api:** add a dragTo bridge action for pointer and HTML5 drag-and-drop ([#167](https://github.com/sethjuarez/auditaur/issues/167)) ([4e129f3](https://github.com/sethjuarez/auditaur/commit/4e129f3f12c7976a3e10684fd5d58a84bb6813d4))

## [0.12.3](https://github.com/sethjuarez/auditaur/compare/api-v0.12.2...api-v0.12.3) (2026-10-30)


### Bug Fixes

* **drive:** select waits for options and matches labels, like Playwright ([#161](https://github.com/sethjuarez/auditaur/issues/161)) ([3228cda](https://github.com/sethjuarez/auditaur/commit/3228cdae7353e2726df13be89c3f9e7d648c583a))

## [0.12.2](https://github.com/sethjuarez/auditaur/compare/api-v0.12.1...api-v0.12.2) (2026-10-30)


### Bug Fixes

* **drive:** check and uncheck by clicking, then verify, like Playwright ([#159](https://github.com/sethjuarez/auditaur/issues/159)) ([3aa6c09](https://github.com/sethjuarez/auditaur/commit/3aa6c09d0c58c39bdf7df69499b6f6713165204e))
* **drive:** click and dblclick with Edge's full pointer sequence ([#158](https://github.com/sethjuarez/auditaur/issues/158)) ([ab8bf8e](https://github.com/sethjuarez/auditaur/commit/ab8bf8e1c131f60cf47a9b8016c67463501a80a4))

## [0.12.1](https://github.com/sethjuarez/auditaur/compare/api-v0.12.0...api-v0.12.1) (2026-10-30)


### Bug Fixes

* **drive:** wait for enabled and editable targets like Playwright ([#154](https://github.com/sethjuarez/auditaur/issues/154)) ([07e9a7b](https://github.com/sethjuarez/auditaur/commit/07e9a7b243445e7228af2e051a8be99ff798c821))

## [0.12.0](https://github.com/sethjuarez/auditaur/compare/api-v0.11.1...api-v0.12.0) (2026-10-30)


### Features

* **drive:** right and middle click with click's button option (AR-27) ([#150](https://github.com/sethjuarez/auditaur/issues/150)) ([3e78612](https://github.com/sethjuarez/auditaur/commit/3e786121865bab3e4f2147f7a2714c466af2e06b))

## [0.11.1](https://github.com/sethjuarez/auditaur/compare/api-v0.11.0...api-v0.11.1) (2026-10-30)


### Bug Fixes

* **api:** fill replaces contenteditable text like Playwright and fails loudly (AR-26) ([#141](https://github.com/sethjuarez/auditaur/issues/141)) ([40ffbdf](https://github.com/sethjuarez/auditaur/commit/40ffbdf51d271b6505563324fc7f41427a67289a))
* **api:** press sends real key definitions and default actions (AR-25) ([#140](https://github.com/sethjuarez/auditaur/issues/140)) ([20be952](https://github.com/sethjuarez/auditaur/commit/20be952dd70b6f670a23b03fe15e2f8c6d024ea0))

## [0.11.0](https://github.com/sethjuarez/auditaur/compare/api-v0.10.1...api-v0.11.0) (2026-10-30)


### Features

* **api:** let drills answer native dialogs (AR-24) ([#136](https://github.com/sethjuarez/auditaur/issues/136)) ([9fb1f36](https://github.com/sethjuarez/auditaur/commit/9fb1f36ffdcf8969e5362957bfdd6744acd772af))

## [0.10.1](https://github.com/sethjuarez/auditaur/compare/api-v0.10.0...api-v0.10.1) (2026-10-30)


### Bug Fixes

* **api:** withhold a console error whose text redactConsole removed ([#128](https://github.com/sethjuarez/auditaur/issues/128)) ([bce28a2](https://github.com/sethjuarez/auditaur/commit/bce28a26f8c6dde27921d19ceb740d73ed713cc3))

## [0.10.0](https://github.com/sethjuarez/auditaur/compare/api-v0.9.1...api-v0.10.0) (2026-10-30)


### Features

* **api:** redact console bodies and uncaught errors ([#121](https://github.com/sethjuarez/auditaur/issues/121)) ([85a25c2](https://github.com/sethjuarez/auditaur/commit/85a25c218558be448e1b91f5af2f80f423c68c05))

## [0.9.1](https://github.com/sethjuarez/auditaur/compare/api-v0.9.0...api-v0.9.1) (2026-10-30)


### Bug Fixes

* **api:** import under plain Node with .js relative specifiers ([#117](https://github.com/sethjuarez/auditaur/issues/117)) ([e980990](https://github.com/sethjuarez/auditaur/commit/e980990741ca510f7eb085566fca687169054bf1))

## [0.9.0](https://github.com/sethjuarez/auditaur/compare/api-v0.8.0...api-v0.9.0) (2026-10-30)


### Features

* **api:** redact invoke/emit error messages; export redaction test helpers ([#113](https://github.com/sethjuarez/auditaur/issues/113)) ([f728cdd](https://github.com/sethjuarez/auditaur/commit/f728cdd2f63bead0bc78c2ef160cf7e8c597ab3d))

## [0.8.0](https://github.com/sethjuarez/auditaur/compare/api-v0.7.1...api-v0.8.0) (2026-10-30)


### Features

* **api:** redact payload summaries; add redactKeys, payloadSummaries, redactPayload ([#102](https://github.com/sethjuarez/auditaur/issues/102)) ([6cd9cd0](https://github.com/sethjuarez/auditaur/commit/6cd9cd027028b9e4982cd9d96a7792b3f6df55ce))

## [0.7.1](https://github.com/sethjuarez/auditaur/compare/api-v0.7.0...api-v0.7.1) (2026-10-29)


### Bug Fixes

* **drills:** api 0.7.1 bridge fixes and CutReady follow-ups (AR-7/8/9) ([#89](https://github.com/sethjuarez/auditaur/issues/89)) ([e56582f](https://github.com/sethjuarez/auditaur/commit/e56582f6fe840d9a26b09e6df338a3a892292df6))

## [0.7.0](https://github.com/sethjuarez/auditaur/compare/api-v0.6.1...api-v0.7.0) (2026-10-29)


### Features

* **drills:** unit-test-style drills, a routed drills canvas, and the Auditaur Copilot plugin ([#82](https://github.com/sethjuarez/auditaur/issues/82)) ([4df1708](https://github.com/sethjuarez/auditaur/commit/4df1708b61aa14e4478d3df67d500bb1726e64e8))

## [0.6.1](https://github.com/sethjuarez/auditaur/compare/api-v0.6.0...api-v0.6.1) (2026-10-23)


### Bug Fixes

* dispatch drive bridge requests exactly once; build against Tauri 2.12 ([#77](https://github.com/sethjuarez/auditaur/issues/77)) ([f610cc5](https://github.com/sethjuarez/auditaur/commit/f610cc5af26c21ed29b362361d0025e8aa580ca3))

## [0.6.0](https://github.com/sethjuarez/auditaur/compare/api-v0.5.1...api-v0.6.0) (2026-07-03)


### Features

* add auditaur drill run ([50b004b](https://github.com/sethjuarez/auditaur/commit/50b004b7a73df21f07ab7ea668ae74dd4e5b6509))
* add auditaur drill run ([23c63f4](https://github.com/sethjuarez/auditaur/commit/23c63f4b9e88fbe1c20d21429901cbeb24988034))
* add targeted native webview screenshots ([5a032bc](https://github.com/sethjuarez/auditaur/commit/5a032bc40468c10bb582ba5a48b84c374a75f18d))
* add targeted native webview screenshots ([a8d596c](https://github.com/sethjuarez/auditaur/commit/a8d596cef2dc64e8f05f64f747f3316b72905ca3))


### Bug Fixes

* scroll screenshot targets into view ([e8878fa](https://github.com/sethjuarez/auditaur/commit/e8878fa544920fe7eac9cc85928847b648cd42df))

## [0.5.1](https://github.com/sethjuarez/auditaur/compare/api-v0.5.0...api-v0.5.1) (2026-06-26)


### Bug Fixes

* polish native drive bridge release readiness ([10728a4](https://github.com/sethjuarez/auditaur/commit/10728a477dc107dd498f17efe1fdaf483ccb00a0))
* polish native drive bridge release readiness ([aaeb7a7](https://github.com/sethjuarez/auditaur/commit/aaeb7a7f1343890ed446624362b289b918133d32))

## [0.5.0](https://github.com/sethjuarez/auditaur/compare/api-v0.4.0...api-v0.5.0) (2026-06-26)


### Features

* add native Tauri drive bridge ([bbff788](https://github.com/sethjuarez/auditaur/commit/bbff78817595b2fb808f51ad8f09566c4e52133d))
* add native Tauri drive bridge ([b8e1c2f](https://github.com/sethjuarez/auditaur/commit/b8e1c2f1224ff8407e239ba1b9045cd7ddfc1c68))

## [0.4.0](https://github.com/sethjuarez/auditaur/compare/api-v0.3.0...api-v0.4.0) (2026-06-18)


### Features

* add in-app drive bridge ([31e6e23](https://github.com/sethjuarez/auditaur/commit/31e6e2380f1bfe65b981133c4bf13e8ff609f5d1)), closes [#29](https://github.com/sethjuarez/auditaur/issues/29)

## [0.3.0](https://github.com/sethjuarez/auditaur/compare/api-v0.2.1...api-v0.3.0) (2026-06-09)


### Features

* **api:** improve Tauri IPC trace propagation ([cb4362c](https://github.com/sethjuarez/auditaur/commit/cb4362ce68d4f15fa2871a43e53c017e6ebb76e6))

## [0.2.1](https://github.com/sethjuarez/auditaur/compare/api-v0.2.0...api-v0.2.1) (2026-06-08)


### Bug Fixes

* lock Linux API publish dependencies ([d7884ae](https://github.com/sethjuarez/auditaur/commit/d7884ae1c9a72e33f3385ce37d85334cc38c505b))

## [0.2.0](https://github.com/sethjuarez/auditaur/compare/api-v0.1.0...api-v0.2.0) (2026-06-08)


### Features

* improve Auditaur integration ergonomics ([750974f](https://github.com/sethjuarez/auditaur/commit/750974f7f447832e1db6bbcabfb07516aa95f7b0))
