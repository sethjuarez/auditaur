export { initAuditaur } from './init.js';
export { withDrillDialogs } from './drill-dialogs.js';
export type { DrillDialogKind, DrillDialogModule } from './drill-dialogs.js';
export { DEFAULT_REDACTION_KEYS, REDACTED, recordedConsole, recordedError, recordedErrorMessage, recordedPayload } from './payload.js';
export type { RecordedConsole, RecordedError, RecordedPayload } from './payload.js';
export {
  AUDITAUR_TRACE_CONTEXT_ARG,
  createAuditaurTraceContext,
} from './invoke.js';
export {
  createAuditaurSpanExporter,
  hrTimeToUnixNanos,
  readableSpanToSpanRecord,
} from './otel.js';
export type { AuditaurClient, AuditaurFrontendConfig } from './init.js';
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
} from './otel.js';
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
  AuditaurRedactConsole,
  AuditaurRedactErrorMessage,
  AuditaurRedactPayload,
  AuditaurTraceContextCarrier,
  DriveBridgeRequest,
  DriveBridgeResponse,
} from './types.js';
