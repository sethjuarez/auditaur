import { afterEach, describe, expect, it } from 'vitest';
import { executeDriveBridgeRequest } from './drive-bridge';
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

  constructor(readonly selector: string) {}

  closest(): FakeElement | null {
    return null;
  }

  getClientRects(): Array<{ width: number; height: number }> {
    return [{ width: 1, height: 1 }];
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
  Object.defineProperty(globalThis, 'InputEvent', { value: Event, configurable: true });
  Object.defineProperty(globalThis, 'KeyboardEvent', { value: Event, configurable: true });
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
      querySelectorAll: (selector: string) => elements.filter((element) => element.selector === selector),
      querySelector: (selector: string) => elements.find((element) => element.selector === selector) ?? null,
    },
    configurable: true,
  });
}

afterEach(() => {
  Reflect.deleteProperty(globalThis, 'document');
  Reflect.deleteProperty(globalThis, 'location');
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
