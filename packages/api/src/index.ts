export { initAuditaur } from './init';
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
} from './types';
