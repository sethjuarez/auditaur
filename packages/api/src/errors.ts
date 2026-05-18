import type { AuditaurExporter } from './exporter';
import type { FrontendErrorRecord } from './types';
import { errorRecordFields, nowUnixNanos } from './utils';

export function instrumentErrors(exporter: AuditaurExporter): () => void {
  if (typeof window === 'undefined') {
    return () => {};
  }

  const onError = (event: ErrorEvent) => {
    exporter.addFrontendError(frontendErrorRecord(event.error ?? event.message, {
      filename: event.filename,
      lineNumber: event.lineno,
      columnNumber: event.colno,
    }));
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    exporter.addFrontendError(frontendErrorRecord(event.reason));
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
): FrontendErrorRecord {
  const fields = errorRecordFields(error);
  return {
    sessionId: '',
    timestampUnixNanos: nowUnixNanos(),
    message: fields.message,
    stack: fields.stack,
    filename: location.filename,
    lineNumber: location.lineNumber,
    columnNumber: location.columnNumber,
    errorType: fields.errorType,
    attributes: { 'auditaur.source': 'frontend' },
  };
}
