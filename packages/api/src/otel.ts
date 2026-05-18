import type { SpanRecord } from './types';

const EXPORT_SUCCESS = 0;
const EXPORT_FAILED = 1;

export interface AuditaurOpenTelemetrySpanExporterOptions {
  exporter: AuditaurSpanSink;
  sessionId?: string;
}

export interface AuditaurSpanSink {
  addSpan(record: SpanRecord): void;
  flush(): Promise<void>;
  shutdown(): Promise<void>;
}

export interface OpenTelemetrySpanExporter {
  export(spans: ReadableSpanLike[], resultCallback: (result: ExportResultLike) => void): void;
  forceFlush(): Promise<void>;
  shutdown(): Promise<void>;
}

export interface ExportResultLike {
  code: 0 | 1;
  error?: Error;
}

export interface ReadableSpanLike {
  name: string;
  kind: number;
  spanContext(): SpanContextLike;
  parentSpanContext?: SpanContextLike;
  startTime: HrTimeLike;
  endTime?: HrTimeLike;
  status?: {
    code: number;
    message?: string;
  };
  attributes?: Record<string, unknown>;
  links?: LinkLike[];
  events?: TimedEventLike[];
  resource?: {
    attributes?: Record<string, unknown>;
  };
  instrumentationScope?: {
    name: string;
    version?: string;
  };
}

export interface SpanContextLike {
  traceId: string;
  spanId: string;
}

export type HrTimeLike = readonly [seconds: number, nanos: number];

export interface TimedEventLike {
  name: string;
  time: HrTimeLike;
  attributes?: Record<string, unknown>;
  droppedAttributesCount?: number;
}

export interface LinkLike {
  context: SpanContextLike;
  attributes?: Record<string, unknown>;
  droppedAttributesCount?: number;
}

export function createAuditaurSpanExporter(
  options: AuditaurOpenTelemetrySpanExporterOptions,
): OpenTelemetrySpanExporter {
  return new AuditaurOpenTelemetrySpanExporter(options);
}

export function readableSpanToSpanRecord(span: ReadableSpanLike, sessionId = ''): SpanRecord {
  const context = span.spanContext();
  return {
    sessionId,
    traceId: context.traceId,
    spanId: context.spanId,
    parentSpanId: span.parentSpanContext?.spanId,
    name: span.name,
    kind: spanKindName(span.kind),
    startTimeUnixNanos: hrTimeToUnixNanos(span.startTime),
    endTimeUnixNanos: span.endTime ? hrTimeToUnixNanos(span.endTime) : undefined,
    statusCode: spanStatusCode(span.status?.code),
    statusMessage: span.status?.message,
    scopeName: span.instrumentationScope?.name,
    scopeVersion: span.instrumentationScope?.version,
    attributes: spanAttributes(span),
    source: 'third_party_otel',
  };
}

export function hrTimeToUnixNanos(hrTime: HrTimeLike): number {
  const [seconds, nanos] = hrTime;
  return seconds * 1_000_000_000 + nanos;
}

class AuditaurOpenTelemetrySpanExporter implements OpenTelemetrySpanExporter {
  constructor(private readonly options: AuditaurOpenTelemetrySpanExporterOptions) {}

  export(spans: ReadableSpanLike[], resultCallback: (result: ExportResultLike) => void): void {
    try {
      for (const span of spans) {
        this.options.exporter.addSpan(readableSpanToSpanRecord(span, this.options.sessionId));
      }
      resultCallback({ code: EXPORT_SUCCESS });
    } catch (error) {
      resultCallback({
        code: EXPORT_FAILED,
        error: error instanceof Error ? error : new Error(String(error)),
      });
    }
  }

  forceFlush(): Promise<void> {
    return this.options.exporter.flush();
  }

  shutdown(): Promise<void> {
    return this.options.exporter.shutdown();
  }
}

function spanKindName(kind: number): string {
  switch (kind) {
    case 1:
      return 'SERVER';
    case 2:
      return 'CLIENT';
    case 3:
      return 'PRODUCER';
    case 4:
      return 'CONSUMER';
    default:
      return 'INTERNAL';
  }
}

function spanStatusCode(code: number | undefined): SpanRecord['statusCode'] {
  switch (code) {
    case 1:
      return 'OK';
    case 2:
      return 'ERROR';
    default:
      return undefined;
  }
}

function spanAttributes(span: ReadableSpanLike): Record<string, unknown> {
  return {
    ...prefixAttributes('resource.', span.resource?.attributes ?? {}),
    ...(span.attributes ?? {}),
    ...(span.events?.length ? { 'otel.events': span.events.map(eventRecord) } : {}),
    ...(span.links?.length ? { 'otel.links': span.links.map(linkRecord) } : {}),
  };
}

function prefixAttributes(prefix: string, attributes: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(attributes).map(([key, value]) => [`${prefix}${key}`, value]));
}

function eventRecord(event: TimedEventLike): Record<string, unknown> {
  return {
    name: event.name,
    timestampUnixNanos: hrTimeToUnixNanos(event.time),
    attributes: event.attributes ?? {},
    ...(event.droppedAttributesCount ? { droppedAttributesCount: event.droppedAttributesCount } : {}),
  };
}

function linkRecord(link: LinkLike): Record<string, unknown> {
  return {
    traceId: link.context.traceId,
    spanId: link.context.spanId,
    attributes: link.attributes ?? {},
    ...(link.droppedAttributesCount ? { droppedAttributesCount: link.droppedAttributesCount } : {}),
  };
}
