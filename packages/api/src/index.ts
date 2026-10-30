export { initAuditaur } from './init';
export { DEFAULT_REDACTION_KEYS } from './payload';
export {
  AUDITAUR_TRACE_CONTEXT_ARG,
  createAuditaurTraceContext,
} from './invoke';
export {
  createAuditaurSpanExporter,
  hrTimeToUnixNanos,
  readableSpanToSpanRecord,
} from './otel';
export type { AuditaurClient, AuditaurFrontendConfig } from './init';
export type {
  AuditaurOpenTelemetrySpanExporterOptions,
  AuditaurSpanSink,
  ExportResultLike,
  HrTimeLike,
  LinkLike,
  OpenTelemetrySpanExporter,
  ReadableSpanLike,
  SpanContextLike,
  TimedEventLike,
} from './otel';
export type {
  AuditaurDriveBridgeConfig,
  FrontendErrorRecord,
  LogRecord,
  OTelBatch,
  SpanEventRecord,
  SpanRecord,
  TauriEventRecord,
  TauriIpcCallRecord,
  AuditaurExportFailure,
  AuditaurInvokeArgs,
  AuditaurPayloadKind,
  AuditaurRedactPayload,
  AuditaurTraceContextCarrier,
  DriveBridgeRequest,
  DriveBridgeResponse,
} from './types';
