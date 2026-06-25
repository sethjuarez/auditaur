import type { OpenTelemetrySpanExporter } from './otel';

export interface AuditaurFrontendConfig {
  serviceName: string;
  serviceVersion?: string;
  instrumentConsole?: boolean;
  instrumentErrors?: boolean;
  instrumentTauriInvoke?: boolean;
  propagateTauriInvokeTraceContext?: boolean;
  instrumentTauriEvents?: boolean;
  captureFullPayloads?: boolean;
  maxPayloadBytes?: number;
  batchIntervalMs?: number;
  maxBatchSize?: number;
  onExportError?: (failure: AuditaurExportFailure) => void;
  driveBridge?: boolean | AuditaurDriveBridgeConfig;
}

export interface AuditaurClient {
  invoke<T>(command: string, args?: AuditaurInvokeArgs): Promise<T>;
  emit(event: string, payload?: unknown): Promise<void>;
  emitTo(target: string, event: string, payload?: unknown): Promise<void>;
  listen<T>(event: string, handler: (event: AuditaurEvent<T>) => void): Promise<() => void>;
  createOpenTelemetrySpanExporter(): OpenTelemetrySpanExporter;
  flush(): Promise<void>;
  shutdown(): Promise<void>;
}

export interface AuditaurDriveBridgeConfig {
  pollIntervalMs?: number;
  windowLabel?: string;
}

export interface AuditaurEvent<T> {
  event: string;
  id: number;
  payload: T;
}

export interface AuditaurTraceContextCarrier {
  traceparent?: string;
}

export type AuditaurInvokeArgs = Record<string, unknown> & {
  auditaurTraceContext?: AuditaurTraceContextCarrier;
};

export interface AuditaurExportFailure {
  error: unknown;
  attemptedRecords: number;
  queuedRecords: number;
  retainedRecords: number;
  droppedRecords: number;
}

export interface DriveBridgeRequest {
  schemaVersion: number;
  protocolVersion: number;
  requestId: string;
  action:
    | 'exists'
    | 'text'
    | 'click'
    | 'fill'
    | 'type'
    | 'press'
    | 'hover'
    | 'select'
    | 'check'
    | 'uncheck'
    | 'evaluate'
    | 'snapshot'
    | 'screenshot'
    | string;
  selector?: string;
  value?: string;
  values?: string[];
  visibleOnly: boolean;
  windowLabel?: string;
  testId?: string;
  stepId?: string;
  createdAtUnixNanos: number;
}

export interface DriveBridgeResponse {
  schemaVersion: number;
  protocolVersion: number;
  requestId: string;
  action: string;
  selector?: string;
  visibleOnly: boolean;
  ok: boolean;
  payload: Record<string, unknown>;
  error?: string;
  completedAtUnixNanos: number;
}

export interface OTelBatch {
  spans: SpanRecord[];
  spanEvents: SpanEventRecord[];
  logs: LogRecord[];
  frontendErrors: FrontendErrorRecord[];
  tauriIpcCalls: TauriIpcCallRecord[];
  tauriEvents: TauriEventRecord[];
}

export interface LogRecord {
  sessionId: string;
  timestampUnixNanos: number;
  observedTimestampUnixNanos?: number;
  severityText?: string;
  severityNumber?: number;
  body?: string;
  bodyJson?: unknown;
  traceId?: string;
  spanId?: string;
  scopeName?: string;
  scopeVersion?: string;
  attributes: Record<string, unknown>;
  source: 'frontend' | 'backend' | 'plugin' | 'third_party_otel';
}

export interface SpanRecord {
  sessionId: string;
  traceId: string;
  spanId: string;
  parentSpanId?: string;
  name: string;
  kind?: string;
  startTimeUnixNanos: number;
  endTimeUnixNanos?: number;
  statusCode?: 'OK' | 'ERROR' | string;
  statusMessage?: string;
  scopeName?: string;
  scopeVersion?: string;
  attributes: Record<string, unknown>;
  source: 'frontend' | 'backend' | 'plugin' | 'third_party_otel';
}

export interface SpanEventRecord {
  sessionId: string;
  traceId: string;
  spanId: string;
  name: string;
  timestampUnixNanos: number;
  attributes: Record<string, unknown>;
}

export interface FrontendErrorRecord {
  sessionId: string;
  timestampUnixNanos: number;
  message: string;
  stack?: string;
  filename?: string;
  lineNumber?: number;
  columnNumber?: number;
  errorType?: string;
  traceId?: string;
  spanId?: string;
  windowLabel?: string;
  attributes: Record<string, unknown>;
}

export interface TauriIpcCallRecord {
  sessionId: string;
  timestampUnixNanos: number;
  durationMs?: number;
  command: string;
  status: 'OK' | 'ERROR' | string;
  errorMessage?: string;
  traceId?: string;
  spanId?: string;
  windowLabel?: string;
  argsJson?: unknown;
  argsRedacted: boolean;
  resultSummary?: string;
}

export interface TauriEventRecord {
  sessionId: string;
  timestampUnixNanos: number;
  eventName: string;
  direction: 'emit' | 'receive' | string;
  target?: string;
  traceId?: string;
  spanId?: string;
  windowLabel?: string;
  payloadSummary?: string;
  payloadJson?: unknown;
  payloadRedacted: boolean;
}
