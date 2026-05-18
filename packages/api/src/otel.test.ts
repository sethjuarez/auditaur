import { BasicTracerProvider, SimpleSpanProcessor } from '@opentelemetry/sdk-trace-base';
import { describe, expect, it } from 'vitest';
import {
  createAuditaurSpanExporter,
  hrTimeToUnixNanos,
  readableSpanToSpanRecord,
  type ReadableSpanLike,
} from './otel';
import type { SpanRecord } from './types';

describe('OpenTelemetry span exporter', () => {
  it('converts hrtime tuples to unix nanoseconds', () => {
    expect(hrTimeToUnixNanos([1_700_000_000, 123])).toBe(1_700_000_000_000_000_000 + 123);
  });

  it('maps readable spans to Auditaur span records', () => {
    const span = fakeSpan({
      kind: 2,
      status: { code: 2, message: 'boom' },
      resource: { attributes: { 'service.name': 'checkout' } },
      events: [{ name: 'exception', time: [10, 20], attributes: { message: 'boom' } }],
      links: [{ context: { traceId: 'f'.repeat(32), spanId: 'e'.repeat(16) }, attributes: { batch: 1 } }],
    });

    expect(readableSpanToSpanRecord(span, 'session-1')).toEqual({
      sessionId: 'session-1',
      traceId: 'a'.repeat(32),
      spanId: 'b'.repeat(16),
      parentSpanId: 'c'.repeat(16),
      name: 'http.request',
      kind: 'CLIENT',
      startTimeUnixNanos: 1_000_000_002,
      endTimeUnixNanos: 3_000_000_004,
      statusCode: 'ERROR',
      statusMessage: 'boom',
      scopeName: 'library',
      scopeVersion: '1.2.3',
      attributes: {
        'resource.service.name': 'checkout',
        route: '/health',
        'otel.events': [{ name: 'exception', timestampUnixNanos: 10_000_000_020, attributes: { message: 'boom' } }],
        'otel.links': [{ traceId: 'f'.repeat(32), spanId: 'e'.repeat(16), attributes: { batch: 1 } }],
      },
      source: 'third_party_otel',
    });
  });

  it('is structurally compatible with OpenTelemetry span processors', async () => {
    const spans: SpanRecord[] = [];
    const exporter = createAuditaurSpanExporter({
      exporter: {
        addSpan(span) {
          spans.push(span);
        },
        flush: async () => {},
        shutdown: async () => {},
      },
    });
    const provider = new BasicTracerProvider({
      spanProcessors: [new SimpleSpanProcessor(exporter)],
    });

    const tracer = provider.getTracer('third-party-library', '4.5.6');
    const span = tracer.startSpan('third-party-operation');
    span.setAttribute('db.system.name', 'sqlite');
    span.addEvent('query.started', { sql: 'select 1' });
    span.end();
    await provider.forceFlush();

    expect(spans).toHaveLength(1);
    expect(spans[0]).toMatchObject({
      name: 'third-party-operation',
      scopeName: 'third-party-library',
      scopeVersion: '4.5.6',
      attributes: {
        'db.system.name': 'sqlite',
      },
      source: 'third_party_otel',
    });
    expect(spans[0].traceId).toHaveLength(32);
    expect(spans[0].spanId).toHaveLength(16);
    expect(spans[0].attributes['otel.events']).toEqual([
      expect.objectContaining({ name: 'query.started', attributes: { sql: 'select 1' } }),
    ]);
  });
});

function fakeSpan(overrides: Partial<ReadableSpanLike> = {}): ReadableSpanLike {
  return {
    name: 'http.request',
    kind: 0,
    spanContext: () => ({ traceId: 'a'.repeat(32), spanId: 'b'.repeat(16) }),
    parentSpanContext: { traceId: 'a'.repeat(32), spanId: 'c'.repeat(16) },
    startTime: [1, 2],
    endTime: [3, 4],
    status: { code: 1 },
    attributes: { route: '/health' },
    resource: { attributes: {} },
    instrumentationScope: { name: 'library', version: '1.2.3' },
    events: [],
    links: [],
    ...overrides,
  };
}
