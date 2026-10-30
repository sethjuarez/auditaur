import {
  emit as tauriEmit,
  emitTo as tauriEmitTo,
  listen as tauriListen,
  type Event,
} from '@tauri-apps/api/event';
import type { AuditaurExporter } from './exporter';
import { recordErrorMessage, recordPayload, type PayloadPolicy, type RecordedPayload } from './payload';
import type { AuditaurEvent, SpanRecord, TauriEventRecord } from './types';
import { currentWindowLabel, nowUnixNanos, randomSpanId, randomTraceId } from './utils';

export async function instrumentedEmit(
  exporter: AuditaurExporter,
  event: string,
  payload: unknown,
  policy: PayloadPolicy,
): Promise<void> {
  return emitAndRecord(exporter, event, undefined, payload, policy, () => tauriEmit(event, payload));
}

export async function instrumentedEmitTo(
  exporter: AuditaurExporter,
  target: string,
  event: string,
  payload: unknown,
  policy: PayloadPolicy,
): Promise<void> {
  return emitAndRecord(exporter, event, target, payload, policy, () => tauriEmitTo(target, event, payload));
}

async function emitAndRecord(
  exporter: AuditaurExporter,
  event: string,
  target: string | undefined,
  payload: unknown,
  policy: PayloadPolicy,
  send: () => Promise<void>,
): Promise<void> {
  const start = nowUnixNanos();
  const windowLabel = currentWindowLabel();
  const record = (statusCode: 'OK' | 'ERROR', statusMessage?: string) => {
    const recorded = eventPayload(policy, event, payload);
    const span = eventSpan(event, 'emit', statusCode, statusMessage, windowLabel, target, recorded, start);
    exporter.addSpan(span);
    exporter.addTauriEvent(eventRecord(event, 'emit', span, target, windowLabel, recorded));
  };
  try {
    await send();
    record('OK');
  } catch (error) {
    record('ERROR', recordErrorMessage(policy, event, error));
    throw error;
  }
}

export async function instrumentedListen<T>(
  exporter: AuditaurExporter,
  event: string,
  handler: (event: AuditaurEvent<T>) => void,
  policy: PayloadPolicy,
): Promise<() => void> {
  const windowLabel = currentWindowLabel();
  return tauriListen<T>(event, (received: Event<T>) => {
    const recorded = eventPayload(policy, event, received.payload);
    const span = eventSpan(event, 'receive', 'OK', undefined, windowLabel, undefined, recorded);
    exporter.addSpan(span);
    exporter.addTauriEvent(eventRecord(event, 'receive', span, undefined, windowLabel, recorded));
    handler(received);
  });
}

function eventPayload(policy: PayloadPolicy, event: string, payload: unknown): RecordedPayload {
  return payload === undefined ? {} : recordPayload(policy, 'eventPayload', event, payload);
}

function eventSpan(
  event: string,
  direction: 'emit' | 'receive',
  statusCode: 'OK' | 'ERROR',
  statusMessage: string | undefined,
  windowLabel: string | undefined,
  target: string | undefined,
  payload: RecordedPayload,
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
      'tauri.window.label': windowLabel,
      'tauri.event.payload.summary': payload.summary,
      'tauri.event.payload': payload.json,
      'auditaur.source': 'frontend',
    },
    source: 'frontend',
  };
}

function eventRecord(
  eventName: string,
  direction: 'emit' | 'receive',
  span: SpanRecord,
  target: string | undefined,
  windowLabel: string | undefined,
  payload: RecordedPayload,
): TauriEventRecord {
  return {
    sessionId: '',
    timestampUnixNanos: span.startTimeUnixNanos,
    eventName,
    direction,
    target,
    traceId: span.traceId,
    spanId: span.spanId,
    windowLabel,
    payloadSummary: payload.summary,
    payloadJson: payload.json,
    payloadRedacted: true,
  };
}