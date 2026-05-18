import type { AuditaurExporter } from './exporter';
import type { LogRecord } from './types';
import { nowUnixNanos, summarizePayload } from './utils';

const LEVELS: Record<string, number> = {
  debug: 5,
  log: 9,
  info: 9,
  warn: 13,
  error: 17,
};

export function instrumentConsole(
  exporter: AuditaurExporter,
  maxPayloadBytes: number,
): () => void {
  const originals = new Map<keyof Console, unknown>();
  for (const level of ['debug', 'log', 'info', 'warn', 'error'] as const) {
    originals.set(level, console[level]);
    console[level] = (...args: unknown[]) => {
      (originals.get(level) as (...args: unknown[]) => void).apply(console, args);
      exporter.addLog(consoleLogRecord(level, args, maxPayloadBytes));
    };
  }

  return () => {
    for (const [level, original] of originals) {
      (console[level] as unknown) = original;
    }
  };
}

export function consoleLogRecord(
  level: string,
  args: unknown[],
  maxPayloadBytes: number,
): LogRecord {
  const timestamp = nowUnixNanos();
  const firstStructuredArg = args.find((arg) => typeof arg === 'object' && arg !== null);
  return {
    sessionId: '',
    timestampUnixNanos: timestamp,
    observedTimestampUnixNanos: timestamp,
    severityText: level.toUpperCase(),
    severityNumber: LEVELS[level] ?? 9,
    body: args.map((arg) => summarizePayload(arg, maxPayloadBytes)).join(' '),
    bodyJson: firstStructuredArg,
    attributes: {
      'auditaur.source': 'frontend',
      'console.level': level,
      ...(firstStructuredArg instanceof Error
        ? {
            'exception.type': firstStructuredArg.name,
            'exception.message': firstStructuredArg.message,
            'exception.stacktrace': firstStructuredArg.stack,
          }
        : {}),
    },
    source: 'frontend',
  };
}
