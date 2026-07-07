import Foundation

public typealias AuditaurAttributes = [String: AuditaurValue]

public enum AuditaurSeverity: String, Codable, Equatable, CaseIterable {
    case trace
    case debug
    case info
    case warn
    case error
    case fatal

    public var severityText: String {
        rawValue.uppercased()
    }

    public var severityNumber: Int {
        switch self {
        case .trace:
            return 1
        case .debug:
            return 5
        case .info:
            return 9
        case .warn:
            return 13
        case .error:
            return 17
        case .fatal:
            return 21
        }
    }
}

public enum AuditaurSpanStatus: String, Codable, Equatable {
    case unset = "UNSET"
    case ok = "OK"
    case error = "ERROR"
}

public struct AuditaurSpan: Equatable {
    public let sessionId: String
    public let traceId: String
    public let spanId: String
    public let parentSpanId: String?
    public let name: String
    public let kind: String
    public let startTimeUnixNanos: Int64
    public let attributes: AuditaurAttributes
}

public struct AuditaurExportBatch: Codable, Equatable {
    public var schemaVersion: Int
    public var events: [AuditaurEventEnvelope]
    public var spans: [AuditaurSpanRecord]
    public var spanEvents: [AuditaurSpanEventRecord]
    public var errors: [AuditaurEventEnvelope]

    public init(
        schemaVersion: Int = 1,
        events: [AuditaurEventEnvelope] = [],
        spans: [AuditaurSpanRecord] = [],
        spanEvents: [AuditaurSpanEventRecord] = [],
        errors: [AuditaurEventEnvelope] = []
    ) {
        self.schemaVersion = schemaVersion
        self.events = events
        self.spans = spans
        self.spanEvents = spanEvents
        self.errors = errors
    }
}

public struct AuditaurEventEnvelope: Codable, Equatable {
    public var schemaVersion: Int
    public var serviceName: String
    public var sessionId: String
    public var timestampUnixNanos: Int64
    public var severityText: String
    public var severityNumber: Int
    public var level: String
    public var name: String
    public var message: String?
    public var attributes: AuditaurAttributes
    public var breadcrumbs: [AuditaurBreadcrumb]
    public var traceId: String?
    public var spanId: String?
    public var source: String

    public init(
        schemaVersion: Int = 1,
        serviceName: String,
        sessionId: String,
        timestampUnixNanos: Int64,
        severityText: String,
        severityNumber: Int,
        level: String,
        name: String,
        message: String?,
        attributes: AuditaurAttributes = [:],
        breadcrumbs: [AuditaurBreadcrumb] = [],
        traceId: String? = nil,
        spanId: String? = nil,
        source: String = "apple"
    ) {
        self.schemaVersion = schemaVersion
        self.serviceName = serviceName
        self.sessionId = sessionId
        self.timestampUnixNanos = timestampUnixNanos
        self.severityText = severityText
        self.severityNumber = severityNumber
        self.level = level
        self.name = name
        self.message = message
        self.attributes = attributes
        self.breadcrumbs = breadcrumbs
        self.traceId = traceId
        self.spanId = spanId
        self.source = source
    }
}

public struct AuditaurBreadcrumb: Codable, Equatable {
    public var timestampUnixNanos: Int64
    public var severityText: String
    public var level: String
    public var name: String
    public var message: String?
    public var attributes: AuditaurAttributes

    public init(
        timestampUnixNanos: Int64,
        severityText: String,
        level: String,
        name: String,
        message: String?,
        attributes: AuditaurAttributes = [:]
    ) {
        self.timestampUnixNanos = timestampUnixNanos
        self.severityText = severityText
        self.level = level
        self.name = name
        self.message = message
        self.attributes = attributes
    }
}

public struct AuditaurSpanRecord: Codable, Equatable {
    public var sessionId: String
    public var traceId: String
    public var spanId: String
    public var parentSpanId: String?
    public var name: String
    public var kind: String?
    public var startTimeUnixNanos: Int64
    public var endTimeUnixNanos: Int64?
    public var durationMs: Double?
    public var statusCode: String?
    public var statusMessage: String?
    public var scopeName: String?
    public var scopeVersion: String?
    public var attributes: AuditaurAttributes
    public var source: String
}

public struct AuditaurSpanEventRecord: Codable, Equatable {
    public var sessionId: String
    public var traceId: String
    public var spanId: String
    public var name: String
    public var timestampUnixNanos: Int64
    public var attributes: AuditaurAttributes
}

public enum AuditaurValue: Codable, Equatable, ExpressibleByStringLiteral, ExpressibleByIntegerLiteral, ExpressibleByFloatLiteral, ExpressibleByBooleanLiteral, ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral {
    case string(String)
    case int(Int)
    case double(Double)
    case bool(Bool)
    case array([AuditaurValue])
    case object([String: AuditaurValue])
    case null

    public init(stringLiteral value: String) {
        self = .string(value)
    }

    public init(integerLiteral value: Int) {
        self = .int(value)
    }

    public init(floatLiteral value: Double) {
        self = .double(value)
    }

    public init(booleanLiteral value: Bool) {
        self = .bool(value)
    }

    public init(arrayLiteral elements: AuditaurValue...) {
        self = .array(elements)
    }

    public init(dictionaryLiteral elements: (String, AuditaurValue)...) {
        self = .object(Dictionary(uniqueKeysWithValues: elements))
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Int.self) {
            self = .int(value)
        } else if let value = try? container.decode(Double.self) {
            self = .double(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([AuditaurValue].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode([String: AuditaurValue].self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value):
            try container.encode(value)
        case .int(let value):
            try container.encode(value)
        case .double(let value):
            try container.encode(value)
        case .bool(let value):
            try container.encode(value)
        case .array(let value):
            try container.encode(value)
        case .object(let value):
            try container.encode(value)
        case .null:
            try container.encodeNil()
        }
    }
}
