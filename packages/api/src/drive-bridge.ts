import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ariaSnapshot } from './aria-snapshot';
import type { AuditaurExporter } from './exporter';
import {
  describeLocator,
  elementText as locatorText,
  isLocatorSelector,
  parseLocator,
  queryLocator,
  resolveLocator,
} from './locator';
import { generateLocator } from './locator-gen';
import type {
  AuditaurDriveBridgeConfig,
  DriveBridgeRequest,
  DriveBridgeResponse,
} from './types';

const REGISTER_COMMAND = 'plugin:auditaur|register_drive_bridge';
const POLL_COMMAND = 'plugin:auditaur|poll_drive_bridge_request';
const COMPLETE_COMMAND = 'plugin:auditaur|complete_drive_bridge_request';
const CAPTURE_SCREENSHOT_COMMAND = 'plugin:auditaur|capture_drive_bridge_screenshot';
const REQUEST_EVENT = 'auditaur://drive-bridge/request';
const PROTOCOL_VERSION = 1;
const SNAPSHOT_TEXT_LIMIT_CHARS = 64 * 1024;
const MAX_SCREENSHOT_DIMENSION = 2048;
const HEARTBEAT_INTERVAL_MS = 1_000;
const POLL_INTERVAL_MS = 1_000;
const POLL_TIMEOUT_MS = 35_000;

interface NativeScreenshotPayload {
  format: 'png';
  pngBase64: string;
  width: number;
  height: number;
  screenshotBackend: string;
  screenshotScope?: string;
  windowLabel?: string;
  windowTitle?: string;
  nativeWindowId?: number;
  nativeWindowTitle?: string;
  nativeAppName?: string;
  webviewScreenshotError?: string;
}

interface ScreenshotTargetRect {
  x: number;
  y: number;
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
  devicePixelRatio: number;
}

interface NativeScreenshotOptions {
  windowLabel: string | undefined;
  targetRect: ScreenshotTargetRect | undefined;
}

let nativeScreenshotInvoker: (options: NativeScreenshotOptions) => Promise<NativeScreenshotPayload> =
  ({ windowLabel, targetRect }) =>
    invoke<NativeScreenshotPayload>(CAPTURE_SCREENSHOT_COMMAND, { windowLabel, targetRect });

export function setDriveBridgeNativeScreenshotInvokerForTests(
  invoker: ((options: NativeScreenshotOptions) => Promise<NativeScreenshotPayload>) | undefined,
): void {
  nativeScreenshotInvoker = invoker
    ?? (({ windowLabel, targetRect }) =>
      invoke<NativeScreenshotPayload>(CAPTURE_SCREENSHOT_COMMAND, { windowLabel, targetRect }));
}

export function startDriveBridge(
  exporter: AuditaurExporter,
  config: boolean | AuditaurDriveBridgeConfig,
): () => void {
  const bridgeConfig = typeof config === 'boolean' ? {} : config;
  const pollIntervalMs = Math.max(bridgeConfig.pollIntervalMs ?? POLL_INTERVAL_MS, 50);
  const windowLabel = bridgeConfig.windowLabel;
  let disposed = false;
  let stopRequestListener: (() => void) | undefined;
  const isDisposed = () => disposed;
  // The timer loop and the wake event share one page-wide gate so a request is never polled concurrently.
  const tryPoll = createSingleFlightPoller(() => pollOnce(exporter, windowLabel), pageDrivePollGate);

  void register(windowLabel).catch(reportBridgeError);
  const heartbeatTimer = setInterval(() => {
    if (!disposed) {
      void register(windowLabel).catch(reportBridgeError);
    }
  }, HEARTBEAT_INTERVAL_MS);
  void pollLoop(tryPoll, isDisposed, pollIntervalMs);
  void listen(REQUEST_EVENT, () => {
    if (disposed) {
      return;
    }
    void drainPendingRequests(tryPoll, isDisposed).catch(reportBridgeError);
  })
    .then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        stopRequestListener = unlisten;
      }
    })
    .catch(reportBridgeError);

  return () => {
    disposed = true;
    clearInterval(heartbeatTimer);
    stopRequestListener?.();
  };
}

async function register(windowLabel: string | undefined): Promise<void> {
  await invoke(REGISTER_COMMAND, { windowLabel });
}

async function pollOnce(exporter: AuditaurExporter, windowLabel: string | undefined): Promise<boolean> {
  const request = await invoke<DriveBridgeRequest | null>(POLL_COMMAND, { windowLabel });
  if (!request) {
    return false;
  }
  console.debug('Auditaur drive bridge received request', {
    action: request.action,
    selector: request.selector,
    windowLabel: request.windowLabel,
  });
  const response = await responseForRequest(request, exporter);
  recordDriveBridgeTelemetry(exporter, request, response);
  await invoke(COMPLETE_COMMAND, { response });
  console.debug('Auditaur drive bridge completed request', {
    action: request.action,
    ok: response.ok,
  });
  return true;
}

export interface SingleFlightGate {
  inFlight: boolean;
}

// Shared by every bridge in this page, so duplicate initAuditaur calls (StrictMode, HMR) never poll concurrently.
const pageDrivePollGate: SingleFlightGate = { inFlight: false };

/**
 * Wraps a poll so at most one invocation per gate is in flight. Returns `undefined` when a poll
 * is already running. The gate is released when the underlying poll settles, not on timeout.
 */
export function createSingleFlightPoller(
  poll: () => Promise<boolean>,
  gate: SingleFlightGate = { inFlight: false },
): () => Promise<boolean> | undefined {
  return () => {
    if (gate.inFlight) {
      return undefined;
    }
    gate.inFlight = true;
    let pending: Promise<boolean>;
    try {
      pending = poll();
    } catch (error) {
      gate.inFlight = false;
      return Promise.reject(error);
    }
    return pending.finally(() => {
      gate.inFlight = false;
    });
  };
}

export async function drainPendingRequests(
  tryPoll: () => Promise<boolean> | undefined,
  isDisposed: () => boolean,
): Promise<void> {
  while (!isDisposed()) {
    const pending = tryPoll();
    if (!pending) {
      return;
    }
    const handled = await withTimeout(pending, POLL_TIMEOUT_MS, 'drive bridge poll timed out');
    if (!handled) {
      return;
    }
  }
}

async function pollLoop(
  tryPoll: () => Promise<boolean> | undefined,
  isDisposed: () => boolean,
  pollIntervalMs: number,
): Promise<void> {
  while (!isDisposed()) {
    await drainPendingRequests(tryPoll, isDisposed).catch(reportBridgeError);
    if (!isDisposed()) {
      await sleep(pollIntervalMs);
    }
  }
}

function sleep(delayMs: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, delayMs));
}

async function responseForRequest(
  request: DriveBridgeRequest,
  exporter: AuditaurExporter,
): Promise<DriveBridgeResponse> {
  try {
    const payload = await executeDriveBridgeRequest(request, exporter);
    return {
      schemaVersion: 1,
      protocolVersion: PROTOCOL_VERSION,
      requestId: request.requestId,
      action: request.action,
      selector: request.selector,
      visibleOnly: request.visibleOnly,
      ok: payloadOk(payload),
      payload,
      completedAtUnixNanos: nowUnixNanos(),
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return {
      schemaVersion: 1,
      protocolVersion: PROTOCOL_VERSION,
      requestId: request.requestId,
      action: request.action,
      selector: request.selector,
      visibleOnly: request.visibleOnly,
      ok: false,
      payload: { ok: false, error: message },
      error: message,
      completedAtUnixNanos: nowUnixNanos(),
    };
  }
}

export async function executeDriveBridgeRequest(
  request: DriveBridgeRequest,
  exporter?: Pick<AuditaurExporter, 'flush'>,
): Promise<Record<string, unknown>> {
  if (request.protocolVersion !== PROTOCOL_VERSION) {
    throw new Error(
      `unsupported drive bridge protocol version: ${request.protocolVersion}; expected ${PROTOCOL_VERSION}`,
    );
  }

  switch (request.action) {
    case 'ping':
      return {
        ok: true,
        protocolVersion: PROTOCOL_VERSION,
        requestId: request.requestId,
        windowLabel: request.windowLabel,
        title: document.title,
      };
    case 'exists': {
      const exists = Boolean(resolveSelector(request.selector, request.visibleOnly));
      return exists
        ? { exists, visibleOnly: request.visibleOnly }
        : { exists, visibleOnly: request.visibleOnly, error: selectorNotFoundMessage(request.selector) };
    }
    case 'text': {
      const el = resolveSelector(request.selector, request.visibleOnly);
      return el
        ? { found: true, visibleOnly: request.visibleOnly, text: elementText(el) }
        : { found: false, visibleOnly: request.visibleOnly, text: null, error: selectorNotFoundMessage(request.selector) };
    }

    case 'click': {
      const el = requireSelector(request.selector, request.visibleOnly);
      el.scrollIntoView?.({ block: 'center', inline: 'center' });
      (el as HTMLElement).click?.();
      return { ok: true, visibleOnly: request.visibleOnly };
    }
    case 'dblclick': {
      const el = requireSelector(request.selector, request.visibleOnly);
      el.scrollIntoView?.({ block: 'center', inline: 'center' });
      (el as HTMLElement).click?.();
      (el as HTMLElement).click?.();
      const DblClick = typeof MouseEvent === 'function' ? MouseEvent : Event;
      el.dispatchEvent(new DblClick('dblclick', { bubbles: true, cancelable: true, detail: 2 } as MouseEventInit));
      return { ok: true, visibleOnly: request.visibleOnly };
    }
    case 'hover': {
      const el = requireSelector(request.selector, request.visibleOnly);
      hoverElement(el);
      return { ok: true, visibleOnly: request.visibleOnly };
    }
    case 'fill': {
      const el = requireSelector(request.selector, request.visibleOnly);
      setElementValue(el, request.value ?? '');
      return { ok: true, visibleOnly: request.visibleOnly };
    }
    case 'type': {
      const el = requireSelector(request.selector, request.visibleOnly);
      const insertedCharacters = typeElementValue(el, request.value ?? '');
      return { ok: true, visibleOnly: request.visibleOnly, insertedCharacters };
    }
    case 'press': {
      pressKey(request.selector, request.value ?? '', request.visibleOnly);
      return { ok: true, visibleOnly: request.visibleOnly };
    }
    case 'select': {
      const el = requireSelector(request.selector, request.visibleOnly);
      const values = request.values?.length ? request.values : [request.value ?? ''];
      return selectElementValues(el, values, request.visibleOnly);
    }
    case 'check': {
      const el = requireSelector(request.selector, request.visibleOnly);
      setChecked(el, true);
      return { ok: true, checked: true, visibleOnly: request.visibleOnly };
    }
    case 'uncheck': {
      const el = requireSelector(request.selector, request.visibleOnly);
      setChecked(el, false);
      return { ok: true, checked: false, visibleOnly: request.visibleOnly };
    }
    case 'evaluate': {
      return evaluateExpression(request.value ?? '');
    }
    case 'snapshot': {
      return captureSnapshot(request.selector);
    }
    case 'inspect': {
      return inspectSelector(request.selector);
    }
    case 'ariaSnapshot': {
      const root = request.selector ? requireSelector(request.selector, false) : document.body;
      return { snapshot: ariaSnapshot(root) };
    }
    case 'flush': {
      if (!exporter) {
        throw new Error('flush needs the Auditaur exporter');
      }
      await exporter.flush();
      return { ok: true };
    }
    case 'screenshot': {
      return captureScreenshot(request.selector, request.windowLabel, {
        masks: request.values ?? [],
        stabilize: request.value === 'expect',
      });
    }
    case 'locate': {
      return locateSelector(request.selector);
    }
    default:
      throw new Error(`unsupported drive bridge action: ${request.action}`);
  }
}

function resolveSelector(selector: string | undefined, visibleOnly: boolean): Element | undefined {
  if (!selector) {
    return undefined;
  }
  if (isLocatorSelector(selector)) {
    return resolveLocator(parseLocator(selector), visibleOnly, isVisible);
  }
  const matches = Array.from(document.querySelectorAll(selector));
  return visibleOnly ? matches.find(isVisible) : matches[0];
}

/**
 * Everything a drill assertion needs about a target, without throwing: how many
 * elements match, a strict-mode message when the locator is ambiguous, and the
 * resolved element's state.
 */
function inspectSelector(selector: string | undefined): Record<string, unknown> {
  if (!selector) {
    throw new Error('inspect needs a selector');
  }
  let matches: Element[];
  let strict: string | undefined;
  if (isLocatorSelector(selector)) {
    const spec = parseLocator(selector);
    const all = queryLocator(spec);
    if (spec.nth !== undefined) {
      matches = pickNth(all, spec.nth);
    } else {
      matches = all;
      if (all.length > 1) {
        strict = `strict mode violation: \`${describeLocator(spec)}\` resolved to ${all.length} elements; add \`nth\` or narrow the locator`;
      }
    }
  } else {
    matches = Array.from(document.querySelectorAll(selector));
  }
  const el = strict ? undefined : matches[0];
  return {
    count: matches.length,
    ...(strict ? { strict } : {}),
    element: el
      ? {
          visible: isVisible(el),
          text: locatorText(el),
          value: valueOf(el),
          checked: checkedOf(el),
        }
      : null,
  };
}

const MAX_LOCATED = 20;

function pickNth(all: Element[], nth: number): Element[] {
  const el = nth < 0 ? all[all.length + nth] : all[nth];
  return el ? [el] : [];
}

/** Suggested drill locators for every element a CSS selector or locator matches. */
function locateSelector(selector: string | undefined): Record<string, unknown> {
  if (!selector) {
    throw new Error('locate needs a selector');
  }
  let matches: Element[];
  if (isLocatorSelector(selector)) {
    const spec = parseLocator(selector);
    const all = queryLocator(spec);
    matches = spec.nth === undefined ? all : pickNth(all, spec.nth);
  } else {
    matches = Array.from(document.querySelectorAll(selector));
  }
  return {
    count: matches.length,
    elements: matches.slice(0, MAX_LOCATED).map((el) => ({
      locator: generateLocator(el),
      tag: el.tagName.toLowerCase(),
      text: locatorText(el).slice(0, 80),
      visible: isVisible(el),
    })),
  };
}

function valueOf(el: Element): string | null {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
    return el.value;
  }
  return null;
}

function checkedOf(el: Element): boolean | 'mixed' | null {
  if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) {
    return el.indeterminate ? 'mixed' : el.checked;
  }
  const aria = el.getAttribute('aria-checked');
  if (aria === 'true' || aria === 'false') {
    return aria === 'true';
  }
  return aria === 'mixed' ? 'mixed' : null;
}

function requireSelector(selector: string | undefined, visibleOnly: boolean): Element {
  const el = resolveSelector(selector, visibleOnly);
  if (!el) {
    throw new Error('selector not found');
  }
  return el;
}

function isVisible(node: Element): boolean {
  if (node.closest('[hidden],[inert],[aria-hidden="true"]')) {
    return false;
  }
  if (!node.getClientRects().length) {
    return false;
  }
  for (let current: Element | null = node; current; current = current.parentElement) {
    const style = getComputedStyle(current);
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') {
      return false;
    }
  }
  return true;
}

function elementText(el: Element): string {
  return (el as HTMLElement).innerText ?? el.textContent ?? '';
}

function setElementValue(el: Element, value: string): void {
  (el as HTMLElement).focus?.();
  if (el instanceof HTMLTextAreaElement) {
    setNativeValue(el, HTMLTextAreaElement.prototype, value);
  } else if (el instanceof HTMLInputElement) {
    setNativeValue(el, HTMLInputElement.prototype, value);
  } else if ((el as HTMLElement).isContentEditable) {
    el.textContent = value;
  } else if ('value' in el) {
    (el as HTMLInputElement).value = value;
  } else {
    throw new Error('selector is not editable');
  }
  const input = typeof InputEvent === 'function'
    ? new InputEvent('input', { bubbles: true, cancelable: true, inputType: 'insertText', data: value })
    : new Event('input', { bubbles: true, cancelable: true });
  el.dispatchEvent(input);
  el.dispatchEvent(new Event('change', { bubbles: true }));
}

function typeElementValue(el: Element, value: string): number {
  (el as HTMLElement).focus?.();
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) {
    const start = typeof el.selectionStart === 'number' ? el.selectionStart : el.value.length;
    const end = typeof el.selectionEnd === 'number' ? el.selectionEnd : start;
    if (typeof el.setRangeText === 'function') {
      el.setRangeText(value, start, end, 'end');
    } else {
      el.value = `${el.value.slice(0, start)}${value}${el.value.slice(end)}`;
    }
  } else if ((el as HTMLElement).isContentEditable) {
    insertContentEditableText(el, value);
  } else {
    throw new Error('selector is not editable');
  }
  const input = typeof InputEvent === 'function'
    ? new InputEvent('input', { bubbles: true, cancelable: true, inputType: 'insertText', data: value })
    : new Event('input', { bubbles: true, cancelable: true });
  el.dispatchEvent(input);
  return Array.from(value).length;
}

function insertContentEditableText(el: Element, value: string): void {
  const selection = document.getSelection?.();
  if (selection?.rangeCount) {
    const range = selection.getRangeAt(0);
    if (el.contains(range.commonAncestorContainer)) {
      range.deleteContents();
      const text = document.createTextNode(value);
      range.insertNode(text);
      range.setStartAfter(text);
      range.setEndAfter(text);
      selection.removeAllRanges();
      selection.addRange(range);
      return;
    }
  }
  el.textContent = `${el.textContent ?? ''}${value}`;
}

// Module scope: a function declared inside handleRequest's switch is in its TDZ for the
// cases above it once compiled, which broke not-found replies (and so every auto-wait).
function selectorNotFoundMessage(selector: string | undefined): string {
  return selector ? `Selector \`${selector}\` was not found.` : 'Selector was not provided.';
}

const MODIFIER_FLAGS = {
  Alt: 'altKey',
  Control: 'ctrlKey',
  Meta: 'metaKey',
  Shift: 'shiftKey',
} as const;
type Modifier = keyof typeof MODIFIER_FLAGS;

/** Split a Playwright-style chord such as `Control+Shift+s` into modifiers and a key. */
export function parseKeyChord(chord: string, isMac = isMacPlatform()): { modifiers: Modifier[]; key: string } {
  // Playwright's tokenizer: `+` splits only after a token, so `+` and `Shift++` name the `+` key.
  const parts: string[] = [];
  let token = '';
  for (const char of chord) {
    if (char === '+' && token) {
      parts.push(token);
      token = '';
    } else {
      token += char;
    }
  }
  parts.push(token);
  const key = parts.pop() ?? '';
  if (!key) {
    throw new Error(`invalid key chord \`${chord}\``);
  }
  const modifiers = parts.map((part) => {
    const name = part === 'ControlOrMeta' ? (isMac ? 'Meta' : 'Control') : part;
    if (!(name in MODIFIER_FLAGS)) {
      throw new Error(`invalid key chord \`${chord}\`: unknown modifier \`${part}\``);
    }
    return name as Modifier;
  });
  return { modifiers, key };
}

function isMacPlatform(): boolean {
  return typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform ?? '');
}

function pressKey(selector: string | undefined, chord: string, visibleOnly: boolean): void {
  const target = selector
    ? requireSelector(selector, visibleOnly)
    : document.activeElement ?? document.body;
  if (!target) {
    throw new Error('selector not found');
  }
  const { modifiers, key } = parseKeyChord(chord);
  (target as HTMLElement).focus?.();
  const flags: Record<string, boolean> = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
  const send = (type: string, eventKey: string) =>
    target.dispatchEvent(new KeyboardEvent(type, { key: eventKey, ...flags, bubbles: true, cancelable: true }));
  for (const modifier of modifiers) {
    flags[MODIFIER_FLAGS[modifier]] = true;
    send('keydown', modifier);
  }
  send('keydown', key);
  send('keyup', key);
  for (const modifier of [...modifiers].reverse()) {
    flags[MODIFIER_FLAGS[modifier]] = false;
    send('keyup', modifier);
  }
}

function hoverElement(el: Element): void {
  (el as HTMLElement).scrollIntoView?.({ block: 'center', inline: 'center' });
  const rect = el.getBoundingClientRect?.();
  const clientX = Math.round((rect?.left ?? 0) + (rect?.width ?? 0) / 2);
  const clientY = Math.round((rect?.top ?? 0) + (rect?.height ?? 0) / 2);
  const eventInit = { bubbles: true, cancelable: true, clientX, clientY };
  for (const type of ['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'mousemove']) {
    const EventConstructor = type.startsWith('pointer') && typeof PointerEvent === 'function'
      ? PointerEvent
      : typeof MouseEvent === 'function' ? MouseEvent : Event;
    el.dispatchEvent(new EventConstructor(type, eventInit));
  }
}

function selectElementValues(el: Element, values: string[], visibleOnly: boolean): Record<string, unknown> {
  if (!(el instanceof HTMLSelectElement)) {
    throw new Error('selector is not a select element');
  }
  const requested = new Set(values);
  const matched = new Set<string>();
  for (const option of Array.from(el.options)) {
    const selected = requested.has(option.value);
    option.selected = selected;
    if (selected) {
      matched.add(option.value);
    }
  }
  const missingValues = values.filter((value) => !matched.has(value));
  el.dispatchEvent(new Event('input', { bubbles: true, cancelable: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return {
    ok: missingValues.length === 0,
    visibleOnly,
    selectedValues: Array.from(el.selectedOptions).map((option) => option.value),
    missingValues,
    ...(missingValues.length ? { error: `option value(s) not found: ${missingValues.join(', ')}` } : {}),
  };
}

function setChecked(el: Element, checked: boolean): void {
  if (!(el instanceof HTMLInputElement) || (el.type !== 'checkbox' && el.type !== 'radio')) {
    throw new Error('selector is not a checkbox or radio input');
  }
  const descriptor = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'checked');
  if (descriptor?.set) {
    descriptor.set.call(el, checked);
  } else {
    el.checked = checked;
  }
  el.dispatchEvent(new Event('input', { bubbles: true, cancelable: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
}

async function evaluateExpression(expression: string): Promise<Record<string, unknown>> {
  if (!expression.trim()) {
    throw new Error('evaluate requires a JavaScript expression');
  }
  const evaluate = globalThis.eval;
  const value = await evaluate(expression);
  return { ok: true, value: serializableEvaluationValue(value) };
}

function serializableEvaluationValue(value: unknown): unknown {
  if (value === undefined) {
    return null;
  }
  if (value instanceof Element) {
    return {
      element: true,
      tagName: value.tagName,
      id: value.id || undefined,
      className: value.className || undefined,
      text: clipText(elementText(value)),
    };
  }
  try {
    return JSON.parse(JSON.stringify(value));
  } catch {
    return String(value);
  }
}

function setNativeValue(
  el: HTMLInputElement | HTMLTextAreaElement,
  prototype: HTMLInputElement | HTMLTextAreaElement,
  value: string,
): void {
  const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value');
  if (descriptor?.set) {
    descriptor.set.call(el, value);
  } else {
    el.value = value;
  }
}

function captureSnapshot(
  selector: string | undefined,
  resolved?: Element,
): Record<string, unknown> {
  const selected = resolved ?? (selector ? resolveSelector(selector, false) : null);
  return {
    title: document.title,
    url: location.href,
    bodyText: clipText(document.body?.innerText ?? document.body?.textContent ?? ''),
    html: clipText(document.documentElement?.outerHTML ?? ''),
    selected: selected
      ? { selector, text: clipText(elementText(selected)), html: clipText(selected.outerHTML) }
      : selector ? { selector, found: false } : null,
    snapshotTextLimitCharacters: SNAPSHOT_TEXT_LIMIT_CHARS,
  };
}

interface ScreenshotOptions {
  /** Selectors whose matches are painted over (all matches, like Playwright's `mask`). */
  masks: string[];
  /** Finish animations and hide the caret so repeated captures are comparable. */
  stabilize: boolean;
}

async function captureScreenshot(
  selector: string | undefined,
  windowLabel: string | undefined,
  options: ScreenshotOptions = { masks: [], stabilize: false },
): Promise<Record<string, unknown>> {
  const target = selector ? resolveSelector(selector, false) : document.documentElement;
  if (!target) {
    throw new Error('selector not found');
  }
  if (selector) {
    await scrollTargetIntoViewForScreenshot(target);
  }
  const snapshot = captureSnapshot(selector, selector ? target : undefined);
  const cleanup: Array<() => void> = [];
  try {
    const prepared = options.stabilize || options.masks.length > 0;
    if (options.stabilize) {
      cleanup.push(stabilizePage());
    }
    const masked = maskElements(options.masks, cleanup);
    if (prepared) {
      await waitForScrollSettle();
    }
    const extra = prepared ? { masked, stabilized: options.stabilize } : {};
    const targetRect = selector ? screenshotTargetRect(target) : undefined;
    try {
      const nativeScreenshot = await nativeScreenshotInvoker({ windowLabel, targetRect });
      return {
        ...nativeScreenshot,
        selector,
        screenshotScope: nativeScreenshot.screenshotScope ?? (targetRect ? 'selector' : 'webview'),
        selectorRect: targetRect,
        snapshot,
        ...extra,
      };
    } catch (error) {
      return {
        ...captureDomSummaryScreenshot(target, selector, snapshot),
        nativeScreenshotError: errorMessage(error),
        ...extra,
      };
    }
  } finally {
    for (const undo of cleanup.reverse()) {
      undo();
    }
  }
}

const MASK_COLOR = '#FF00FF';
const STABILIZE_CSS = '*, *::before, *::after { transition: none !important; caret-color: transparent !important; }';

/** Finish finite animations, cancel infinite ones, and freeze transitions and the caret. */
function stabilizePage(): () => void {
  // As in Playwright: finite animations jump to their end state, infinite ones reset and
  // play again once the screenshot is taken.
  const cancelled: Animation[] = [];
  for (const animation of document.getAnimations?.() ?? []) {
    if (animation.effect?.getTiming().iterations === Infinity) {
      if (animation.playState === 'running') {
        cancelled.push(animation);
      }
      animation.cancel();
    } else {
      animation.finish();
    }
  }
  const style = document.createElement('style');
  style.setAttribute('data-auditaur-screenshot', 'stabilize');
  style.textContent = STABILIZE_CSS;
  (document.head ?? document.documentElement).appendChild(style);
  return () => {
    style.remove();
    for (const animation of cancelled) {
      try {
        animation.play();
      } catch {
        // The animation's element may be gone.
      }
    }
  };
}

/** Cover every visible match of each mask selector with a solid box; returns how many. */
function maskElements(selectors: string[], cleanup: Array<() => void>): number {
  let masked = 0;
  for (const selector of selectors) {
    const matches = isLocatorSelector(selector)
      ? queryLocator(parseLocator(selector))
      : Array.from(document.querySelectorAll(selector));
    for (const el of matches) {
      if (!isVisible(el)) {
        continue;
      }
      const rect = el.getBoundingClientRect();
      const box = document.createElement('div');
      box.setAttribute('data-auditaur-screenshot', 'mask');
      Object.assign(box.style, {
        position: 'fixed',
        left: `${rect.left}px`,
        top: `${rect.top}px`,
        width: `${rect.width}px`,
        height: `${rect.height}px`,
        background: MASK_COLOR,
        zIndex: '2147483647',
        pointerEvents: 'none',
      });
      document.documentElement.appendChild(box);
      cleanup.push(() => box.remove());
      masked += 1;
    }
  }
  return masked;
}

async function scrollTargetIntoViewForScreenshot(target: Element): Promise<void> {
  target.scrollIntoView({
    behavior: 'auto',
    block: 'center',
    inline: 'center',
  });
  await waitForScrollSettle();
}

function waitForScrollSettle(): Promise<void> {
  if (typeof requestAnimationFrame !== 'function') {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve());
    });
  });
}

function screenshotTargetRect(target: Element): ScreenshotTargetRect {
  const rect = target.getBoundingClientRect();
  return {
    x: rect.x,
    y: rect.y,
    width: rect.width,
    height: rect.height,
    viewportWidth: window.innerWidth || document.documentElement.clientWidth || 1,
    viewportHeight: window.innerHeight || document.documentElement.clientHeight || 1,
    devicePixelRatio: window.devicePixelRatio || 1,
  };
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  if (error && typeof error === 'object') {
    const record = error as Record<string, unknown>;
    const message = record.message ?? record.error;
    if (typeof message === 'string') {
      return message;
    }
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }
  return String(error);
}

function captureDomSummaryScreenshot(
  target: Element,
  selector: string | undefined,
  snapshot: Record<string, unknown>,
): Record<string, unknown> {
  const rect = target.getBoundingClientRect?.();
  const rawWidth = Math.ceil(rect?.width || document.documentElement.clientWidth || window.innerWidth || 1);
  const rawHeight = Math.ceil(rect?.height || document.documentElement.clientHeight || window.innerHeight || 1);
  const width = Math.max(1, Math.min(MAX_SCREENSHOT_DIMENSION, rawWidth));
  const height = Math.max(1, Math.min(MAX_SCREENSHOT_DIMENSION, rawHeight));
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) {
    throw new Error('2D canvas context is unavailable');
  }
  const rootStyle = getComputedStyle(document.body ?? document.documentElement);
  context.fillStyle = rootStyle.backgroundColor && rootStyle.backgroundColor !== 'rgba(0, 0, 0, 0)'
    ? rootStyle.backgroundColor
    : '#0f172a';
  context.fillRect(0, 0, width, height);
  context.fillStyle = rootStyle.color || '#f8fafc';
  context.font = '14px system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
  const lines = [
    document.title,
    location.href,
    '',
    ...wrapCanvasText(elementText(target).slice(0, 4000), Math.max(20, width - 32), context),
  ];
  let y = 24;
  for (const line of lines) {
    if (y > height - 16) {
      break;
    }
    context.fillText(line, 16, y);
    y += 20;
  }
  const png = canvas.toDataURL('image/png');
  return {
    format: 'png',
    pngBase64: png.replace(/^data:image\/png;base64,/, ''),
    width,
    height,
    selector,
    screenshotBackend: 'bridge_dom_summary_canvas',
    snapshot,
  };
}

function wrapCanvasText(text: string, maxWidth: number, context: CanvasRenderingContext2D): string[] {
  const lines: string[] = [];
  for (const paragraph of text.split(/\r?\n/)) {
    let line = '';
    for (const word of paragraph.split(/\s+/).filter(Boolean)) {
      const candidate = line ? `${line} ${word}` : word;
      if (context.measureText(candidate).width <= maxWidth) {
        line = candidate;
      } else {
        if (line) {
          lines.push(line);
        }
        line = word;
      }
    }
    lines.push(line);
  }
  return lines;
}

function clipText(value: unknown): { value: string; truncated: boolean; length: number } {
  const text = String(value ?? '');
  return {
    value: text.slice(0, SNAPSHOT_TEXT_LIMIT_CHARS),
    truncated: text.length > SNAPSHOT_TEXT_LIMIT_CHARS,
    length: text.length,
  };
}

function payloadOk(payload: Record<string, unknown>): boolean {
  if (typeof payload.ok === 'boolean') {
    return payload.ok;
  }
  if (typeof payload.exists === 'boolean') {
    return payload.exists;
  }
  if (typeof payload.found === 'boolean') {
    return payload.found;
  }
  return true;
}

function recordDriveBridgeTelemetry(
  exporter: AuditaurExporter,
  request: DriveBridgeRequest,
  response: DriveBridgeResponse,
): void {
  exporter.addLog({
    sessionId: '',
    timestampUnixNanos: response.completedAtUnixNanos,
    severityText: response.ok ? 'INFO' : 'ERROR',
    body: `auditaur drive ${request.action}`,
    attributes: {
      'auditaur.source': 'frontend',
      'auditaur.driver.backend': 'bridge',
      'auditaur.driver.action': request.action,
      'auditaur.driver.selector': request.selector,
      'auditaur.driver.visible_only': request.visibleOnly,
      'auditaur.driver.ok': response.ok,
      'auditaur.test_id': request.testId,
      'auditaur.step_id': request.stepId,
    },
    source: 'frontend',
  });
}

function nowUnixNanos(): number {
  return Date.now() * 1_000_000;
}

function reportBridgeError(error: unknown): void {
  console.warn('Auditaur drive bridge failed', error);
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}
