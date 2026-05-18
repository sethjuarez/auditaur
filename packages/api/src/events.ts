import {
  emit as tauriEmit,
  emitTo as tauriEmitTo,
  listen as tauriListen,
  type Event,
} from '@tauri-apps/api/event';
import type { AuditaurExporter } from './exporter';
import type { AuditaurEvent, SpanRecord } from './types';
import { errorRecordFields, maybePayload, nowUnixNanos, randomSpanId, randomTraceId, summarizePayload } from './utils';

export async function instrumentedEmit(
  exporter: AuditaurExporter,
  event: string,
  payload: unknown,
  maxPayloadBytes: number,
  captureFullPayloads: boolean,
): Promise<void> {
  const start = nowUnixNanos();
  try {
    await tauriEmit(event, payload);
    exporter.addSpan(eventSpan(event, 'emit', 'OK', undefined, undefined, payload, maxPayloadBytes, captureFullPayloads, start));
  } catch (error) {
    exporter.addSpan(eventSpan(event, 'emit', 'ERROR', errorRecordFields(error).message, undefined, payload, maxPayloadBytes, captureFullPayloads, start));
    throw error;
  }
}

export async function instrumentedEmitTo(
  exporter: AuditaurExporter,
  target: string,
  event: string,
  payload: unknown,
  maxPayloadBytes: number,
  captureFullPayloads: boolean,
): Promise<void> {
  const start = nowUnixNanos();
  try {
    await tauriEmitTo(target, event, payload);
    exporter.addSpan(eventSpan(event, 'emit', 'OK', undefined, target, payload, maxPayloadBytes, captureFullPayloads, start));
  } catch (error) {
    exporter.addSpan(eventSpan(event, 'emit', 'ERROR', errorRecordFields(error).message, target, payload, maxPayloadBytes, captureFullPayloads, start));
    throw error;
  }
}

export async function instrumentedListen<T>(
  exporter: AuditaurExporter,
  event: string,
  handler: (event: AuditaurEvent<T>) => void,
): Promise<() => void> {
  return tauriListen<T>(event, (received: Event<T>) => {
    exporter.addSpan(eventSpan(event, 'receive', 'OK'));
    handler(received);
  });
}

function eventSpan(
  event: string,
  direction: 'emit' | 'receive',
  statusCode: 'OK' | 'ERROR',
  statusMessage?: string,
  target?: string,
  payload?: unknown,
  maxPayloadBytes = 16_384,
  captureFullPayloads = false,
  startTimeUnixNanos = nowUnixNanos(),
): SpanRecord {
  const endTimeUnixNanos = nowUnixNanos();
  return {
    sessionId: '',
    traceId: randomTraceId(),
    spanId: randomSpanId(),
    name: `tauri.event ${event}`,
    kind: 'internal',
    startTimeUnixNanos,
    endTimeUnixNanos,
    statusCode,
    statusMessage,
    attributes: {
      'tauri.event.name': event,
      'tauri.event.direction': direction,
      'tauri.event.target': target,
      'tauri.event.payload.summary': payload === undefined ? undefined : summarizePayload(payload, maxPayloadBytes),
      'tauri.event.payload': payload === undefined ? undefined : maybePayload(payload, captureFullPayloads, maxPayloadBytes),
      'auditaur.source': 'frontend',
    },
    source: 'frontend',
  };
}
