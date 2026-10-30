import type { AuditaurExporter } from './exporter.js';
import { payloadPolicy, recordConsole, type PayloadPolicy } from './payload.js';
import type { LogRecord } from './types.js';
import { nowUnixNanos } from './utils.js';

const LEVELS: Record<string, number> = {
  debug: 5,
  log: 9,
  info: 9,
  warn: 13,
  error: 17,
};

export function instrumentConsole(exporter: AuditaurExporter, policy: PayloadPolicy): () => void {
  const originals = new Map<keyof Console, unknown>();
  for (const level of ['debug', 'log', 'info', 'warn', 'error'] as const) {
    originals.set(level, console[level]);
    console[level] = (...args: unknown[]) => {
      (originals.get(level) as (...args: unknown[]) => void).apply(console, args);
      exporter.addLog(consoleLogRecord(level, args, policy));
    };
  }

  return () => {
    for (const [level, original] of originals) {
      (console[level] as unknown) = original;
    }
  };
}

export function consoleLogRecord(level: string, args: unknown[], policy: PayloadPolicy = payloadPolicy()): LogRecord {
  const timestamp = nowUnixNanos();
  const { body, bodyJson, error } = recordConsole(policy, level, args);
  return {
    sessionId: '',
    timestampUnixNanos: timestamp,
    observedTimestampUnixNanos: timestamp,
    severityText: level.toUpperCase(),
    severityNumber: LEVELS[level] ?? 9,
    body,
    bodyJson,
    attributes: {
      'auditaur.source': 'frontend',
      'console.level': level,
      ...(error
        ? {
            'exception.type': error.errorType,
            'exception.message': error.message,
            'exception.stacktrace': error.stack,
          }
        : {}),
    },
    source: 'frontend',
  };
}