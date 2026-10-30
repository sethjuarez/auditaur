// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest';
import { ELEMENT_DISABLED } from './actionability.js';
import { isChecked, setChecked } from './checkable.js';

describe('setChecked', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('clicks, so click-only handlers like React onChange see the change', async () => {
    document.body.innerHTML = '<input id="c" type="checkbox">';
    const box = document.getElementById('c') as HTMLInputElement;
    const log: string[] = [];
    for (const type of ['click', 'input', 'change']) box.addEventListener(type, () => log.push(type));
    await setChecked(box, true);
    expect(box.checked).toBe(true);
    expect(log).toEqual(['click', 'input', 'change']);
    await setChecked(box, false);
    expect(box.checked).toBe(false);
  });

  it('does nothing when already in the requested state, even when disabled', async () => {
    document.body.innerHTML = '<input id="c" type="checkbox" checked disabled>';
    const box = document.getElementById('c') as HTMLInputElement;
    const log: string[] = [];
    box.addEventListener('click', () => log.push('click'));
    await setChecked(box, true);
    expect(log).toEqual([]);
    await expect(setChecked(box, false)).rejects.toThrow(ELEMENT_DISABLED);
  });

  it('toggles an ARIA switch through its own click handler', async () => {
    document.body.innerHTML = '<button id="s" role="switch" aria-checked="false">Wi-Fi</button>';
    const toggle = document.getElementById('s')!;
    toggle.addEventListener('click', () =>
      toggle.setAttribute('aria-checked', String(toggle.getAttribute('aria-checked') !== 'true')));
    await setChecked(toggle, true);
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    await setChecked(toggle, false);
    expect(toggle.getAttribute('aria-checked')).toBe('false');
  });

  it('verifies after promise callbacks from the click handler run', async () => {
    document.body.innerHTML = '<div id="d" role="checkbox" aria-checked="false">Later</div>';
    const box = document.getElementById('d')!;
    box.addEventListener('click', () => void Promise.resolve().then(() => box.setAttribute('aria-checked', 'true')));
    await setChecked(box, true);
    expect(box.getAttribute('aria-checked')).toBe('true');
  });

  it('fails when the click does not change the state', async () => {
    document.body.innerHTML = '<div id="d" role="checkbox" aria-checked="false">Locked</div>';
    await expect(setChecked(document.getElementById('d')!, true)).rejects.toThrow('clicking did not check the element');
  });

  it('checks a radio but refuses to uncheck one, like Playwright', async () => {
    document.body.innerHTML = '<input id="a" type="radio" name="r"><input id="b" type="radio" name="r">';
    const a = document.getElementById('a') as HTMLInputElement;
    await setChecked(a, true);
    expect(a.checked).toBe(true);
    await expect(setChecked(a, false)).rejects.toThrow('cannot uncheck a radio button');
  });

  it.each([
    ['a mixed ARIA checkbox', '<div id="x" role="checkbox" aria-checked="mixed"></div>', false],
    ['an indeterminate input by its checked state', '<input id="x" type="checkbox" checked>', true],
  ])('reads %s', (_label, html, expected) => {
    document.body.innerHTML = html;
    const el = document.getElementById('x')!;
    if (el instanceof HTMLInputElement) el.indeterminate = true;
    expect(isChecked(el)).toBe(expected);
  });

  it('rejects elements that are not checkable', async () => {
    document.body.innerHTML = '<button id="b">B</button>';
    expect(() => isChecked(document.getElementById('b')!)).toThrow('not a checkbox, radio button or switch');
  });
});
