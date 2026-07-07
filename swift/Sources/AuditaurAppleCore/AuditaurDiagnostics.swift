import Foundation

public final class AuditaurDiagnostics {
    public let serviceName: String
    public let sessionId: String

    private let exporter: any AuditaurExporting
    private let now: () -> Date
    private let idGenerator: () -> String
    private let breadcrumbStore = LockedBreadcrumbStore()

    public init(
        serviceName: String,
        sessionId: String = UUID().uuidString,
        exporter: any AuditaurExporting = InMemoryAuditaurExporter(),
        now: @escaping () -> Date = Date.init,
        idGenerator: @escaping () -> String = { UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased() }
    ) {
        self.serviceName = serviceName
        self.sessionId = sessionId
        self.exporter = exporter
        self.now = now
        self.idGenerator = idGenerator
    }

    @discardableResult
    public func recordEvent(
        name: String,
        message: String? = nil,
        severity: AuditaurSeverity = .info,
        attributes: AuditaurAttributes = [:]
    ) async throws -> AuditaurEventEnvelope {
        let event = makeEvent(
            name: name,
            message: message,
            severity: severity,
            attributes: attributes
        )
        try await exporter.export(AuditaurExportBatch(events: [event]))
        return event
    }

    @discardableResult
    public func addBreadcrumb(
        name: String,
        message: String? = nil,
        severity: AuditaurSeverity = .info,
        attributes: AuditaurAttributes = [:]
    ) -> AuditaurBreadcrumb {
        let breadcrumb = AuditaurBreadcrumb(
            timestampUnixNanos: timestampUnixNanos(now()),
            severityText: severity.severityText,
            level: severity.rawValue,
            name: name,
            message: message,
            attributes: attributes
        )
        breadcrumbStore.append(breadcrumb)
        return breadcrumb
    }

    public func startSpan(
        name: String,
        kind: String = "internal",
        attributes: AuditaurAttributes = [:],
        parentSpanId: String? = nil
    ) -> AuditaurSpan {
        AuditaurSpan(
            sessionId: sessionId,
            traceId: makeTraceId(),
            spanId: makeSpanId(),
            parentSpanId: parentSpanId,
            name: name,
            kind: kind,
            startTimeUnixNanos: timestampUnixNanos(now()),
            attributes: attributes
        )
    }

    @discardableResult
    public func endSpan(
        _ span: AuditaurSpan,
        status: AuditaurSpanStatus = .ok,
        statusMessage: String? = nil,
        attributes: AuditaurAttributes = [:]
    ) async throws -> AuditaurSpanRecord {
        let endTimeUnixNanos = timestampUnixNanos(now())
        var mergedAttributes = span.attributes
        attributes.forEach { mergedAttributes[$0.key] = $0.value }

        let record = AuditaurSpanRecord(
            sessionId: span.sessionId,
            traceId: span.traceId,
            spanId: span.spanId,
            parentSpanId: span.parentSpanId,
            name: span.name,
            kind: span.kind,
            startTimeUnixNanos: span.startTimeUnixNanos,
            endTimeUnixNanos: endTimeUnixNanos,
            durationMs: Double(endTimeUnixNanos - span.startTimeUnixNanos) / 1_000_000,
            statusCode: status.rawValue,
            statusMessage: statusMessage,
            scopeName: "AuditaurAppleCore",
            scopeVersion: nil,
            attributes: mergedAttributes,
            source: "apple"
        )
        try await exporter.export(AuditaurExportBatch(spans: [record]))
        return record
    }

    @discardableResult
    public func recordSpanEvent(
        span: AuditaurSpan,
        name: String,
        attributes: AuditaurAttributes = [:]
    ) async throws -> AuditaurSpanEventRecord {
        let record = AuditaurSpanEventRecord(
            sessionId: sessionId,
            traceId: span.traceId,
            spanId: span.spanId,
            name: name,
            timestampUnixNanos: timestampUnixNanos(now()),
            attributes: attributes
        )
        try await exporter.export(AuditaurExportBatch(spanEvents: [record]))
        return record
    }

    @discardableResult
    public func capture(
        error: Error,
        name: String = "error.nonfatal",
        attributes: AuditaurAttributes = [:]
    ) async throws -> AuditaurEventEnvelope {
        let nsError = error as NSError
        var errorAttributes = attributes
        errorAttributes["error.type"] = .string(String(reflecting: type(of: error)))
        errorAttributes["error.domain"] = .string(nsError.domain)
        errorAttributes["error.code"] = .int(nsError.code)
        errorAttributes["exception.message"] = .string(nsError.localizedDescription)

        let event = makeEvent(
            name: name,
            message: nsError.localizedDescription,
            severity: .error,
            attributes: errorAttributes
        )
        try await exporter.export(AuditaurExportBatch(events: [event], errors: [event]))
        return event
    }

    private func makeEvent(
        name: String,
        message: String?,
        severity: AuditaurSeverity,
        attributes: AuditaurAttributes
    ) -> AuditaurEventEnvelope {
        AuditaurEventEnvelope(
            serviceName: serviceName,
            sessionId: sessionId,
            timestampUnixNanos: timestampUnixNanos(now()),
            severityText: severity.severityText,
            severityNumber: severity.severityNumber,
            level: severity.rawValue,
            name: name,
            message: message,
            attributes: attributes,
            breadcrumbs: breadcrumbStore.snapshot(),
            source: "apple"
        )
    }

    private func makeTraceId() -> String {
        let candidate = idGenerator()
        if candidate.count >= 32 {
            return String(candidate.prefix(32))
        }
        return candidate.padding(toLength: 32, withPad: "0", startingAt: 0)
    }

    private func makeSpanId() -> String {
        let candidate = idGenerator()
        if candidate.count >= 16 {
            return String(candidate.prefix(16))
        }
        return candidate.padding(toLength: 16, withPad: "0", startingAt: 0)
    }
}

private final class LockedBreadcrumbStore {
    private var breadcrumbs: [AuditaurBreadcrumb] = []
    private let lock = NSLock()

    func append(_ breadcrumb: AuditaurBreadcrumb) {
        lock.lock()
        breadcrumbs.append(breadcrumb)
        lock.unlock()
    }

    func snapshot() -> [AuditaurBreadcrumb] {
        lock.lock()
        let value = breadcrumbs
        lock.unlock()
        return value
    }
}

private func timestampUnixNanos(_ date: Date) -> Int64 {
    Int64((date.timeIntervalSince1970 * 1_000_000_000).rounded())
}
