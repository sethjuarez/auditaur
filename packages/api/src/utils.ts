export function nowUnixNanos(): number {
  return Date.now() * 1_000_000;
}

export function randomTraceId(): string {
  return randomHex(16);
}

export function randomSpanId(): string {
  return randomHex(8);
}

export function summarizePayload(value: unknown, maxBytes: number): string {
  if (value === undefined) {
    return 'undefined';
  }
  if (value instanceof Error) {
    return truncateUtf8(`${value.name}: ${value.message}${value.stack ? `\n${value.stack}` : ''}`, maxBytes);
  }
  const text = typeof value === 'string' ? value : safeJson(value);
  return truncateUtf8(text, maxBytes);
}

export function maybePayload(value: unknown, captureFullPayloads: boolean, maxBytes: number): unknown {
  if (!captureFullPayloads) {
    return undefined;
  }
  return JSON.parse(safeJson(value, maxBytes));
}

export function errorRecordFields(error: unknown): {
  message: string;
  stack?: string;
  errorType?: string;
} {
  if (error instanceof Error) {
    return {
      message: error.message,
      stack: error.stack,
      errorType: error.name,
    };
  }
  return {
    message: typeof error === 'string' ? error : safeJson(error),
    errorType: typeof error,
  };
}

function randomHex(bytes: number): string {
  const buffer = new Uint8Array(bytes);
  crypto.getRandomValues(buffer);
  return Array.from(buffer, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function safeJson(value: unknown, maxBytes = 16_384): string {
  try {
    return truncateUtf8(JSON.stringify(value), maxBytes);
  } catch {
    return String(value);
  }
}

function truncateUtf8(value: string, maxBytes: number): string {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const bytes = encoder.encode(value);
  if (bytes.length <= maxBytes) {
    return value;
  }
  return decoder.decode(bytes.slice(0, maxBytes));
}
