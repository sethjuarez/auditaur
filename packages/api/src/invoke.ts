import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type { AuditaurExporter } from './exporter';
import type { AuditaurInvokeArgs, AuditaurTraceContextCarrier, SpanRecord, TauriIpcCallRecord } from './types';
import { recordPayload, type PayloadPolicy, type RecordedPayload } from './payload';
import { currentWindowLabel, errorRecordFields, nowUnixNanos, randomSpanId, randomTraceId } from './utils';

export const AUDITAUR_TRACE_CONTEXT_ARG = 'auditaurTraceContext';
export const AUDITAUR_TRACEPARENT_HEADER = 'traceparent';

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
  policy: PayloadPolicy,
  propagateTraceContext: boolean,
): Promise<T> {
  const start = nowUnixNanos();
  const traceId = randomTraceId();
  const spanId = randomSpanId();
  const traceContext = createAuditaurTraceContext(traceId, spanId);
  const windowLabel = currentWindowLabel();
  const shouldPropagateTraceContext = propagateTraceContext && !hasReservedTraceContextArg(args);
  const invokeArgs = shouldPropagateTraceContext ? argsWithTraceContext(args, traceContext) : args;
  const invokeOptions = shouldPropagateTraceContext ? optionsWithTraceContext(traceContext) : undefined;
  try {
    const result = await tauriInvoke<T>(command, invokeArgs, invokeOptions);
    const end = nowUnixNanos();
    const recordedArgs = recordPayload(policy, 'invokeArgs', command, args ?? {});
    const recordedResult = recordPayload(policy, 'invokeResult', command, result);
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, end, 'OK', undefined, windowLabel, recordedArgs));
    exporter.addTauriIpcCall(ipcCall(command, traceId, spanId, start, end, 'OK', undefined, windowLabel, recordedArgs, recordedResult.summary));
    return result;
  } catch (error) {
    const fields = errorRecordFields(error);
    const end = nowUnixNanos();
    const recordedArgs = recordPayload(policy, 'invokeArgs', command, args ?? {});
    exporter.addSpan(invokeSpan(command, traceId, spanId, start, end, 'ERROR', fields.message, windowLabel, recordedArgs));
    exporter.addTauriIpcCall(ipcCall(command, traceId, spanId, start, end, 'ERROR', fields.message, windowLabel, recordedArgs, undefined));
    throw error;
  }
}

function argsWithTraceContext(args: AuditaurInvokeArgs | undefined, traceContext: AuditaurTraceContextCarrier): AuditaurInvokeArgs {
  if (args && hasReservedTraceContextArg(args)) {
    return args;
  }
  return {
    ...(args ?? {}),
    [AUDITAUR_TRACE_CONTEXT_ARG]: traceContext,
  };
}

function hasReservedTraceContextArg(args: AuditaurInvokeArgs | undefined): boolean {
  return Boolean(args && Object.prototype.hasOwnProperty.call(args, AUDITAUR_TRACE_CONTEXT_ARG));
}

function optionsWithTraceContext(traceContext: AuditaurTraceContextCarrier): { headers: HeadersInit } | undefined {
  if (!traceContext.traceparent) {
    return undefined;
  }
  return {
    headers: {
      [AUDITAUR_TRACEPARENT_HEADER]: traceContext.traceparent,
    },
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
  args: RecordedPayload,
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
      'tauri.command.args.summary': args.summary,
      'tauri.command.args': args.json,
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
  args: RecordedPayload,
  resultSummary: string | undefined,
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
    argsJson: args.json,
    argsRedacted: true,
    resultSummary,
  };
}
