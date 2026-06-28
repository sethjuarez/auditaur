import { afterEach, describe, expect, it } from 'vitest';
import {
  executeDriveBridgeRequest,
  setDriveBridgeNativeScreenshotInvokerForTests,
} from './drive-bridge';
import type { DriveBridgeRequest } from './types';

class FakeElement {
  parentElement: FakeElement | null = null;
  textContent = '';
  innerText = '';
  outerHTML = '<div></div>';
  isContentEditable = false;
  clicked = false;
  dispatched: string[] = [];
  value = '';
  selectionStart: number | null = null;
  selectionEnd: number | null = null;
  tagName = 'DIV';
  id = '';
  className = '';

  constructor(readonly selector: string) {}

  closest(): FakeElement | null {
    return null;
  }

  getClientRects(): Array<{ width: number; height: number }> {
    return [{ width: 1, height: 1 }];
  }

  getBoundingClientRect(): { x: number; y: number; left: number; top: number; width: number; height: number } {
    return { x: 0, y: 0, left: 0, top: 0, width: 10, height: 10 };
  }

  scrollIntoView(): void {}

  click(): void {
    this.clicked = true;
  }

  focus(): void {}

  dispatchEvent(event: Event): boolean {
    this.dispatched.push(event.type);
    return true;
  }

  setRangeText(value: string, start: number, end: number): void {
    this.value = `${this.value.slice(0, start)}${value}${this.value.slice(end)}`;
    this.selectionStart = start + value.length;
    this.selectionEnd = this.selectionStart;
  }
}

class FakeInputElement extends FakeElement {}
class FakeTextAreaElement extends FakeElement {}
class FakeSelectElement extends FakeElement {
  options: Array<{ value: string; selected: boolean }> = [];

  get selectedOptions(): Array<{ value: string; selected: boolean }> {
    return this.options.filter((option) => option.selected);
  }
}

class FakeCanvas {
  width = 0;
  height = 0;

  getContext(): { fillStyle: string; font: string; fillRect: () => void; fillText: () => void; measureText: (text: string) => { width: number } } {
    return {
      fillStyle: '',
      font: '',
      fillRect: () => {},
      fillText: () => {},
      measureText: (text: string) => ({ width: text.length * 8 }),
    };
  }

  toDataURL(): string {
    return 'data:image/png;base64,ZmFrZS1wbmc=';
  }
}

function request(action: string, selector = '#target', value?: string): DriveBridgeRequest {
  return {
    schemaVersion: 1,
    protocolVersion: 1,
    requestId: 'request-1',
    action,
    selector,
    value,
    visibleOnly: false,
    createdAtUnixNanos: 1,
  };
}

function installDom(elements: FakeElement[]): void {
  Object.defineProperty(globalThis, 'Element', { value: FakeElement, configurable: true });
  Object.defineProperty(globalThis, 'HTMLElement', { value: FakeElement, configurable: true });
  Object.defineProperty(globalThis, 'HTMLInputElement', { value: FakeInputElement, configurable: true });
  Object.defineProperty(globalThis, 'HTMLTextAreaElement', { value: FakeTextAreaElement, configurable: true });
  Object.defineProperty(globalThis, 'HTMLSelectElement', { value: FakeSelectElement, configurable: true });
  Object.defineProperty(globalThis, 'InputEvent', { value: Event, configurable: true });
  Object.defineProperty(globalThis, 'KeyboardEvent', { value: Event, configurable: true });
  Object.defineProperty(globalThis, 'MouseEvent', { value: Event, configurable: true });
  Object.defineProperty(globalThis, 'PointerEvent', { value: Event, configurable: true });
  Object.defineProperty(globalThis, 'getComputedStyle', {
    value: () => ({ display: 'block', visibility: 'visible' }),
    configurable: true,
  });
  Object.defineProperty(globalThis, 'location', {
    value: { href: 'tauri://localhost/' },
    configurable: true,
  });
  Object.defineProperty(globalThis, 'document', {
    value: {
      title: 'Dogfood',
      body: { innerText: 'Ready', textContent: 'Ready' },
      documentElement: { outerHTML: '<html><body>Ready</body></html>' },
      createElement: (tagName: string) => {
        if (tagName === 'canvas') {
          return new FakeCanvas();
        }
        return new FakeElement(tagName);
      },
      querySelectorAll: (selector: string) => elements.filter((element) => element.selector === selector),
      querySelector: (selector: string) => elements.find((element) => element.selector === selector) ?? null,
    },
    configurable: true,
  });
  Object.defineProperty(globalThis, 'window', {
    value: { innerWidth: 800, innerHeight: 600, devicePixelRatio: 1 },
    configurable: true,
  });
}

afterEach(() => {
  setDriveBridgeNativeScreenshotInvokerForTests(undefined);
  Reflect.deleteProperty(globalThis, 'document');
  Reflect.deleteProperty(globalThis, 'location');
  Reflect.deleteProperty(globalThis, 'window');
});

describe('drive bridge selector operations', () => {
  it('checks existence and reads text', async () => {
    const target = new FakeElement('#target');
    target.innerText = 'Bridge ready';
    installDom([target]);

    await expect(executeDriveBridgeRequest(request('exists'))).resolves.toEqual({
      exists: true,
      visibleOnly: false,
    });
    await expect(executeDriveBridgeRequest(request('text'))).resolves.toEqual({
      found: true,
      visibleOnly: false,
      text: 'Bridge ready',
    });
  });

  it('clicks and fills selector targets', async () => {
    const button = new FakeElement('#button');
    const input = new FakeInputElement('#input');
    installDom([button, input]);

    await expect(executeDriveBridgeRequest(request('click', '#button'))).resolves.toEqual({
      ok: true,
      visibleOnly: false,
    });
    expect(button.clicked).toBe(true);

    await expect(executeDriveBridgeRequest(request('fill', '#input', 'hello'))).resolves.toEqual({
      ok: true,
      visibleOnly: false,
    });
    expect(input.value).toBe('hello');
    expect(input.dispatched).toEqual(['input', 'change']);
  });

  it('types text and dispatches key presses', async () => {
    const input = new FakeInputElement('#input');
    input.value = 'hello ';
    input.selectionStart = 6;
    input.selectionEnd = 6;
    const button = new FakeElement('#button');
    installDom([input, button]);

    await expect(executeDriveBridgeRequest(request('type', '#input', 'bridge'))).resolves.toEqual({
      ok: true,
      visibleOnly: false,
      insertedCharacters: 6,
    });
    expect(input.value).toBe('hello bridge');
    expect(input.dispatched).toEqual(['input']);

    await expect(executeDriveBridgeRequest(request('press', '#button', 'Enter'))).resolves.toEqual({
      ok: true,
      visibleOnly: false,
    });
    expect(button.dispatched).toEqual(['keydown', 'keyup']);
  });

  it('hovers, selects options, and toggles checked inputs', async () => {
    const button = new FakeElement('#button');
    const select = new FakeSelectElement('#select');
    select.options = [
      { value: 'one', selected: false },
      { value: 'two', selected: false },
    ];
    const input = new FakeInputElement('#check') as FakeInputElement & { checked: boolean; type: string };
    input.type = 'checkbox';
    input.checked = false;
    installDom([button, select, input]);

    await expect(executeDriveBridgeRequest(request('hover', '#button'))).resolves.toEqual({
      ok: true,
      visibleOnly: false,
    });
    expect(button.dispatched).toEqual(['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'mousemove']);

    const selected = await executeDriveBridgeRequest({
      ...request('select', '#select', 'two'),
      values: ['two'],
    });
    expect(selected).toEqual({
      ok: true,
      visibleOnly: false,
      selectedValues: ['two'],
      missingValues: [],
    });

    await expect(executeDriveBridgeRequest(request('check', '#check'))).resolves.toEqual({
      ok: true,
      checked: true,
      visibleOnly: false,
    });
    expect(input.checked).toBe(true);

    await expect(executeDriveBridgeRequest(request('uncheck', '#check'))).resolves.toEqual({
      ok: true,
      checked: false,
      visibleOnly: false,
    });
    expect(input.checked).toBe(false);
  });

  it('evaluates JavaScript expressions', async () => {
    installDom([]);

    await expect(executeDriveBridgeRequest(request('evaluate', '', '1 + 2'))).resolves.toEqual({
      ok: true,
      value: 3,
    });
  });

  it('responds to selector-independent ping requests', async () => {
    installDom([]);

    const response = await executeDriveBridgeRequest({
      ...request('ping'),
      requestId: 'ping-1',
      windowLabel: 'main',
    });

    expect(response).toEqual({
      ok: true,
      protocolVersion: 1,
      requestId: 'ping-1',
      windowLabel: 'main',
      title: 'Dogfood',
    });
  });

  it('captures a bounded DOM snapshot', async () => {
    const target = new FakeElement('#target');
    target.innerText = 'Selected text';
    target.outerHTML = '<button id="target">Selected text</button>';
    installDom([target]);

    const snapshot = await executeDriveBridgeRequest(request('snapshot'));

    expect(snapshot.title).toBe('Dogfood');
    expect(snapshot.url).toBe('tauri://localhost/');
    expect(snapshot.selected).toEqual({
      selector: '#target',
      text: { value: 'Selected text', truncated: false, length: 13 },
      html: {
        value: '<button id="target">Selected text</button>',
        truncated: false,
        length: 42,
      },
    });
  });

  it('prefers native window screenshots', async () => {
    const target = new FakeElement('#target');
    target.innerText = 'Selected text';
    installDom([target]);
    let invokedWindowLabel: string | undefined;
    setDriveBridgeNativeScreenshotInvokerForTests(async ({ windowLabel, targetRect }) => {
      invokedWindowLabel = windowLabel;
      expect(targetRect).toEqual({
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        viewportWidth: 800,
        viewportHeight: 600,
        devicePixelRatio: 1,
      });
      return {
        format: 'png',
        pngBase64: 'bmF0aXZlLXBuZw==',
        width: 640,
        height: 480,
        screenshotBackend: 'tauri_native_webview_snapshot',
        screenshotScope: 'selector',
        windowLabel: 'main',
        windowTitle: 'Dogfood',
      };
    });

    const screenshot = await executeDriveBridgeRequest({
      ...request('screenshot', '#target'),
      windowLabel: 'main',
    });

    expect(invokedWindowLabel).toBe('main');
    expect(screenshot).toMatchObject({
      format: 'png',
      pngBase64: 'bmF0aXZlLXBuZw==',
      width: 640,
      height: 480,
      selector: '#target',
      screenshotBackend: 'tauri_native_webview_snapshot',
      screenshotScope: 'selector',
      selectorRect: {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        viewportWidth: 800,
        viewportHeight: 600,
        devicePixelRatio: 1,
      },
    });
    expect(screenshot.snapshot).toMatchObject({
      title: 'Dogfood',
      selected: { selector: '#target' },
    });
  });

  it('falls back to DOM summary screenshots when native capture fails', async () => {
    const target = new FakeElement('#target');
    target.innerText = 'Selected text';
    installDom([target]);
    setDriveBridgeNativeScreenshotInvokerForTests(async () => {
      throw new Error('screen recording permission denied');
    });

    const screenshot = await executeDriveBridgeRequest(request('screenshot', '#target'));

    expect(screenshot).toMatchObject({
      format: 'png',
      pngBase64: 'ZmFrZS1wbmc=',
      width: 10,
      height: 10,
      selector: '#target',
      screenshotBackend: 'bridge_dom_summary_canvas',
      nativeScreenshotError: 'screen recording permission denied',
    });
  });

  it('preserves object-shaped native screenshot errors', async () => {
    const target = new FakeElement('#target');
    target.innerText = 'Selected text';
    installDom([target]);
    setDriveBridgeNativeScreenshotInvokerForTests(async () => {
      throw { message: 'no matching native window' };
    });

    const screenshot = await executeDriveBridgeRequest(request('screenshot', '#target'));

    expect(screenshot).toMatchObject({
      screenshotBackend: 'bridge_dom_summary_canvas',
      nativeScreenshotError: 'no matching native window',
    });
  });

  it('rejects incompatible protocol versions', async () => {
    installDom([]);

    await expect(
      executeDriveBridgeRequest({
        ...request('exists'),
        protocolVersion: 2,
      }),
    ).rejects.toThrow('unsupported drive bridge protocol version: 2; expected 1');
  });
});
