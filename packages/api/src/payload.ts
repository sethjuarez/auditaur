import type {
  AuditaurFrontendConfig,
  AuditaurPayloadKind,
  AuditaurRedactConsole,
  AuditaurRedactErrorMessage,
  AuditaurRedactPayload,
} from './types.js';
import { errorRecordFields, maybePayload, summarizePayload } from './utils.js';

/** Keep in sync with `DEFAULT_REDACTION_KEYS` in crates/auditaur-core/src/redaction.rs (a test checks). */
export const DEFAULT_REDACTION_KEYS = [
  'password',
  'passwd',
  'pwd',
  'secret',
  'token',
  'access_token',
  'refresh_token',
  'id_token',
  'authorization',
  'api_key',
  'apikey',
  'key',
  'cookie',
  'set-cookie',
  'session',
  'credential',
  'connection_string',
] as const;

export const REDACTED = '[REDACTED]';

/** What gets recorded about invoke args/results and event payloads, decided once at init. */
export interface PayloadPolicy {
  readonly maxBytes: number;
  readonly full: boolean;
  readonly summaries: boolean;
  readonly keys: ReadonlySet<string>;
  readonly hook?: AuditaurRedactPayload;
  readonly errorHook?: AuditaurRedactErrorMessage;
  readonly consoleHook?: AuditaurRedactConsole;
}

export function payloadPolicy(config: Partial<AuditaurFrontendConfig> = {}): PayloadPolicy {
  return {
    maxBytes: config.maxPayloadBytes ?? 16_384,
    full: config.captureFullPayloads ?? false,
    summaries: config.payloadSummaries ?? true,
    keys: new Set([...DEFAULT_REDACTION_KEYS, ...(config.redactKeys ?? [])].map((key) => key.toLowerCase())),
    hook: config.redactPayload,
    errorHook: config.redactErrorMessage,
    consoleHook: config.redactConsole,
  };
}

/** The payload as it may be recorded: run through the app's hook, then key redaction. */
export interface RecordedPayload {
  summary?: string;
  json?: unknown;
}

export function recordPayload(
  policy: PayloadPolicy,
  kind: AuditaurPayloadKind,
  name: string,
  value: unknown,
): RecordedPayload {
  if (!policy.summaries && !policy.full) {
    return {};
  }
  // Fail closed: telemetry must never throw into the app or leak what a broken hook or getter was hiding.
  try {
    const shown = redactKeys(policy.hook ? policy.hook(value, { kind, name }) : value, policy.keys);
    return {
      summary: policy.summaries ? summarizePayload(shown, policy.maxBytes) : undefined,
      json: maybePayload(shown, policy.full, policy.maxBytes),
    };
  } catch {
    return { summary: policy.summaries ? REDACTED : undefined, json: policy.full ? REDACTED : undefined };
  }
}

/** A failed invoke's or emit's error message as it may be recorded: key redaction (object errors), then the app's hook. */
export function recordErrorMessage(policy: PayloadPolicy, name: string, error: unknown): string {
  try {
    const message = errorRecordFields(redactKeys(error, policy.keys)).message;
    if (!policy.errorHook) {
      return message;
    }
    const shown = policy.errorHook(message, { name });
    return typeof shown === 'string' ? shown : REDACTED;
  } catch {
    return REDACTED;
  }
}

/** What recording would keep of `value` under `config`, for testing an app's redaction settings. */
export function recordedPayload(
  config: Partial<AuditaurFrontendConfig>,
  kind: AuditaurPayloadKind,
  name: string,
  value: unknown,
): RecordedPayload {
  return recordPayload(payloadPolicy(config), kind, name, value);
}

/** The message recording would keep when command or event `name` fails with `error` under `config`. */
export function recordedErrorMessage(config: Partial<AuditaurFrontendConfig>, name: string, error: unknown): string {
  return recordErrorMessage(payloadPolicy(config), name, error);
}

/** An uncaught or logged error as it may be recorded. */
export interface RecordedError {
  message: string;
  stack?: string;
  errorType?: string;
}

/**
 * An error as it may be recorded: its message goes through {@link recordErrorMessage}, and the stack
 * keeps only frames once the message is rewritten (V8 stacks repeat the message on their first line).
 */
export function recordError(policy: PayloadPolicy, name: string, error: unknown): RecordedError {
  let fields: ReturnType<typeof errorRecordFields>;
  try {
    fields = errorRecordFields(error);
  } catch {
    return { message: REDACTED };
  }
  const message = recordErrorMessage(policy, name, error);
  if (message === fields.message || !fields.stack) {
    return { message, stack: fields.stack, errorType: fields.errorType };
  }
  // Fail closed: a stack that doesn't visibly contain the original message may hide it elsewhere.
  const stack = fields.message && fields.stack.includes(fields.message)
    ? fields.stack.replace(fields.message, () => message)
    : undefined;
  return { message, stack, errorType: fields.errorType };
}

/** What recording would keep of an uncaught error, a rejection, or an error passed to `console`. */
export function recordedError(config: Partial<AuditaurFrontendConfig>, name: string, error: unknown): RecordedError {
  return recordError(payloadPolicy(config), name, error);
}

/** A console call as it may be recorded. */
export interface RecordedConsole {
  body: string;
  bodyJson?: unknown;
  error?: RecordedError;
}

/**
 * A console call as it may be recorded: each argument key-redacted and summarized, then the app's
 * `redactConsole` hook over the joined text. The first object argument is kept as JSON only with
 * `captureFullPayloads`, and an Error argument goes through {@link recordError}.
 */
export function recordConsole(policy: PayloadPolicy, level: string, args: unknown[]): RecordedConsole {
  const name = `console.${level}`;
  const errors = new Map<Error, RecordedError>();
  for (const arg of args) {
    if (arg instanceof Error && !errors.has(arg)) errors.set(arg, recordError(policy, name, arg));
  }
  const error = errors.values().next().value;
  try {
    const shown = args.map((arg) => (arg instanceof Error ? errorText(errors.get(arg)!) : redactKeys(arg, policy.keys)));
    let body = shown.map((arg) => summarizePayload(arg, policy.maxBytes)).join(' ');
    if (policy.consoleHook) {
      const hooked = policy.consoleHook(body, { level });
      body = typeof hooked === 'string' ? hooked : REDACTED;
    }
    const structured = shown.find((arg) => typeof arg === 'object' && arg !== null);
    return {
      body,
      bodyJson: policy.full && structured !== undefined ? maybePayload(structured, true, policy.maxBytes) : undefined,
      error,
    };
  } catch {
    return { body: REDACTED, error };
  }
}

/** What recording would keep of `console[level](...args)` under `config`. */
export function recordedConsole(config: Partial<AuditaurFrontendConfig>, level: string, args: unknown[]): RecordedConsole {
  return recordConsole(payloadPolicy(config), level, args);
}

// Matches how an Error argument was always summarized: `Type: message` then the stack.
function errorText(recorded: RecordedError): string {
  return `${recorded.errorType ?? 'Error'}: ${recorded.message}${recorded.stack ? `\n${recorded.stack}` : ''}`;
}

/** Objects and arrays visited before the rest of a payload is recorded as `[Truncated]`. */
export const MAX_REDACTION_NODES = 10_000;

/** Replace the values of matching keys (case-insensitive, any depth), walking values the way JSON.stringify does. */
export function redactKeys(value: unknown, keys: ReadonlySet<string>): unknown {
  return walk(value, keys, { seen: new WeakSet<object>(), budget: MAX_REDACTION_NODES });
}

function walk(value: unknown, keys: ReadonlySet<string>, state: { seen: WeakSet<object>; budget: number }): unknown {
  if (value === null || typeof value !== 'object' || value instanceof Error) {
    return value;
  }
  if (state.seen.has(value)) {
    return '[Circular]';
  }
  if (state.budget-- <= 0) {
    return '[Truncated]';
  }
  state.seen.add(value);
  try {
    const toJSON = (value as { toJSON?: unknown }).toJSON;
    if (typeof toJSON === 'function') {
      return walk(toJSON.call(value), keys, state);
    }
    if (Array.isArray(value)) {
      return value.map((item) => walk(item, keys, state));
    }
    const fields: Record<string, unknown> = {};
    for (const [key, item] of Object.entries(value)) {
      fields[key] = keys.has(key.toLowerCase()) ? REDACTED : walk(item, keys, state);
    }
    return fields;
  } finally {
    state.seen.delete(value);
  }
}