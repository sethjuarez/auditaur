export { initAuditaur } from './init';
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
  FrontendErrorRecord,
  LogRecord,
  OTelBatch,
  SpanRecord,
  TauriEventRecord,
  TauriIpcCallRecord,
  AuditaurExportFailure,
  AuditaurInvokeArgs,
  AuditaurTraceContextCarrier,
} from './types';
