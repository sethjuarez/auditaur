import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type { AuditaurExporter } from './exporter';
import type { SpanRecord, TauriIpcCallRecord } from './types';
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
    const end = nowUnixNanos();
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, end, 'OK', undefined, args, maxPayloadBytes, captureFullPayloads));
    exporter.addTauriIpcCall(ipcCall(command, traceId, spanId, start, end, 'OK', undefined, args, result, maxPayloadBytes, captureFullPayloads));
    return result;
  } catch (error) {
    const fields = errorRecordFields(error);
    const end = nowUnixNanos();
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, end, 'ERROR', fields.message, args, maxPayloadBytes, captureFullPayloads));
    exporter.addTauriIpcCall(ipcCall(command, traceId, spanId, start, end, 'ERROR', fields.message, args, undefined, maxPayloadBytes, captureFullPayloads));
    throw error;
  }
}

function invokeSpan(
  command: string,
  traceId: string,
  spanId: string,
  startTimeUnixNanos: number,
  endTimeUnixNanos: number,
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
    endTimeUnixNanos,
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

function ipcCall(
  command: string,
  traceId: string,
  spanId: string,
  startTimeUnixNanos: number,
  endTimeUnixNanos: number,
  status: 'OK' | 'ERROR',
  errorMessage: string | undefined,
  args: Record<string, unknown> | undefined,
  result: unknown,
  maxPayloadBytes: number,
  captureFullPayloads: boolean,
): TauriIpcCallRecord {
  return {
    sessionId: '',
    timestampUnixNanos: startTimeUnixNanos,
    durationMs: (endTimeUnixNanos - startTimeUnixNanos) / 1_000_000,
    command,
    status,
    errorMessage,
    traceId,
    spanId,
    argsJson: maybePayload(args ?? {}, captureFullPayloads, maxPayloadBytes),
    argsRedacted: true,
    resultSummary: status === 'OK' ? summarizePayload(result, maxPayloadBytes) : undefined,
  };
}
