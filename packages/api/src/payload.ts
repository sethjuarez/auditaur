import type {
  AuditaurFrontendConfig,
  AuditaurPayloadKind,
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
}

export function payloadPolicy(config: Partial<AuditaurFrontendConfig> = {}): PayloadPolicy {
  return {
    maxBytes: config.maxPayloadBytes ?? 16_384,
    full: config.captureFullPayloads ?? false,
    summaries: config.payloadSummaries ?? true,
    keys: new Set([...DEFAULT_REDACTION_KEYS, ...(config.redactKeys ?? [])].map((key) => key.toLowerCase())),
    hook: config.redactPayload,
    errorHook: config.redactErrorMessage,
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