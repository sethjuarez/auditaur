import type { AuditaurExporter } from './exporter.js';
import { payloadPolicy, recordError, type PayloadPolicy } from './payload.js';
import type { FrontendErrorRecord } from './types.js';
import { currentWindowLabel, nowUnixNanos } from './utils.js';

export function instrumentErrors(exporter: AuditaurExporter, policy: PayloadPolicy): () => void {
  if (typeof window === 'undefined') {
    return () => {};
  }

  const onError = (event: ErrorEvent) => {
    exporter.addFrontendError(frontendErrorRecord(event.error ?? event.message, {
      filename: event.filename,
      lineNumber: event.lineno,
      columnNumber: event.colno,
    }, policy, 'window.error'));
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    exporter.addFrontendError(frontendErrorRecord(event.reason, {}, policy, 'unhandledrejection'));
  };

  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onUnhandledRejection);

  return () => {
    window.removeEventListener('error', onError);
    window.removeEventListener('unhandledrejection', onUnhandledRejection);
  };
}

export function frontendErrorRecord(
  error: unknown,
  location: { filename?: string; lineNumber?: number; columnNumber?: number } = {},
  policy: PayloadPolicy = payloadPolicy(),
  name = 'window.error',
): FrontendErrorRecord {
  const fields = recordError(policy, name, error);
  return {
    sessionId: '',
    timestampUnixNanos: nowUnixNanos(),
    message: fields.message,
    stack: fields.stack,
    filename: location.filename,
    lineNumber: location.lineNumber,
    columnNumber: location.columnNumber,
    errorType: fields.errorType,
    windowLabel: currentWindowLabel(),
    attributes: { 'auditaur.source': 'frontend' },
  };
}