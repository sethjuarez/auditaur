import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

public protocol AuditaurExporting: AnyObject {
    func export(_ batch: AuditaurExportBatch) async throws
}

public actor InMemoryAuditaurExporter: AuditaurExporting {
    private var batches: [AuditaurExportBatch] = []

    public init() {}

    public func export(_ batch: AuditaurExportBatch) async throws {
        batches.append(batch)
    }

    public func exportedBatches() -> [AuditaurExportBatch] {
        batches
    }

    public func exportedEvents() -> [AuditaurEventEnvelope] {
        batches.flatMap(\.events)
    }

    public func exportedSpans() -> [AuditaurSpanRecord] {
        batches.flatMap(\.spans)
    }

    public func exportedSpanEvents() -> [AuditaurSpanEventRecord] {
        batches.flatMap(\.spanEvents)
    }

    public func exportedErrors() -> [AuditaurEventEnvelope] {
        batches.flatMap(\.errors)
    }
}

public actor FileAuditaurExporter: AuditaurExporting {
    private let directory: URL
    private let encoder: JSONEncoder
    private var sequence: Int

    public init(
        directory: URL,
        encoder: JSONEncoder = AuditaurJSON.encoder(),
        sequence: Int = 0
    ) {
        self.directory = directory
        self.encoder = encoder
        self.sequence = sequence
    }

    public func export(_ batch: AuditaurExportBatch) async throws {
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        let fileName = String(format: "auditaur-apple-%06d.json", sequence)
        sequence += 1
        try encoder.encode(batch).write(
            to: directory.appendingPathComponent(fileName),
            options: .atomic
        )
    }
}

public final class AuditaurHTTPExporter: AuditaurExporting {
    private let endpoint: URL
    private let session: URLSession
    private let encoder: JSONEncoder
    private let additionalHeaders: [String: String]

    public init(
        endpoint: URL,
        session: URLSession = .shared,
        encoder: JSONEncoder = AuditaurJSON.encoder(),
        additionalHeaders: [String: String] = [:]
    ) {
        self.endpoint = endpoint
        self.session = session
        self.encoder = encoder
        self.additionalHeaders = additionalHeaders
    }

    public func export(_ batch: AuditaurExportBatch) async throws {
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        for (name, value) in additionalHeaders {
            request.setValue(value, forHTTPHeaderField: name)
        }
        request.httpBody = try encoder.encode(batch)

        let (_, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw AuditaurExportError.invalidHTTPResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw AuditaurExportError.httpStatus(httpResponse.statusCode)
        }
    }
}

public enum AuditaurExportError: Error, Equatable {
    case invalidHTTPResponse
    case httpStatus(Int)
}

public enum AuditaurJSON {
    public static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
}
