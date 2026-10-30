// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest';
import { ELEMENT_DISABLED } from './actionability.js';
import { mouseClick, mouseDrag } from './mouse.js';

const TYPES = [
  'pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointermove', 'mousemove',
  'pointerdown', 'mousedown', 'focus', 'pointerup', 'mouseup', 'auxclick', 'contextmenu', 'click', 'dblclick',
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

describe('mouseClick', () => {
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

    expect(mouseClick(button, 'right')).toEqual({ button: 'right', contextMenuPrevented: false });
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
        seen[type] = [event instanceof PointerEvent, event.bubbles, event.cancelable, event.composed, mouse.detail, mouse.which, (event as PointerEvent).isPrimary];
      });
    }

    mouseClick(button, 'right');
    // [PointerEvent, bubbles, cancelable, composed, detail, which, isPrimary]
    expect(seen).toMatchObject({
      pointerenter: [true, false, false, false, 0, 0, true],
      mouseenter: [false, false, false, false, 0, 0, undefined],
      pointerdown: [true, true, true, true, 0, 3, true],
      mousedown: [false, true, true, true, 1, 3, undefined],
      pointerup: [true, true, true, true, 0, 3, true],
      mouseup: [false, true, true, true, 1, 3, undefined],
      auxclick: [true, true, true, true, 1, 3, false],
      contextmenu: [true, true, true, true, 0, 3, false],
    });
  });

  it('left-clicks with the events Edge fires, ending with a click PointerEvent', () => {
    document.body.innerHTML = '<input id="a"><button id="b">B</button>';
    (document.querySelector('#a') as HTMLElement).focus();
    const button = document.querySelector('#b') as HTMLElement;
    const log = record(button);
    let click: PointerEvent | undefined;
    button.addEventListener('click', (event) => (click = event as PointerEvent));

    expect(mouseClick(button, 'left')).toEqual({ button: 'left' });
    // Recorded from Edge with CDP Input.dispatchMouseEvent.
    expect(log).toEqual([
      'pointerover(-1,0)', 'pointerenter(-1,0)', 'mouseover(0,0)', 'mouseenter(0,0)',
      'pointermove(-1,0)', 'mousemove(0,0)', 'pointerdown(0,1)', 'mousedown(0,1)', 'focus',
      'pointerup(0,0)', 'mouseup(0,0)', 'click(0,0)',
    ]);
    expect(click).toBeInstanceOf(PointerEvent);
    expect([click?.detail, click?.which, click?.isPrimary, click?.pointerType]).toEqual([1, 1, false, 'mouse']);
    expect(document.activeElement).toBe(button);
  });

  it('double-clicks with two counted presses and a dblclick, as Edge does', () => {
    document.body.innerHTML = '<button id="b">B</button>';
    const button = document.querySelector('#b') as HTMLElement;
    const log: string[] = [];
    for (const type of TYPES) {
      button.addEventListener(type, (event) => log.push(`${type}:${(event as MouseEvent).detail}`));
    }

    mouseClick(button, 'left', 2);
    // Recorded from Edge: hover and focus happen once; mouse events and clicks count presses.
    expect(log.slice(6)).toEqual([
      'pointerdown:0', 'mousedown:1', 'focus:0', 'pointerup:0', 'mouseup:1', 'click:1',
      'pointerdown:0', 'mousedown:2', 'pointerup:0', 'mouseup:2', 'click:2', 'dblclick:2',
    ]);
  });

  it('runs activation behavior, so a click toggles a checkbox with input and change', () => {
    document.body.innerHTML = '<label><input id="c" type="checkbox"> Remember</label>';
    const box = document.querySelector('#c') as HTMLInputElement;
    const log: string[] = [];
    for (const type of ['click', 'input', 'change']) {
      box.addEventListener(type, () => log.push(type));
    }

    mouseClick(box, 'left');
    expect(box.checked).toBe(true);
    expect(log).toEqual(['click', 'input', 'change']);
  });

  it('middle-clicks with auxclick and no contextmenu or click', () => {
    document.body.innerHTML = '<a id="l" href="#x">link</a>';
    const link = document.querySelector('#l') as HTMLElement;
    const log = record(link);

    expect(mouseClick(link, 'middle')).toEqual({ button: 'middle' });
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

    expect(mouseClick(item, 'right').contextMenuPrevented).toBe(true);
    expect(at).toEqual([60, 40]);
  });

  it('moves focus to the nearest focusable ancestor, or clears it', () => {
    document.body.innerHTML = '<input id="a"><div id="row" tabindex="-1"><span id="label">Row</span></div><p id="text">x</p>';
    const input = document.querySelector('#a') as HTMLElement;
    input.focus();
    mouseClick(document.querySelector('#label')!, 'right');
    expect(document.activeElement?.id).toBe('row');

    input.focus();
    let blurred = false;
    input.addEventListener('blur', () => (blurred = true));
    mouseClick(document.querySelector('#text')!, 'right');
    expect(blurred).toBe(true);
    expect(document.activeElement).not.toBe(input);
  });

  it('keeps focus when mousedown is cancelled', () => {
    document.body.innerHTML = '<input id="a"><button id="b">B</button>';
    const input = document.querySelector('#a') as HTMLElement;
    const button = document.querySelector('#b') as HTMLElement;
    input.focus();
    button.addEventListener('mousedown', (event) => event.preventDefault());
    mouseClick(button, 'right');
    expect(document.activeElement).toBe(input);
  });

  it('skips mouse events and keeps focus when pointerdown is cancelled, as Edge does', () => {
    document.body.innerHTML = '<input id="a"><div id="b" tabindex="0">B</div>';
    const input = document.querySelector('#a') as HTMLElement;
    const target = document.querySelector('#b') as HTMLElement;
    input.focus();
    const log = record(target);
    target.addEventListener('pointerdown', (event) => event.preventDefault());
    mouseClick(target, 'right');
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
      expect(() => mouseClick(el, 'right')).toThrow(ELEMENT_DISABLED);
      expect(log).toEqual([]);
    }
  });

  it.each([
    ['removed on pointerdown', 'pointerdown', 'remove', ['pointerdown']],
    ['disabled on mousedown', 'mousedown', 'disable', ['pointerdown', 'mousedown']],
    ['removed by the first click', 'click', 'remove', ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']],
  ])('stops the sequence when a handler leaves the target %s', (_label, on, change, expected) => {
    document.body.innerHTML = '<input id="field"><button id="b">B</button>';
    const field = document.getElementById('field') as HTMLInputElement;
    const button = document.getElementById('b') as HTMLButtonElement;
    field.focus();
    const log: string[] = [];
    for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click', 'dblclick']) {
      button.addEventListener(type, () => {
        log.push(type);
        if (type === on) {
          if (change === 'remove') button.remove();
          else button.disabled = true;
        }
      });
    }
    mouseClick(button, 'left', 2);
    expect(log).toEqual(expected);
    if (on !== 'click') expect(document.activeElement).toBe(field);
  });
});
describe('mouseDrag', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  const DRAG_TYPES = [
    'pointerdown', 'mousedown', 'pointermove', 'mousemove', 'pointerup', 'mouseup', 'click', 'pointercancel',
    'dragstart', 'drag', 'dragenter', 'dragleave', 'dragover', 'drop', 'dragend',
  ];

  function recordDrag(): string[] {
    const log: string[] = [];
    for (const type of DRAG_TYPES) {
      document.addEventListener(type, (event) => {
        const id = (event.target as Element).id || (event.target as Element).tagName?.toLowerCase();
        log.push(`${type}@${id}:${(event as MouseEvent).buttons}`);
      }, { capture: true, signal: controller.signal });
    }
    return log;
  }
  let controller = new AbortController();
  afterEach(() => {
    controller.abort();
    controller = new AbortController();
  });

  it('presses the source, moves in steps with the button held, and releases over the target', async () => {
    document.body.innerHTML = '<ul id="list"><li id="a">A</li><li id="b">B</li></ul>';
    const log = recordDrag();
    const result = await mouseDrag(document.getElementById('a')!, document.getElementById('b')!);

    expect(result).toEqual({ mode: 'pointer' });
    const moves = log.filter((entry) => entry.startsWith('pointermove'));
    // One hover move, then five steps and a repeat at the target, all with the button held.
    expect(moves).toHaveLength(7);
    expect(moves.slice(1).every((entry) => entry.endsWith(':1'))).toBe(true);
    // Like Playwright, the source is hovered before it's pressed.
    expect(log.slice(0, 4)).toEqual(['pointermove@a:0', 'mousemove@a:0', 'pointerdown@a:1', 'mousedown@a:1']);
    // The click goes to the nearest element containing both the press and the release.
    expect(log.slice(-3)).toEqual(['pointerup@b:0', 'mouseup@b:0', 'click@list:0']);
  });

  it('waits a frame between moves so the page can render', async () => {
    document.body.innerHTML = '<div id="a">A</div><div id="b">B</div>';
    const frames: number[] = [];
    let moves = 0;
    document.addEventListener('pointermove', () => moves++, { signal: controller.signal });
    const tick = setInterval(() => frames.push(moves), 1);
    await mouseDrag(document.getElementById('a')!, document.getElementById('b')!);
    clearInterval(tick);
    // Some timer ran between the first and last move.
    expect(frames.some((count) => count > 0 && count < 7)).toBe(true);
  });

  it('runs the HTML5 sequence for a draggable source and drops where dragover is cancelled', async () => {
    document.body.innerHTML = '<div id="a" draggable="true">A</div><div id="b">B</div>';
    const target = document.getElementById('b')!;
    target.addEventListener('dragover', (event) => event.preventDefault());
    const log = recordDrag();
    const result = await mouseDrag(document.getElementById('a')!, target);

    expect(result).toEqual({ mode: 'html5', dropped: true });
    const kinds = log.map((entry) => entry.split(':')[0]);
    expect(kinds.slice(0, 6)).toEqual(['pointermove@a', 'mousemove@a', 'pointerdown@a', 'mousedown@a', 'dragstart@a', 'pointercancel@a']);
    expect(kinds).toContain('dragenter@b');
    expect(kinds.slice(-2)).toEqual(['drop@b', 'dragend@a']);
    // A native drag ends the pointer stream: no moves, release or click after dragstart.
    expect(kinds.filter((kind) => /^(pointermove|pointerup|mouseup|click)@/.test(kind))).toEqual(['pointermove@a']);
  });

  it('reports no drop when the target never accepts it', async () => {
    document.body.innerHTML = '<div id="a" draggable="true">A</div><div id="b">B</div>';
    const result = await mouseDrag(document.getElementById('a')!, document.getElementById('b')!);
    expect(result).toEqual({ mode: 'html5', dropped: false });
  });

  it('falls back to pointer events when dragstart is cancelled', async () => {
    document.body.innerHTML = '<div id="a" draggable="true">A</div><div id="b">B</div>';
    const source = document.getElementById('a')!;
    source.addEventListener('dragstart', (event) => event.preventDefault());
    const log = recordDrag();
    expect(await mouseDrag(source, document.getElementById('b')!)).toEqual({ mode: 'pointer' });
    expect(log.at(-2)).toBe('mouseup@b:0');
  });

  it('refuses a disabled source before pressing it', async () => {
    document.body.innerHTML = '<button id="a" disabled>A</button><div id="b">B</div>';
    const log = recordDrag();
    await expect(mouseDrag(document.getElementById('a')!, document.getElementById('b')!)).rejects.toThrow(ELEMENT_DISABLED);
    expect(log).toEqual([]);
  });
});
