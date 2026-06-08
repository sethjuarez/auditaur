import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type { AuditaurExporter } from './exporter';
import type { AuditaurInvokeArgs, AuditaurTraceContextCarrier, SpanRecord, TauriIpcCallRecord } from './types';
import { currentWindowLabel, errorRecordFields, maybePayload, nowUnixNanos, randomSpanId, randomTraceId, summarizePayload } from './utils';

export const AUDITAUR_TRACE_CONTEXT_ARG = 'auditaurTraceContext';

export function createAuditaurTraceContext(
  traceId = randomTraceId(),
  spanId = randomSpanId(),
  traceFlags = '01',
): AuditaurTraceContextCarrier {
  return { traceparent: `00-${traceId}-${spanId}-${traceFlags}` };
}

export async function instrumentedInvoke<T>(
  exporter: AuditaurExporter,
  command: string,
  args: AuditaurInvokeArgs | undefined,
  maxPayloadBytes: number,
  captureFullPayloads: boolean,
  propagateTraceContext: boolean,
): Promise<T> {
  const start = nowUnixNanos();
  const traceId = randomTraceId();
  const spanId = randomSpanId();
  const traceContext = createAuditaurTraceContext(traceId, spanId);
  const windowLabel = currentWindowLabel();
  const invokeArgs = propagateTraceContext ? argsWithTraceContext(args, traceContext) : args;
  try {
    const result = await tauriInvoke<T>(command, invokeArgs);
    const end = nowUnixNanos();
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, end, 'OK', undefined, windowLabel, args, maxPayloadBytes, captureFullPayloads));
    exporter.addTauriIpcCall(ipcCall(command, traceId, spanId, start, end, 'OK', undefined, windowLabel, args, result, maxPayloadBytes, captureFullPayloads));
    return result;
  } catch (error) {
    const fields = errorRecordFields(error);
    const end = nowUnixNanos();
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, end, 'ERROR', fields.message, windowLabel, args, maxPayloadBytes, captureFullPayloads));
    exporter.addTauriIpcCall(ipcCall(command, traceId, spanId, start, end, 'ERROR', fields.message, windowLabel, args, undefined, maxPayloadBytes, captureFullPayloads));
    throw error;
  }
}

function argsWithTraceContext(args: AuditaurInvokeArgs | undefined, traceContext: AuditaurTraceContextCarrier): AuditaurInvokeArgs {
  if (args && Object.prototype.hasOwnProperty.call(args, AUDITAUR_TRACE_CONTEXT_ARG)) {
    return args;
  }
  return {
    ...(args ?? {}),
    [AUDITAUR_TRACE_CONTEXT_ARG]: traceContext,
  };
}

function invokeSpan(
  command: string,
  traceId: string,
  spanId: string,
  startTimeUnixNanos: number,
  endTimeUnixNanos: number,
  statusCode: 'OK' | 'ERROR',
  statusMessage: string | undefined,
  windowLabel: string,
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
      'tauri.window.label': windowLabel,
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
  windowLabel: string,
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
    windowLabel,
    argsJson: maybePayload(args ?? {}, captureFullPayloads, maxPayloadBytes),
    argsRedacted: true,
    resultSummary: status === 'OK' ? summarizePayload(result, maxPayloadBytes) : undefined,
  };
}
