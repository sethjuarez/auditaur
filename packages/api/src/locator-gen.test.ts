// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest';
import { executeDriveBridgeRequest } from './drive-bridge';
import { cssPath, generateLocator } from './locator-gen';
import { MAX_RECORDED_STEPS, startRecording, stopRecording, takeRecording } from './recorder';
import type { DriveBridgeRequest } from './types';

const $ = (selector: string) => document.querySelector(selector)!;

beforeEach(() => {
  document.body.innerHTML = `
    <main>
      <button data-testid="go">Go</button>
      <button id="save">Save</button>
      <button>Save all</button>
      <ul><li><button>Edit</button></li><li><button>Edit</button></li></ul>
      <label>Name <input id="name"></label>
      <input id="search" placeholder="Search here">
      <span class="plain">Just some text</span>
      <div><div class="anon"></div><div class="anon"></div></div>
      <select id="pick"><option value="a">A</option><option value="b">B</option></select>
      <label><input id="agree" type="checkbox"> Agree</label>
      <input id="when" type="date">
      <div id="notes" contenteditable="true">Draft</div>
    </main>`;
});

describe('generateLocator', () => {
  it('prefers test ids', () => {
    expect(generateLocator($('[data-testid="go"]'))).toEqual({ testId: 'go' });
  });

  it('uses role + name, adding exact only when needed', () => {
    expect(generateLocator($('#save'))).toEqual({ role: 'button', name: 'Save', exact: true });
    expect(generateLocator(document.querySelectorAll('button')[2])).toEqual({ role: 'button', name: 'Save all' });
  });

  it('falls back to nth for duplicate names', () => {
    const second = document.querySelectorAll('li button')[1];
    expect(generateLocator(second)).toEqual({ role: 'button', name: 'Edit', exact: true, nth: 1 });
  });

  it('names fields by label and placeholder', () => {
    expect(generateLocator($('#name'))).toEqual({ role: 'textbox', name: 'Name' });
    expect(generateLocator($('#search'))).toEqual({ role: 'textbox', name: 'Search here' });
  });

  it('uses text for unnamed elements and a css path last', () => {
    expect(generateLocator($('.plain'))).toEqual({ text: 'Just some text' });
    expect(generateLocator(document.querySelectorAll('.anon')[1])).toBe(cssPath(document.querySelectorAll('.anon')[1]));
    expect(document.querySelectorAll(cssPath(document.querySelectorAll('.anon')[1]))).toHaveLength(1);
  });

  it('never uses editable content as a text locator', () => {
    expect(generateLocator($('#notes'))).toBe('#notes');
  });

  it('anchors css paths at the real root', () => {
    expect(cssPath(document.body)).toBe('body');
    expect(cssPath(document.documentElement)).toBe('html');
    const meta = document.head.appendChild(document.createElement('meta'));
    expect(document.querySelectorAll(cssPath(meta))[0]).toBe(meta);
  });

  it('names wrapped controls without their own content', () => {
    document.body.innerHTML = `
      <label>Bio <textarea>hello</textarea></label>
      <label>Size <select><option>Small</option><option>Large</option></select></label>`;
    expect(generateLocator($('textarea'))).toEqual({ role: 'textbox', name: 'Bio' });
    expect(generateLocator($('select'))).toEqual({ role: 'combobox', name: 'Size' });
  });});

describe('locate bridge action', () => {
  const request = (selector?: string): DriveBridgeRequest => ({
    schemaVersion: 1,
    protocolVersion: 1,
    requestId: 'locate',
    action: 'locate',
    selector,
    visibleOnly: false,
  });

  it('suggests a locator for each css match', async () => {
    await expect(executeDriveBridgeRequest(request('li button'))).resolves.toMatchObject({
      count: 2,
      elements: [
        { locator: { role: 'button', name: 'Edit', exact: true, nth: 0 }, tag: 'button', text: 'Edit' },
        { locator: { role: 'button', name: 'Edit', exact: true, nth: 1 }, tag: 'button', text: 'Edit' },
      ],
    });
  });

  it('accepts drill locators and reports no matches', async () => {
    await expect(executeDriveBridgeRequest(request('locator:{"kind":"role","value":"textbox","name":"Name"}'))).resolves.toMatchObject({
      count: 1,
      elements: [{ locator: { role: 'textbox', name: 'Name' }, tag: 'input' }],
    });
    await expect(executeDriveBridgeRequest(request('.missing'))).resolves.toMatchObject({ count: 0, elements: [] });
  });

  it('applies nth from a locator target', async () => {
    const target = (nth: number) => request('locator:{"kind":"role","value":"button","name":"Edit","nth":' + nth + '}');
    await expect(executeDriveBridgeRequest(target(1))).resolves.toMatchObject({
      count: 1,
      elements: [{ locator: { role: 'button', name: 'Edit', exact: true, nth: 1 } }],
    });
    await expect(executeDriveBridgeRequest(target(5))).resolves.toMatchObject({ count: 0, elements: [] });
  });

  it('needs a selector', async () => {
    await expect(executeDriveBridgeRequest(request())).rejects.toThrow('locate needs a selector');
  });
});