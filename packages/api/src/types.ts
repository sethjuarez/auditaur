export interface AuditaurFrontendConfig {
  serviceName: string;
  serviceVersion?: string;
  instrumentConsole?: boolean;
  instrumentErrors?: boolean;
  instrumentTauriInvoke?: boolean;
  instrumentTauriEvents?: boolean;
  captureFullPayloads?: boolean;
  maxPayloadBytes?: number;
  batchIntervalMs?: number;
  maxBatchSize?: number;
}

export interface AuditaurClient {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  emit(event: string, payload?: unknown): Promise<void>;
  emitTo(target: string, event: string, payload?: unknown): Promise<void>;
  listen<T>(event: string, handler: (event: AuditaurEvent<T>) => void): Promise<() => void>;
  flush(): Promise<void>;
  shutdown(): Promise<void>;
}

export interface AuditaurEvent<T> {
  event: string;
  id: number;
  payload: T;
}

export interface OTelBatch {
  spans: SpanRecord[];
  logs: LogRecord[];
  frontendErrors: FrontendErrorRecord[];
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
