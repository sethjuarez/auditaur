// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest';
import { ELEMENT_DISABLED, clickButton } from './mouse.js';

const TYPES = [
  'pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointermove', 'mousemove',
  'pointerdown', 'mousedown', 'focus', 'pointerup', 'mouseup', 'auxclick', 'contextmenu', 'click',
];

function record(el: Element): string[] {
  const log: string[] = [];
  for (const type of TYPES) {
    el.addEventListener(type, (event) => {
      const mouse = event as MouseEvent;
      log.push(type === 'focus' ? type : `${type}(${mouse.button},${mouse.buttons})`);
    }, true);
  }
  return log;
}

describe('clickButton', () => {
  const cleanups: Array<() => void> = [];
  afterEach(() => {
    cleanups.splice(0).forEach((cleanup) => cleanup());
    document.body.innerHTML = '';
  });

  it('fires the events a real right click fires in Edge, ending with contextmenu', () => {
    document.body.innerHTML = '<input id="a"><button id="b">B</button>';
    (document.querySelector('#a') as HTMLElement).focus();
    const button = document.querySelector('#b') as HTMLElement;
    const log = record(button);

    expect(clickButton(button, 'right')).toEqual({ button: 'right', contextMenuPrevented: false });
    // Recorded from Edge with CDP Input.dispatchMouseEvent.
    expect(log).toEqual([
      'pointerover(-1,0)', 'pointerenter(-1,0)', 'mouseover(0,0)', 'mouseenter(0,0)',
      'pointermove(-1,0)', 'mousemove(0,0)', 'pointerdown(2,2)', 'mousedown(2,2)', 'focus',
      'pointerup(2,0)', 'mouseup(2,0)', 'auxclick(2,0)', 'contextmenu(2,0)',
    ]);
    expect(document.activeElement).toBe(button);
  });

  it('gives each event the interface and flags Edge does', () => {
    document.body.innerHTML = '<button id="b">B</button>';
    const button = document.querySelector('#b') as HTMLElement;
    const seen: Record<string, unknown> = {};
    for (const type of TYPES) {
      button.addEventListener(type, (event) => {
        const mouse = event as MouseEvent;
        seen[type] = [event instanceof PointerEvent, event.bubbles, event.cancelable, event.composed, mouse.detail, mouse.which];
      });
    }

    clickButton(button, 'right');
    // [PointerEvent, bubbles, cancelable, composed, detail, which]
    expect(seen).toMatchObject({
      pointerenter: [true, false, false, false, 0, 0],
      mouseenter: [false, false, false, false, 0, 0],
      pointerdown: [true, true, true, true, 0, 3],
      mousedown: [false, true, true, true, 1, 3],
      pointerup: [true, true, true, true, 0, 3],
      mouseup: [false, true, true, true, 1, 3],
      auxclick: [true, true, true, true, 1, 3],
      contextmenu: [true, true, true, true, 0, 3],
    });
  });

  it('middle-clicks with auxclick and no contextmenu or click', () => {
    document.body.innerHTML = '<a id="l" href="#x">link</a>';
    const link = document.querySelector('#l') as HTMLElement;
    const log = record(link);

    expect(clickButton(link, 'middle')).toEqual({ button: 'middle' });
    expect(log.slice(6)).toEqual([
      'pointerdown(1,4)', 'mousedown(1,4)', 'focus', 'pointerup(1,0)', 'mouseup(1,0)', 'auxclick(1,0)',
    ]);
  });

  it('reports a handled context menu and puts the target in clientX/clientY', () => {
    document.body.innerHTML = '<ul><li id="item">Note</li></ul>';
    const item = document.querySelector('#item') as HTMLElement;
    item.getBoundingClientRect = () => ({ left: 10, top: 20, width: 100, height: 40 }) as DOMRect;
    let at: [number, number] = [0, 0];
    const onContextMenu = (event: MouseEvent) => {
      at = [event.clientX, event.clientY];
      event.preventDefault();
    };
    document.addEventListener('contextmenu', onContextMenu);
    cleanups.push(() => document.removeEventListener('contextmenu', onContextMenu));

    expect(clickButton(item, 'right').contextMenuPrevented).toBe(true);
    expect(at).toEqual([60, 40]);
  });

  it('moves focus to the nearest focusable ancestor, or clears it', () => {
    document.body.innerHTML = '<input id="a"><div id="row" tabindex="-1"><span id="label">Row</span></div><p id="text">x</p>';
    const input = document.querySelector('#a') as HTMLElement;
    input.focus();
    clickButton(document.querySelector('#label')!, 'right');
    expect(document.activeElement?.id).toBe('row');

    input.focus();
    let blurred = false;
    input.addEventListener('blur', () => (blurred = true));
    clickButton(document.querySelector('#text')!, 'right');
    expect(blurred).toBe(true);
    expect(document.activeElement).not.toBe(input);
  });

  it('keeps focus when mousedown is cancelled', () => {
    document.body.innerHTML = '<input id="a"><button id="b">B</button>';
    const input = document.querySelector('#a') as HTMLElement;
    const button = document.querySelector('#b') as HTMLElement;
    input.focus();
    button.addEventListener('mousedown', (event) => event.preventDefault());
    clickButton(button, 'right');
    expect(document.activeElement).toBe(input);
  });

  it('skips mouse events and keeps focus when pointerdown is cancelled, as Edge does', () => {
    document.body.innerHTML = '<input id="a"><div id="b" tabindex="0">B</div>';
    const input = document.querySelector('#a') as HTMLElement;
    const target = document.querySelector('#b') as HTMLElement;
    input.focus();
    const log = record(target);
    target.addEventListener('pointerdown', (event) => event.preventDefault());
    clickButton(target, 'right');
    expect(log.slice(6)).toEqual(['pointerdown(2,2)', 'pointerup(2,0)', 'auxclick(2,0)', 'contextmenu(2,0)']);
    expect(document.activeElement).toBe(input);
  });

  it('refuses disabled targets without firing anything, like Playwright', () => {
    document.body.innerHTML = [
      '<button id="native" disabled><span id="inner">x</span></button>',
      '<fieldset disabled><input id="fielded"></fieldset>',
      '<div aria-disabled="true"><span id="aria">x</span></div>',
    ].join('');
    for (const id of ['native', 'inner', 'fielded', 'aria']) {
      const el = document.getElementById(id)!;
      const log = record(el);
      expect(() => clickButton(el, 'right')).toThrow(ELEMENT_DISABLED);
      expect(log).toEqual([]);
    }
  });
});