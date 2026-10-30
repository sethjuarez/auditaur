/**
 * Native dialog stubs for `auditaur drill test`.
 *
 * Tauri defines `window.__TAURI_INTERNALS__.invoke` as non-writable, so dialogs can't be
 * intercepted below the app. Instead the app routes `@tauri-apps/plugin-dialog` through
 * {@link withDrillDialogs}. The wrapper delegates to the real plugin unless a drill armed it
 * through the drive bridge; armed, it answers from the queue drill steps fill and fails
 * (instead of blocking on an OS window) when nothing is queued.
 */

export type DrillDialogKind = 'open' | 'save' | 'ask' | 'confirm' | 'message';

const KINDS: readonly DrillDialogKind[] = ['open', 'save', 'ask', 'confirm', 'message'];

/** The functions of `@tauri-apps/plugin-dialog` that drills can answer. */
export interface DrillDialogModule {
  open(options?: never): Promise<unknown>;
  save(options?: never): Promise<unknown>;
  ask(message: string, options?: never): Promise<boolean>;
  confirm(message: string, options?: never): Promise<boolean>;
  message(message: string, options?: never): Promise<unknown>;
}

export interface QueuedDrillDialog {
  id: string;
  kind: DrillDialogKind;
  respond: unknown;
  /** A deep-partial match of the options the app passes; a mismatch rejects the dialog. */
  expect?: unknown;
}

export interface DrillDialogLogEntry {
  kind: DrillDialogKind;
  options: unknown;
  /** `answered` from the queue; `unexpected` with nothing queued; `mismatch` when the next
   * queued response was for another kind or expected other options. All but the first
   * reject the call. */
  outcome: 'answered' | 'unexpected' | 'mismatch';
  id?: string;
  expected?: DrillDialogKind;
  /** Why the options didn't match the queued `expect`. */
  problem?: string;
  atUnixMs: number;
}

export interface DrillDialogState {
  armed: boolean;
  /** Whether the app has routed its dialog module through {@link withDrillDialogs}. */
  wrapped: boolean;
  /** When the arming lapses on its own, so a drill that dies can't leave the app stubbed. */
  expiresAtUnixMs: number;
  queue: QueuedDrillDialog[];
  log: DrillDialogLogEntry[];
}

/** How long arming lasts unless the drill renews it. */
const DEFAULT_LEASE_MS = 60_000;

// Shared across copies of this package in one page, so the bridge and the wrapper agree.
const STATE_KEY = Symbol.for('auditaur.drillDialogs');

function state(): DrillDialogState {
  const holder = globalThis as unknown as Record<symbol, DrillDialogState | undefined>;
  holder[STATE_KEY] ??= { armed: false, wrapped: false, expiresAtUnixMs: 0, queue: [], log: [] };
  const current = holder[STATE_KEY];
  if (current.armed && Date.now() >= current.expiresAtUnixMs) {
    current.armed = false;
    current.queue = [];
    current.log = [];
  }
  return current;
}

/**
 * Wrap `@tauri-apps/plugin-dialog` so drills can answer its dialogs:
 *
 * ```ts
 * import * as dialog from '@tauri-apps/plugin-dialog';
 * import { withDrillDialogs } from '@auditaur/api';
 * export const { open, save, ask, confirm, message } = withDrillDialogs(dialog);
 * ```
 *
 * Outside a drill every call goes straight to the plugin.
 */
export function withDrillDialogs<T extends DrillDialogModule>(dialog: T): Pick<T, DrillDialogKind> {
  state().wrapped = true;
  return {
    open: (options?: unknown) => answer(dialog, 'open', [options], options ?? {}),
    save: (options?: unknown) => answer(dialog, 'save', [options], options ?? {}),
    ask: (message: string, options?: unknown) => answer(dialog, 'ask', [message, options], messageOptions(message, options)),
    confirm: (message: string, options?: unknown) =>
      answer(dialog, 'confirm', [message, options], messageOptions(message, options)),
    message: (message: string, options?: unknown) =>
      answer(dialog, 'message', [message, options], messageOptions(message, options)),
  } as unknown as Pick<T, DrillDialogKind>;
}

function messageOptions(message: string, options: unknown): Record<string, unknown> {
  if (typeof options === 'string') {
    return { message, title: options };
  }
  return { message, ...(options && typeof options === 'object' ? options : {}) };
}

async function answer(
  dialog: DrillDialogModule,
  kind: DrillDialogKind,
  args: unknown[],
  options: unknown,
): Promise<unknown> {
  const current = state();
  if (!current.armed) {
    return (dialog[kind] as (...args: unknown[]) => Promise<unknown>)(...args);
  }
  const recorded = plain(options);
  const next = current.queue[0];
  if (!next) {
    current.log.push({ kind, options: recorded, outcome: 'unexpected', atUnixMs: Date.now() });
    throw new Error(`Auditaur drill: the app opened a ${kind} dialog, but no response was queued`);
  }
  if (next.kind !== kind) {
    current.log.push({ kind, options: recorded, outcome: 'mismatch', expected: next.kind, id: next.id, atUnixMs: Date.now() });
    throw new Error(`Auditaur drill: the app opened a ${kind} dialog, but the next queued response is for ${next.kind}`);
  }
  const problem = next.expect === undefined ? undefined : partialMismatch(next.expect, recorded, 'options');
  if (problem) {
    current.log.push({ kind, options: recorded, outcome: 'mismatch', expected: next.kind, id: next.id, problem, atUnixMs: Date.now() });
    throw new Error(`Auditaur drill: the ${kind} dialog didn't match the queued response: ${problem}`);
  }
  current.queue.shift();
  current.log.push({ kind, options: recorded, outcome: 'answered', id: next.id, atUnixMs: Date.now() });
  return next.respond;
}

/**
 * Where `actual` fails to contain `expected`, like Playwright's `toMatchObject`: objects match
 * on the listed keys, arrays element by element with the same length, anything else exactly.
 */
export function partialMismatch(expected: unknown, actual: unknown, path: string): string | undefined {
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || actual.length !== expected.length) {
      return `${path}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual ?? null)}`;
    }
    for (let index = 0; index < expected.length; index += 1) {
      const problem = partialMismatch(expected[index], actual[index], `${path}[${index}]`);
      if (problem) {
        return problem;
      }
    }
    return undefined;
  }
  if (expected !== null && typeof expected === 'object') {
    if (actual === null || typeof actual !== 'object' || Array.isArray(actual)) {
      return `${path}: expected an object, got ${JSON.stringify(actual ?? null)}`;
    }
    for (const [key, value] of Object.entries(expected)) {
      const problem = partialMismatch(value, (actual as Record<string, unknown>)[key], `${path}.${key}`);
      if (problem) {
        return problem;
      }
    }
    return undefined;
  }
  return Object.is(expected, actual) ? undefined : `${path}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual ?? null)}`;
}

/** A JSON-safe copy, so the bridge can report what the app asked for. */
function plain(value: unknown): unknown {
  try {
    return JSON.parse(JSON.stringify(value ?? null)) as unknown;
  } catch {
    return String(value);
  }
}

/**
 * Start answering dialogs from the queue, with an empty queue and log. `lease` is the JSON
 * `{"leaseMs": n}`; arming lapses after it unless renewed by another arm or queue request.
 */
export function armDrillDialogs(lease?: string): DrillDialogState {
  const current = state();
  current.armed = true;
  current.expiresAtUnixMs = Date.now() + leaseMs(lease);
  current.queue = [];
  current.log = [];
  return drillDialogState();
}

function leaseMs(lease: string | undefined): number {
  if (!lease) {
    return DEFAULT_LEASE_MS;
  }
  const parsed = (JSON.parse(lease) as { leaseMs?: unknown }).leaseMs;
  if (typeof parsed !== 'number' || !Number.isFinite(parsed) || parsed <= 0) {
    throw new Error('`leaseMs` must be a positive number of milliseconds');
  }
  return parsed;
}

/** Queue one response and renew the lease; `entry` is the JSON of a {@link QueuedDrillDialog}
 * plus an optional `leaseMs`. */
export function queueDrillDialog(entry: string): DrillDialogState {
  const current = state();
  if (!current.armed) {
    throw new Error('dialog stubs are not armed (the page reloaded, or the lease lapsed)');
  }
  const parsed = JSON.parse(entry) as Partial<QueuedDrillDialog> & { leaseMs?: number };
  if (typeof parsed.id !== 'string' || !KINDS.includes(parsed.kind as DrillDialogKind) || !('respond' in parsed)) {
    throw new Error('a queued dialog needs `id`, `kind` (open, save, ask, confirm or message) and `respond`');
  }
  if (parsed.leaseMs !== undefined) {
    current.expiresAtUnixMs = Date.now() + leaseMs(JSON.stringify({ leaseMs: parsed.leaseMs }));
  }
  const queued: QueuedDrillDialog = { id: parsed.id, kind: parsed.kind as DrillDialogKind, respond: parsed.respond };
  if (parsed.expect !== undefined) {
    queued.expect = parsed.expect;
  }
  current.queue.push(queued);
  return drillDialogState();
}

export function drillDialogState(): DrillDialogState {
  const current = state();
  return plain(current) as DrillDialogState;
}

/** Hand dialogs back to the real plugin; returns the final state. */
export function disarmDrillDialogs(): DrillDialogState {
  const final = drillDialogState();
  const current = state();
  current.armed = false;
  current.expiresAtUnixMs = 0;
  current.queue = [];
  current.log = [];
  return final;
}
