import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AuditaurExporter } from './exporter';
import type {
  AuditaurDriveBridgeConfig,
  DriveBridgeRequest,
  DriveBridgeResponse,
} from './types';

const REGISTER_COMMAND = 'plugin:auditaur|register_drive_bridge';
const POLL_COMMAND = 'plugin:auditaur|poll_drive_bridge_request';
const COMPLETE_COMMAND = 'plugin:auditaur|complete_drive_bridge_request';
const REQUEST_EVENT = 'auditaur://drive-bridge/request';
const PROTOCOL_VERSION = 1;
const SNAPSHOT_TEXT_LIMIT_CHARS = 64 * 1024;
const MAX_SCREENSHOT_DIMENSION = 2048;
const HEARTBEAT_INTERVAL_MS = 1_000;
const POLL_INTERVAL_MS = 1_000;
const POLL_TIMEOUT_MS = 35_000;

export function startDriveBridge(
  exporter: AuditaurExporter,
  config: boolean | AuditaurDriveBridgeConfig,
): () => void {
  const bridgeConfig = typeof config === 'boolean' ? {} : config;
  const pollIntervalMs = Math.max(bridgeConfig.pollIntervalMs ?? POLL_INTERVAL_MS, 50);
  const windowLabel = bridgeConfig.windowLabel;
  let disposed = false;
  let stopRequestListener: (() => void) | undefined;
  let requestWakePollRunning = false;

  void register(windowLabel).catch(reportBridgeError);
  const heartbeatTimer = setInterval(() => {
    if (!disposed) {
      void register(windowLabel).catch(reportBridgeError);
    }
  }, HEARTBEAT_INTERVAL_MS);
  void pollLoop(exporter, windowLabel, () => disposed, pollIntervalMs);
  void listen(REQUEST_EVENT, () => {
    if (disposed || requestWakePollRunning) {
      return;
    }
    requestWakePollRunning = true;
    void drainPendingRequests(exporter, windowLabel, () => disposed)
      .catch(reportBridgeError)
      .finally(() => {
        requestWakePollRunning = false;
      });
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
  const response = await responseForRequest(request);
  recordDriveBridgeTelemetry(exporter, request, response);
  await invoke(COMPLETE_COMMAND, { response });
  console.debug('Auditaur drive bridge completed request', {
    action: request.action,
    ok: response.ok,
  });
  return true;
}

async function drainPendingRequests(
  exporter: AuditaurExporter,
  windowLabel: string | undefined,
  isDisposed: () => boolean,
): Promise<void> {
  while (!isDisposed()) {
    const handled = await withTimeout(
      pollOnce(exporter, windowLabel),
      POLL_TIMEOUT_MS,
      'drive bridge wake poll timed out',
    );
    if (!handled) {
      return;
    }
  }
}

async function pollLoop(
  exporter: AuditaurExporter,
  windowLabel: string | undefined,
  isDisposed: () => boolean,
  pollIntervalMs: number,
): Promise<void> {
  while (!isDisposed()) {
    await withTimeout(pollOnce(exporter, windowLabel), POLL_TIMEOUT_MS, 'drive bridge poll timed out')
      .catch(reportBridgeError);
    if (!isDisposed()) {
      await sleep(pollIntervalMs);
    }
  }
}

function sleep(delayMs: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, delayMs));
}

async function responseForRequest(request: DriveBridgeRequest): Promise<DriveBridgeResponse> {
  try {
    const payload = await executeDriveBridgeRequest(request);
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

export async function executeDriveBridgeRequest(request: DriveBridgeRequest): Promise<Record<string, unknown>> {
  if (request.protocolVersion !== PROTOCOL_VERSION) {
    throw new Error(
      `unsupported drive bridge protocol version: ${request.protocolVersion}; expected ${PROTOCOL_VERSION}`,
    );
  }

  switch (request.action) {
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

    function selectorNotFoundMessage(selector: string | undefined): string {
      return selector ? `Selector \`${selector}\` was not found.` : 'Selector was not provided.';
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
    case 'snapshot': {
      return captureSnapshot(request.selector);
    }
    case 'screenshot': {
      return captureScreenshot(request.selector);
    }
    default:
      throw new Error(`unsupported drive bridge action: ${request.action}`);
  }
}

function resolveSelector(selector: string | undefined, visibleOnly: boolean): Element | undefined {
  if (!selector) {
    return undefined;
  }
  const matches = Array.from(document.querySelectorAll(selector));
  return visibleOnly ? matches.find(isVisible) : matches[0];
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

function pressKey(selector: string | undefined, key: string, visibleOnly: boolean): void {
  const target = selector
    ? requireSelector(selector, visibleOnly)
    : document.activeElement ?? document.body;
  if (!target) {
    throw new Error('selector not found');
  }
  (target as HTMLElement).focus?.();
  for (const type of ['keydown', 'keyup']) {
    target.dispatchEvent(new KeyboardEvent(type, { key, bubbles: true, cancelable: true }));
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

function captureSnapshot(selector: string | undefined): Record<string, unknown> {
  const selected = selector ? document.querySelector(selector) : null;
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

async function captureScreenshot(selector: string | undefined): Promise<Record<string, unknown>> {
  const target = selector ? document.querySelector(selector) : document.documentElement;
  if (!target) {
    throw new Error('selector not found');
  }
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
    snapshot: captureSnapshot(selector),
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
