// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest';
import { OPTION_DISABLED, OPTION_NOT_FOUND, selectOptions } from './select.js';

function select(html: string): HTMLSelectElement {
  document.body.innerHTML = html;
  return document.querySelector('select')!;
}

describe('selectOptions', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it.each([
    ['value', 'b'],
    ['label attribute', 'Second'],
  ])('matches an option by %s, like Playwright', (_label, value) => {
    const el = select('<select><option value="a">Alpha</option><option value="b" label="Second">  Beta  </option></select>');
    const log: string[] = [];
    for (const type of ['input', 'change']) el.addEventListener(type, () => log.push(type));
    expect(selectOptions(el, [value])).toEqual(['b']);
    expect(el.value).toBe('b');
    expect(log).toEqual(['input', 'change']);
  });

  it('uses the label attribute instead of the text when present, as option.label does', () => {
    const el = select('<select><option value="b" label="Second">Beta</option></select>');
    expect(() => selectOptions(el, ['Beta'])).toThrow(OPTION_NOT_FOUND);
  });

  it('matches whitespace-collapsed text when there is no label attribute', () => {
    const el = select('<select><option value="1">Two\n  words</option></select>');
    expect(selectOptions(el, ['Two words'])).toEqual(['1']);
  });

  it('takes only the first match on a single select', () => {
    const el = select('<select><option value="a">A</option><option value="b">B</option></select>');
    expect(selectOptions(el, ['b', 'a'])).toEqual(['a']);
  });

  it('takes one option per value on a multiple select and replaces the selection', () => {
    const el = select('<select multiple><option value="a" selected>A</option><option value="b">B</option><option value="c">C</option></select>');
    expect(selectOptions(el, ['c', 'B'])).toEqual(['b', 'c']);
    expect(Array.from(el.selectedOptions).map((option) => option.value)).toEqual(['b', 'c']);
  });

  it('changes nothing and fires nothing until every value has an option', () => {
    const el = select('<select multiple><option value="a" selected>A</option><option value="b">B</option></select>');
    const log: string[] = [];
    el.addEventListener('change', () => log.push('change'));
    expect(() => selectOptions(el, ['b', 'missing'])).toThrow(OPTION_NOT_FOUND);
    expect(Array.from(el.selectedOptions).map((option) => option.value)).toEqual(['a']);
    expect(log).toEqual([]);
  });

  it.each([
    ['a disabled option', '<select><option value="a" disabled>A</option></select>'],
    ['an option in a disabled optgroup', '<select><optgroup disabled><option value="a">A</option></optgroup></select>'],
    ['an aria-disabled option', '<select><option value="a" aria-disabled="true">A</option></select>'],
  ])('refuses %s so the runner waits for it', (_label, html) => {
    expect(() => selectOptions(select(html), ['a'])).toThrow(OPTION_DISABLED);
  });

  it.each([
    ['a non-breaking space', 'Two\u00a0words', 'Two words'],
    ['a zero-width space and soft hyphen', 'Two wo\u200brd\u00ads', 'Two words'],
    ['a label attribute with repeated spaces', 'Two words', 'Two   words'],
  ])('normalizes %s in labels like Playwright', (_label, text, value) => {
    const el = select('<select><option value="1"></option></select>');
    el.options[0].setAttribute('label', text);
    expect(selectOptions(el, [value])).toEqual(['1']);
  });

  it('rejects non-select targets', () => {
    document.body.innerHTML = '<input>';
    expect(() => selectOptions(document.querySelector('input')!, ['a'])).toThrow('not a select element');
  });
});
