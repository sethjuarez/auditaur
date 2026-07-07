import XCTest
@testable import AuditaurAppleCore

final class AuditaurDiagnosticsTests: XCTestCase {
    func testEnvelopeEncodingUsesAuditaurCamelCaseShape() async throws {
        let exporter = InMemoryAuditaurExporter()
        let diagnostics = AuditaurDiagnostics(
            serviceName: "cutready-ios",
            sessionId: "session-1",
            exporter: exporter,
            now: { Date(timeIntervalSince1970: 1) }
        )

        let event = try await diagnostics.recordEvent(
            name: "cutready.auth.complete",
            message: "Auth completed",
            attributes: [
                "account.kind": "consumer",
                "retry.count": 2,
                "success": true,
            ]
        )

        let data = try AuditaurJSON.encoder().encode(event)
        let json = String(decoding: data, as: UTF8.self)

        XCTAssertTrue(json.contains("\"serviceName\":\"cutready-ios\""))
        XCTAssertTrue(json.contains("\"sessionId\":\"session-1\""))
        XCTAssertTrue(json.contains("\"timestampUnixNanos\":1000000000"))
        XCTAssertTrue(json.contains("\"severityText\":\"INFO\""))
        XCTAssertTrue(json.contains("\"level\":\"info\""))
        XCTAssertTrue(json.contains("\"name\":\"cutready.auth.complete\""))
        XCTAssertEqual(event.attributes["retry.count"], .int(2))
    }

    func testBreadcrumbsAreAttachedToRecordedEvents() async throws {
        let exporter = InMemoryAuditaurExporter()
        let diagnostics = AuditaurDiagnostics(
            serviceName: "cutready-ios",
            sessionId: "session-1",
            exporter: exporter,
            now: { Date(timeIntervalSince1970: 2) }
        )

        diagnostics.addBreadcrumb(
            name: "screen.open",
            message: "Sketch editor opened",
            attributes: ["screen": "sketch"]
        )

        let event = try await diagnostics.recordEvent(name: "cutready.sketch.edit")

        XCTAssertEqual(event.breadcrumbs.count, 1)
        XCTAssertEqual(event.breadcrumbs.first?.name, "screen.open")
        XCTAssertEqual(event.breadcrumbs.first?.attributes["screen"], .string("sketch"))
        let exportedEvents = await exporter.exportedEvents()
        XCTAssertEqual(exportedEvents.count, 1)
    }

    func testSpanLifecycleRecordsDurationAndStatus() async throws {
        let exporter = InMemoryAuditaurExporter()
        var dates = [
            Date(timeIntervalSince1970: 10),
            Date(timeIntervalSince1970: 10.25),
        ]
        let diagnostics = AuditaurDiagnostics(
            serviceName: "cutready-ios",
            sessionId: "session-1",
            exporter: exporter,
            now: { dates.removeFirst() },
            idGenerator: { "0123456789abcdef0123456789abcdef" }
        )

        let span = diagnostics.startSpan(
            name: "cutready.sync.push",
            attributes: ["entity.count": 3]
        )
        let record = try await diagnostics.endSpan(
            span,
            status: .error,
            statusMessage: "Upload rejected",
            attributes: ["http.status_code": 409]
        )

        XCTAssertEqual(record.traceId, "0123456789abcdef0123456789abcdef")
        XCTAssertEqual(record.spanId, "0123456789abcdef")
        XCTAssertEqual(record.statusCode, "ERROR")
        XCTAssertEqual(record.statusMessage, "Upload rejected")
        XCTAssertEqual(record.durationMs, 250)
        XCTAssertEqual(record.attributes["entity.count"], .int(3))
        XCTAssertEqual(record.attributes["http.status_code"], .int(409))
        let exportedSpans = await exporter.exportedSpans()
        XCTAssertEqual(exportedSpans, [record])
    }

    func testCaptureErrorRecordsNonfatalErrorEventAndErrorExport() async throws {
        let exporter = InMemoryAuditaurExporter()
        let diagnostics = AuditaurDiagnostics(
            serviceName: "cutready-ios",
            sessionId: "session-1",
            exporter: exporter,
            now: { Date(timeIntervalSince1970: 3) }
        )
        let error = NSError(domain: "CutReady.Sync", code: 42, userInfo: [
            NSLocalizedDescriptionKey: "Push failed",
        ])

        let event = try await diagnostics.capture(
            error: error,
            name: "cutready.sync.push.error",
            attributes: ["retryable": true]
        )

        XCTAssertEqual(event.severityText, "ERROR")
        XCTAssertEqual(event.level, "error")
        XCTAssertEqual(event.message, "Push failed")
        XCTAssertEqual(event.attributes["error.domain"], .string("CutReady.Sync"))
        XCTAssertEqual(event.attributes["error.code"], .int(42))
        XCTAssertEqual(event.attributes["retryable"], .bool(true))
        let exportedEvents = await exporter.exportedEvents()
        let exportedErrors = await exporter.exportedErrors()
        XCTAssertEqual(exportedEvents, [event])
        XCTAssertEqual(exportedErrors, [event])
    }

    func testSpanEventsAndInMemoryExporterExposeExportedBatches() async throws {
        let exporter = InMemoryAuditaurExporter()
        let diagnostics = AuditaurDiagnostics(
            serviceName: "cutready-ios",
            sessionId: "session-1",
            exporter: exporter,
            now: { Date(timeIntervalSince1970: 4) },
            idGenerator: { "abcdef0123456789abcdef0123456789" }
        )

        let span = diagnostics.startSpan(name: "cutready.agentive.rewrite")
        let spanEvent = try await diagnostics.recordSpanEvent(
            span: span,
            name: "rewrite.prompt.sent",
            attributes: ["model": "local"]
        )

        let batches = await exporter.exportedBatches()
        let exportedSpanEvents = await exporter.exportedSpanEvents()
        XCTAssertEqual(batches.count, 1)
        XCTAssertEqual(batches.first?.spanEvents, [spanEvent])
        XCTAssertEqual(exportedSpanEvents, [spanEvent])
    }

    func testFileExporterWritesBatchJsonForCliCollection() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        defer {
            try? FileManager.default.removeItem(at: directory)
        }
        let exporter = FileAuditaurExporter(directory: directory)
        let diagnostics = AuditaurDiagnostics(
            serviceName: "cutready-ios",
            sessionId: "session-1",
            exporter: exporter,
            now: { Date(timeIntervalSince1970: 5) }
        )

        try await diagnostics.recordEvent(name: "cutready.auth.complete")

        let files = try FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: nil
        )
        XCTAssertEqual(files.count, 1)
        let json = String(decoding: try Data(contentsOf: files[0]), as: UTF8.self)
        XCTAssertTrue(json.contains("\"events\":["))
        XCTAssertTrue(json.contains("\"cutready.auth.complete\""))
    }
}
