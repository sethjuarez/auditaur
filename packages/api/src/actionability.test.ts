// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest';
import { ELEMENT_DISABLED, ELEMENT_NOT_EDITABLE, isReadOnly } from './actionability.js';
import { executeDriveBridgeRequest } from './drive-bridge.js';
import type { DriveBridgeRequest } from './types.js';

function request(action: string, selector: string, value?: string): DriveBridgeRequest {
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

function events(el: Element): string[] {
  const log: string[] = [];
  for (const type of ['click', 'dblclick', 'input', 'change']) {
    el.addEventListener(type, () => log.push(type));
  }
  return log;
}

describe('actionability', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it.each([
    ['click', '<button id="t" disabled>B</button>', undefined],
    ['dblclick', '<button id="t" disabled>B</button>', undefined],
    ['check', '<input id="t" type="checkbox" disabled>', undefined],
    ['uncheck', '<input id="t" type="checkbox" checked disabled>', undefined],
    ['select', '<select id="t" disabled><option value="a">A</option><option value="b">B</option></select>', 'b'],
    ['fill', '<input id="t" disabled>', 'x'],
    ['type', '<textarea id="t" disabled></textarea>', 'x'],
  ])('%s waits on a disabled target without touching it', async (action, html, value) => {
    document.body.innerHTML = html;
    const el = document.querySelector('#t') as HTMLInputElement;
    const before = { value: el.value, checked: el.checked };
    const log = events(el);

    await expect(executeDriveBridgeRequest(request(action, '#t', value))).rejects.toThrow(ELEMENT_DISABLED);
    expect(log).toEqual([]);
    expect({ value: el.value, checked: el.checked }).toEqual(before);
  });

  it('treats a disabled fieldset and an aria-disabled ancestor as disabled', async () => {
    document.body.innerHTML = `
      <fieldset disabled><input id="f"></fieldset>
      <div aria-disabled="true"><span id="a" role="button">A</span></div>`;
    await expect(executeDriveBridgeRequest(request('fill', '#f', 'x'))).rejects.toThrow(ELEMENT_DISABLED);
    await expect(executeDriveBridgeRequest(request('click', '#a'))).rejects.toThrow(ELEMENT_DISABLED);
  });

  it('treats options in a disabled optgroup or select as disabled', async () => {
    document.body.innerHTML = `
      <select><optgroup disabled><option id="g">G</option></optgroup></select>
      <select disabled><option id="s">S</option></select>
      <select><option id="ok">OK</option></select>`;
    await expect(executeDriveBridgeRequest(request('click', '#g'))).rejects.toThrow(ELEMENT_DISABLED);
    await expect(executeDriveBridgeRequest(request('click', '#s'))).rejects.toThrow(ELEMENT_DISABLED);
    await expect(executeDriveBridgeRequest(request('click', '#ok'))).resolves.toMatchObject({ ok: true });
  });

  it.each(['fill', 'type'])('%s waits on a descendant of a readonly contenteditable host', async (action) => {
    document.body.innerHTML = '<div contenteditable="true" aria-readonly="true"><span id="t">keep</span></div>';
    await expect(executeDriveBridgeRequest(request(action, '#t', 'x'))).rejects.toThrow(ELEMENT_NOT_EDITABLE);
    expect(document.body.textContent).toBe('keep');
  });

  it.each([
    ['fill', 'readonly input', '<input id="t" readonly value="keep">'],
    ['type', 'readonly textarea', '<textarea id="t" readonly>keep</textarea>'],
    ['fill', 'aria-readonly contenteditable', '<div id="t" contenteditable="true" aria-readonly="true">keep</div>'],
  ])('%s waits on a %s', async (action, _label, html) => {
    document.body.innerHTML = html;
    const el = document.querySelector('#t') as HTMLElement;
    const log = events(el);

    await expect(executeDriveBridgeRequest(request(action, '#t', 'x'))).rejects.toThrow(ELEMENT_NOT_EDITABLE);
    expect(log).toEqual([]);
    expect((el as HTMLInputElement).value ?? el.textContent).toContain('keep');
  });

  it('acts on enabled targets and hovers disabled ones, as Playwright does', async () => {
    document.body.innerHTML = '<button id="b">B</button><input id="i"><button id="d" disabled>D</button>';
    const log = events(document.querySelector('#b') as Element);

    await expect(executeDriveBridgeRequest(request('click', '#b'))).resolves.toMatchObject({ ok: true });
    await expect(executeDriveBridgeRequest(request('fill', '#i', 'hi'))).resolves.toMatchObject({ ok: true });
    await expect(executeDriveBridgeRequest(request('hover', '#d'))).resolves.toMatchObject({ ok: true });
    expect(log).toEqual(['click']);
    expect((document.querySelector('#i') as HTMLInputElement).value).toBe('hi');
  });

  it('reads readonly from the property and aria-readonly', () => {
    document.body.innerHTML = '<input id="a" readonly><input id="b"><div id="c" aria-readonly="false"></div>';
    expect(isReadOnly(document.querySelector('#a') as Element)).toBe(true);
    expect(isReadOnly(document.querySelector('#b') as Element)).toBe(false);
    expect(isReadOnly(document.querySelector('#c') as Element)).toBe(false);
  });
});
