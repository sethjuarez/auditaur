// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest';
import { executeDriveBridgeRequest } from './drive-bridge.js';
import {
  accessibleName,
  describeLocator,
  parseLocator,
  queryLocator,
  resolveLocator,
  type LocatorSpec,
} from './locator.js';
import type { DriveBridgeRequest } from './types.js';

const always = () => true;

function ids(spec: LocatorSpec): string[] {
  return queryLocator(spec).map((el) => el.id || el.tagName.toLowerCase());
}

function sel(spec: LocatorSpec): string {
  return `locator:${JSON.stringify(spec)}`;
}

beforeEach(() => {
  document.body.innerHTML = `
    <main id="main">
      <h1 id="title">Welcome  home</h1>
      <h2 id="sub">Settings</h2>
      <button id="save">Save</button>
      <button id="save-all">Save all</button>
      <button id="icon" aria-label="Close dialog">x</button>
      <input id="submit" type="submit" value="Send">
      <a id="docs" href="/docs">Read the docs</a>
      <a id="anchor">Not a link</a>
      <label for="email">Email address</label>
      <input id="email" type="email" placeholder="you@example.com">
      <label id="wrap-label">Name <input id="name" type="text"></label>
      <span id="phone-label">Phone</span>
      <input id="phone" aria-labelledby="phone-label">
      <input id="search" type="search" aria-label="Search everything">
      <input id="agree" type="checkbox"><label for="agree">I agree</label>
      <select id="pick"><option value="a">Alpha</option><option value="b">Beta</option></select>
      <textarea id="notes" placeholder="Notes"></textarea>
      <div id="card" data-testid="card"><span id="card-text">Hello <b id="world">world</b></span></div>
      <div id="card2" data-testid="card2">Hello world</div>
      <button id="hidden-btn" hidden>Save</button>
      <div aria-hidden="true"><button id="aria-hidden-btn">Save</button></div>
      <ul id="list"><li id="li1">One</li><li id="li2">Two</li><li id="li3">Three</li></ul>
      <img id="logo" alt="Company logo" src="x.png">
      <script>var text = "Welcome";</script>
    </main>`;
});

describe('locator parsing', () => {
  it('parses and describes', () => {
    const spec = parseLocator('locator:{"kind":"role","value":"button","name":"Save","exact":true,"nth":-1}');
    expect(spec).toEqual({ kind: 'role', value: 'button', name: 'Save', exact: true, nth: -1 });
    expect(describeLocator(spec)).toBe('role=button[name="Save"s] >> nth=-1');
    expect(describeLocator({ kind: 'text', value: 'Hi' })).toBe('text="Hi"i');
  });

  it('rejects bad specs', () => {
    expect(() => parseLocator('locator:{')).toThrow(/invalid locator/);
    expect(() => parseLocator('locator:{"kind":"xpath","value":"//a"}')).toThrow(/unknown kind/);
    expect(() => parseLocator('locator:{"kind":"css"}')).toThrow(/value/);
    expect(() => parseLocator('locator:{"kind":"css","value":"a","nth":1.5}')).toThrow(/nth/);
  });
});

describe('role', () => {
  it('matches implicit and explicit roles, skipping hidden ones', () => {
    expect(ids({ kind: 'role', value: 'button' })).toEqual(['save', 'save-all', 'icon', 'submit']);
    expect(ids({ kind: 'role', value: 'link' })).toEqual(['docs']);
    expect(ids({ kind: 'role', value: 'heading' })).toEqual(['title', 'sub']);
    expect(ids({ kind: 'role', value: 'checkbox' })).toEqual(['agree']);
    expect(ids({ kind: 'role', value: 'combobox' })).toEqual(['pick']);
    expect(ids({ kind: 'role', value: 'searchbox' })).toEqual(['search']);
    expect(ids({ kind: 'role', value: 'textbox' })).toEqual(['email', 'name', 'phone', 'notes']);
    expect(ids({ kind: 'role', value: 'listitem' })).toEqual(['li1', 'li2', 'li3']);
    expect(ids({ kind: 'role', value: 'img' })).toEqual(['logo']);
  });

  it('filters by accessible name, substring by default and whole string with exact', () => {
    expect(ids({ kind: 'role', value: 'button', name: 'save' })).toEqual(['save', 'save-all']);
    expect(ids({ kind: 'role', value: 'button', name: 'Save', exact: true })).toEqual(['save']);
    expect(ids({ kind: 'role', value: 'button', name: 'close dialog' })).toEqual(['icon']);
    expect(ids({ kind: 'role', value: 'button', name: 'Send' })).toEqual(['submit']);
    expect(ids({ kind: 'role', value: 'heading', name: 'welcome home', exact: false })).toEqual(['title']);
    expect(ids({ kind: 'role', value: 'heading', name: 'Welcome home', exact: true })).toEqual(['title']);
  });

  it('computes accessible names', () => {
    const name = (id: string) => accessibleName(document.getElementById(id)!);
    expect(name('email')).toBe('Email address');
    expect(name('name')).toBe('Name');
    expect(name('phone')).toBe('Phone');
    expect(name('search')).toBe('Search everything');
    expect(name('logo')).toBe('Company logo');
    expect(name('notes')).toBe('Notes');
  });

  it('skips CSS-hidden and inert elements and hidden name content', () => {
    document.body.innerHTML = `
      <button id="star"><span aria-hidden="true">★</span>Star</button>
      <button id="img-btn"><img alt="Delete" src="x.png"></button>
      <button id="gone" style="display:none">Star</button>
      <div style="display:none"><button id="nested-gone">Star</button></div>
      <button id="invisible" style="visibility:hidden">Star</button>
      <div inert><button id="inert-btn">Star</button></div>`;
    expect(ids({ kind: 'role', value: 'button', name: 'Star', exact: true })).toEqual(['star']);
    expect(ids({ kind: 'role', value: 'button', name: 'Delete', exact: true })).toEqual(['img-btn']);
  });
});

describe('text', () => {
  it('returns the innermost matching elements', () => {
    expect(ids({ kind: 'text', value: 'hello world' })).toEqual(['card-text', 'card2']);
    expect(ids({ kind: 'text', value: 'world' })).toEqual(['world', 'card2']);
    expect(ids({ kind: 'text', value: 'Welcome home', exact: true })).toEqual(['title']);
    expect(ids({ kind: 'text', value: 'welcome home', exact: true })).toEqual([]);
    expect(ids({ kind: 'text', value: 'Send' })).toEqual(['submit']);
  });

  it('ignores script content', () => {
    expect(ids({ kind: 'text', value: 'var text' })).toEqual([]);
  });
});

describe('label, placeholder and testId', () => {
  it('finds controls by label', () => {
    expect(ids({ kind: 'label', value: 'email' })).toEqual(['email']);
    expect(ids({ kind: 'label', value: 'Name', exact: true })).toEqual(['name']);
    expect(ids({ kind: 'label', value: 'phone' })).toEqual(['phone']);
    expect(ids({ kind: 'label', value: 'search everything' })).toEqual(['search']);
    expect(ids({ kind: 'label', value: 'I agree' })).toEqual(['agree']);
  });

  it('finds by placeholder and test id', () => {
    expect(ids({ kind: 'placeholder', value: 'example.com' })).toEqual(['email']);
    expect(ids({ kind: 'placeholder', value: 'Notes', exact: true })).toEqual(['notes']);
    expect(ids({ kind: 'testId', value: 'card' })).toEqual(['card']);
  });
});

describe('strictness and nth', () => {
  it('fails when a locator matches several elements', () => {
    expect(() => resolveLocator({ kind: 'role', value: 'button', name: 'save' }, false, always)).toThrow(
      'strict mode violation: `role=button[name="save"i]` resolved to 2 elements',
    );
  });

  it('picks with nth, counting from the end when negative', () => {
    expect(resolveLocator({ kind: 'role', value: 'listitem', nth: 0 }, false, always)?.id).toBe('li1');
    expect(resolveLocator({ kind: 'role', value: 'listitem', nth: -1 }, false, always)?.id).toBe('li3');
    expect(resolveLocator({ kind: 'role', value: 'listitem', nth: 5 }, false, always)).toBeUndefined();
  });

  it('applies visibility after resolving', () => {
    expect(resolveLocator({ kind: 'css', value: '#save' }, true, () => false)).toBeUndefined();
    expect(resolveLocator({ kind: 'css', value: '#missing' }, false, always)).toBeUndefined();
  });
});

describe('within', () => {
  beforeEach(() => {
    document.body.innerHTML = `
      <section id="a" aria-label="Drafts"><button id="a-del">Delete</button><p id="a-t">Note</p></section>
      <section id="b" aria-label="Archive"><button id="b-del">Delete</button>
        <section id="c" aria-label="Old"><button id="c-del">Delete</button></section>
      </section>`;
  });

  it('searches inside every container match, deduped, in document order', () => {
    const del: LocatorSpec = { kind: 'role', value: 'button', name: 'Delete' };
    expect(ids({ ...del, within: { kind: 'css', value: '#a' } })).toEqual(['a-del']);
    expect(ids({ ...del, within: { kind: 'css', value: 'section' } })).toEqual(['a-del', 'b-del', 'c-del']);
    expect(ids({ ...del, within: { kind: 'role', value: 'region', name: 'Archive' } })).toEqual(['b-del', 'c-del']);
    expect(ids({ kind: 'text', value: 'Note', within: { kind: 'css', value: 'section', nth: 0 } })).toEqual(['a-t']);
    expect(ids({ kind: 'css', value: 'section', within: { kind: 'css', value: '#c' } })).toEqual([]);
  });

  it('applies strictness and nth to the inner matches', () => {
    const spec: LocatorSpec = { kind: 'role', value: 'button', name: 'Delete', within: { kind: 'css', value: '#b' } };
    expect(() => resolveLocator(spec, false, always)).toThrow(
      'strict mode violation: `#b >> role=button[name="Delete"i]` resolved to 2 elements',
    );
    expect(resolveLocator({ ...spec, nth: -1 }, false, always)?.id).toBe('c-del');
  });

  it('validates nested locators', () => {
    expect(parseLocator(sel({ kind: 'css', value: 'b', within: { kind: 'css', value: '#a' } })).within?.value).toBe('#a');
    expect(() => parseLocator('locator:{"kind":"css","value":"b","within":{"kind":"nope","value":""}}')).toThrow(
      'unknown kind',
    );
    expect(() => parseLocator('locator:{"kind":"css","value":"b","within":{"kind":"css","value":1}}')).toThrow(
      '`value` must be a string',
    );
  });
});

describe('bridge integration', () => {
  function request(action: string, selector?: string, value?: string): DriveBridgeRequest {
    return {
      schemaVersion: 1,
      protocolVersion: 1,
      requestId: 'r1',
      action,
      selector,
      value,
      visibleOnly: false,
      createdAtUnixNanos: 0,
    } as DriveBridgeRequest;
  }

  it('acts on locator targets', async () => {
    let clicks = 0;
    document.getElementById('save')!.addEventListener('click', () => clicks++);
    await executeDriveBridgeRequest(request('click', sel({ kind: 'role', value: 'button', name: 'Save', exact: true })));
    expect(clicks).toBe(1);

    await executeDriveBridgeRequest(request('fill', sel({ kind: 'label', value: 'Email' }), 'a@b.c'));
    expect((document.getElementById('email') as HTMLInputElement).value).toBe('a@b.c');

    const exists = await executeDriveBridgeRequest(request('exists', sel({ kind: 'text', value: 'Read the docs' })));
    expect(exists.exists).toBe(true);
  });

  it('double clicks', async () => {
    const seen: string[] = [];
    const save = document.getElementById('save')!;
    save.addEventListener('click', () => seen.push('click'));
    save.addEventListener('dblclick', (event) => seen.push(`dblclick:${(event as MouseEvent).detail}`));
    await executeDriveBridgeRequest(request('dblclick', sel({ kind: 'css', value: '#save' })));
    expect(seen).toEqual(['click', 'click', 'dblclick:2']);
  });

  it('surfaces strict violations as errors', async () => {
    await expect(
      executeDriveBridgeRequest(request('exists', sel({ kind: 'role', value: 'button', name: 'save' }))),
    ).rejects.toThrow(/strict mode violation/);
  });

  it('inspects targets for assertions without throwing', async () => {
    const inspect = (spec: LocatorSpec | string) =>
      executeDriveBridgeRequest(request('inspect', typeof spec === 'string' ? spec : sel(spec)));

    const many = await inspect({ kind: 'role', value: 'button', name: 'save' });
    expect(many.count).toBe(2);
    expect(many.strict).toMatch(/strict mode violation/);
    expect(many.element).toBeNull();

    const last = await inspect({ kind: 'role', value: 'listitem', nth: -1 });
    expect(last.count).toBe(1);
    expect(last.element).toMatchObject({ text: 'Three', value: null, checked: null });

    (document.getElementById('email') as HTMLInputElement).value = 'x@y.z';
    const email = await inspect({ kind: 'label', value: 'Email' });
    expect(email.element).toMatchObject({ value: 'x@y.z', checked: null });

    (document.getElementById('agree') as HTMLInputElement).checked = true;
    expect((await inspect({ kind: 'role', value: 'checkbox' })).element).toMatchObject({ checked: true });

    expect((await inspect({ kind: 'css', value: '#pick' })).element).toMatchObject({ value: 'a' });
    expect((await inspect({ kind: 'text', value: 'no such text' })).count).toBe(0);
    expect((await inspect({ kind: 'role', value: 'listitem', nth: 9 })).count).toBe(0);
    expect((await inspect('li')).count).toBe(3);
  });
});
