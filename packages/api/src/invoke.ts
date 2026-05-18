import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type { AuditaurExporter } from './exporter';
import type { SpanRecord } from './types';
import { errorRecordFields, maybePayload, nowUnixNanos, randomSpanId, randomTraceId, summarizePayload } from './utils';

export async function instrumentedInvoke<T>(
  exporter: AuditaurExporter,
  command: string,
  args: Record<string, unknown> | undefined,
  maxPayloadBytes: number,
  captureFullPayloads: boolean,
): Promise<T> {
  const start = nowUnixNanos();
  const traceId = randomTraceId();
  const spanId = randomSpanId();
  try {
    const result = await tauriInvoke<T>(command, args);
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, 'OK', undefined, args, maxPayloadBytes, captureFullPayloads));
    return result;
  } catch (error) {
    const fields = errorRecordFields(error);
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, 'ERROR', fields.message, args, maxPayloadBytes, captureFullPayloads));
    throw error;
  }
}

function invokeSpan(
  command: string,
  traceId: string,
  spanId: string,
  startTimeUnixNanos: number,
  statusCode: 'OK' | 'ERROR',
  statusMessage: string | undefined,
  args: Record<string, unknown> | undefined,
  maxPayloadBytes: number,
  captureFullPayloads: boolean,
): SpanRecord {
  return {
    sessionId: '',
    traceId,
    spanId,
    name: `tauri.invoke ${command}`,
    kind: 'client',
    startTimeUnixNanos,
    endTimeUnixNanos: nowUnixNanos(),
    statusCode,
    statusMessage,
    attributes: {
      'tauri.command': command,
      'auditaur.source': 'frontend',
      'tauri.command.args.summary': summarizePayload(args ?? {}, maxPayloadBytes),
      'tauri.command.args': maybePayload(args ?? {}, captureFullPayloads, maxPayloadBytes),
    },
    source: 'frontend',
  };
}
