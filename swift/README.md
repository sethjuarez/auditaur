# Auditaur Apple Swift package

This Swift Package Manager package provides a small, pure Swift diagnostics bridge for Apple apps. It is intentionally not tied to a specific consuming app.

## Add the dependency

```swift
.package(url: "https://github.com/sethjuarez/auditaur.git", branch: "main")
```

Then add the product:

```swift
.product(name: "AuditaurAppleCore", package: "auditaur")
```

The root package manifest points at the sources under `swift/` so remote SwiftPM consumers can use the Auditaur repository URL directly. For local Auditaur development, run package commands from this directory or use `swift package --package-path swift`.

## Emit diagnostics

```swift
import AuditaurAppleCore

let exporter = InMemoryAuditaurExporter()
let auditaur = AuditaurDiagnostics(
    serviceName: "cutready-ios",
    sessionId: "current-session-id",
    exporter: exporter
)

try await auditaur.recordEvent(name: "cutready.auth.complete")
try await auditaur.recordEvent(name: "cutready.sketch.edit", attributes: ["tool": "pencil"])

let span = auditaur.startSpan(name: "cutready.sync.push")
do {
    try await pushSketches()
    try await auditaur.endSpan(span, status: .ok)
} catch {
    try await auditaur.endSpan(span, status: .error, statusMessage: error.localizedDescription)
    try await auditaur.capture(error: error, name: "cutready.sync.push.error")
}

try await auditaur.recordEvent(name: "cutready.agentive.rewrite")
```

For Simulator observation, use `FileAuditaurExporter(directory:)` to write JSON batches into an app-controlled directory, then pass that directory to `auditaur apple observe --diagnostics <path>` after the run.
