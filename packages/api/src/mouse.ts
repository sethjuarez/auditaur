import { isActionDisabled, requireEnabled } from './actionability.js';
import { TABBABLE, isDisabled } from './keyboard.js';

export type MouseButton = 'left' | 'right' | 'middle';

/** `MouseEvent.button`, the `buttons` mask while held, and the legacy `which`. */
const BUTTONS: Record<MouseButton, { button: number; buttons: number; which: number }> = {
  left: { button: 0, buttons: 1, which: 1 },
  middle: { button: 1, buttons: 4, which: 2 },
  right: { button: 2, buttons: 2, which: 3 },
};

export interface MouseClickResult {
  button: MouseButton;
  /** The `contextmenu` event's `defaultPrevented`, for a right click. */
  contextMenuPrevented?: boolean;
}

export function isMouseButton(value: string): value is MouseButton {
  return Object.prototype.hasOwnProperty.call(BUTTONS, value);
}

type Phase = 'hover' | 'press' | 'release' | 'click';

/**
 * Each event, with the interface and flags Edge on Windows gives a real click (recorded with CDP
 * `Input.dispatchMouseEvent`). `enter` events don't bubble, cancel or cross shadow roots.
 */
const EVENTS: Record<string, { pointer: boolean; phase: Phase; boundary?: boolean }> = {
  pointerover: { pointer: true, phase: 'hover' },
  pointerenter: { pointer: true, phase: 'hover', boundary: true },
  mouseover: { pointer: false, phase: 'hover' },
  mouseenter: { pointer: false, phase: 'hover', boundary: true },
  pointermove: { pointer: true, phase: 'hover' },
  mousemove: { pointer: false, phase: 'hover' },
  pointerout: { pointer: true, phase: 'hover' },
  mouseout: { pointer: false, phase: 'hover' },
  pointerdown: { pointer: true, phase: 'press' },
  mousedown: { pointer: false, phase: 'press' },
  pointerup: { pointer: true, phase: 'release' },
  mouseup: { pointer: false, phase: 'release' },
  click: { pointer: true, phase: 'click' },
  auxclick: { pointer: true, phase: 'click' },
  contextmenu: { pointer: true, phase: 'click' },
  dblclick: { pointer: false, phase: 'click' },
};

/**
 * Click at the element's center with the events Edge on Windows fires for a real click: pointer
 * and mouse over/enter/move, then per press `pointerdown`, `mousedown`, focus, `pointerup`,
 * `mouseup` and `click` (left) or `auxclick` (right, middle), then `contextmenu` for the right
 * button and `dblclick` after a second left press. As in the browser, cancelling `pointerdown`
 * suppresses that press's `mousedown`, `mouseup` and focus change, and cancelling `mousedown`
 * suppresses the focus change. The dispatched `click` runs the element's activation behavior
 * (toggling a checkbox, following a link, submitting a form), as a real click does. If a handler
 * removes or disables the target, the sequence stops there, as the browser stops targeting it.
 * Throws `element is disabled` for a disabled target.
 */
export function mouseClick(el: Element, button: MouseButton, clickCount: 1 | 2 = 1): MouseClickResult {
  requireEnabled(el);
  el.scrollIntoView?.({ block: 'center', inline: 'center' });
  const at = centerOf(el);
  const held = BUTTONS[button];
  const fire = (type: string, count = 0) => fireMouse(el, type, at, held, count);
  for (const type of HOVER) {
    fire(type);
  }
  const gone = () => el.isConnected === false || isActionDisabled(el);
  for (let count = 1; count <= clickCount; count++) {
    const compatibility = !fire('pointerdown').defaultPrevented;
    if (gone()) return { button };
    if (compatibility) {
      const focus = !fire('mousedown', count).defaultPrevented;
      if (gone()) return { button };
      if (focus) focusForMouse(el);
    }
    fire('pointerup');
    if (compatibility) fire('mouseup', count);
    if (gone()) return { button };
    fire(button === 'left' ? 'click' : 'auxclick', count);
    if (gone()) return { button };
  }
  if (button === 'left' && clickCount === 2) {
    fire('dblclick', 2);
  }
  if (button !== 'right') {
    return { button };
  }
  return { button, contextMenuPrevented: fire('contextmenu').defaultPrevented };
}

const HOVER = ['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointermove', 'mousemove'];

interface Point {
  clientX: number;
  clientY: number;
}

function centerOf(el: Element): Point {
  const rect = el.getBoundingClientRect?.();
  return {
    clientX: Math.round((rect?.left ?? 0) + (rect?.width ?? 0) / 2),
    clientY: Math.round((rect?.top ?? 0) + (rect?.height ?? 0) / 2),
  };
}

/** Dispatch one mouse or pointer event; `holding` is the `buttons` mask for moves while pressed. */
function fireMouse(
  el: Element,
  type: string,
  at: Point,
  held: { button: number; buttons: number; which: number },
  count = 0,
  holding = 0,
): Event {
  const { pointer, phase, boundary } = EVENTS[type];
  const hover = phase === 'hover';
  const init: MouseEventInit = {
    bubbles: !boundary,
    cancelable: !boundary,
    composed: !boundary,
    view: window,
    ...at,
    // Without a button change, pointer events report -1 and mouse events the main button.
    button: hover ? (pointer ? -1 : 0) : held.button,
    buttons: phase === 'press' ? held.buttons : hover ? holding : 0,
    // Mouse events and the click events count presses; pointer events and contextmenu don't.
    detail: hover || type === 'contextmenu' || (pointer && phase !== 'click') ? 0 : count,
  };
  const event =
    pointer && typeof PointerEvent === 'function'
      ? new PointerEvent(type, { ...init, pointerId: 1, pointerType: 'mouse', isPrimary: phase !== 'click' })
      : new MouseEvent(type, init);
  // The constructors can't set the legacy `which`, which older handlers check for 3 or 2.
  Object.defineProperty(event, 'which', { value: hover && !holding ? 0 : held.which });
  el.dispatchEvent(event);
  return event;
}

export interface MouseDragResult {
  /** `html5` when the source is `draggable` and its `dragstart` wasn't cancelled. */
  mode: 'pointer' | 'html5';
  /** For an HTML5 drag, whether the target accepted the drop (cancelled `dragover`). */
  dropped?: boolean;
}

/** Moves between press and release; drag libraries need several to pass their activation distance. */
const DRAG_MOVES = 5;

/**
 * Drag `source` onto `target` the way a real mouse does, like Playwright's `dragTo`: hover and
 * press the source's center, move in steps to the target's center (once more at the end, so
 * libraries that update the drop target on a move see it), and release, then `click` their common
 * ancestor. Each move waits a frame, so libraries such as dnd-kit can render between them. A
 * `draggable="true"` source whose `dragstart` isn't cancelled gets the HTML5 sequence instead,
 * as in Chromium: `pointercancel`, then `drag`, `dragenter`, `dragleave` and `dragover` per move,
 * `drop` if the last `dragover` was cancelled, and `dragend`.
 */
export async function mouseDrag(source: Element, target: Element): Promise<MouseDragResult> {
  requireEnabled(source);
  source.scrollIntoView?.({ block: 'center', inline: 'center' });
  const from = centerOf(source);
  const held = BUTTONS.left;
  for (const type of HOVER) {
    fireMouse(source, type, from, held);
  }
  // A cancelled `pointerdown` suppresses the compatibility mouse events until release.
  const compatibility = !fireMouse(source, 'pointerdown', from, held).defaultPrevented;
  if (compatibility && !fireMouse(source, 'mousedown', from, held, 1).defaultPrevented) {
    focusForMouse(source);
  }
  await nextFrame();
  target.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  const to = centerOf(target);

  const draggable = source.closest('[draggable="true"]');
  let html5: { transfer: DataTransfer | null; over: Element | null; accepted: boolean } | undefined;
  let current: Element = source;
  let at = from;
  for (let move = 1; move <= DRAG_MOVES + 1; move++) {
    const t = Math.min(move / DRAG_MOVES, 1);
    at = {
      clientX: Math.round(from.clientX + (to.clientX - from.clientX) * t),
      clientY: Math.round(from.clientY + (to.clientY - from.clientY) * t),
    };
    const hit = hitTest(at, t === 1 ? target : current);
    if (move === 1 && draggable) {
      const transfer = typeof DataTransfer === 'function' ? new DataTransfer() : null;
      if (!fireDrag(draggable, 'dragstart', from, transfer).defaultPrevented) {
        html5 = { transfer, over: null, accepted: false };
        source.dispatchEvent(
          typeof PointerEvent === 'function'
            ? new PointerEvent('pointercancel', { bubbles: true, composed: true, pointerId: 1, pointerType: 'mouse', isPrimary: true, ...from })
            : new MouseEvent('pointercancel', { bubbles: true, composed: true, ...from }),
        );
      }
    }
    if (html5) {
      fireDrag(draggable as Element, 'drag', at, html5.transfer);
      if (hit !== html5.over) {
        fireDrag(hit, 'dragenter', at, html5.transfer);
        if (html5.over) fireDrag(html5.over, 'dragleave', at, html5.transfer);
        html5.over = hit;
      }
      html5.accepted = fireDrag(hit, 'dragover', at, html5.transfer).defaultPrevented;
    } else {
      // Boundary events as the pointer crosses elements (enter/leave aren't sent).
      if (hit !== current) {
        fireMouse(current, 'pointerout', at, held, 0, held.buttons);
        if (compatibility) fireMouse(current, 'mouseout', at, held, 0, held.buttons);
        fireMouse(hit, 'pointerover', at, held, 0, held.buttons);
        if (compatibility) fireMouse(hit, 'mouseover', at, held, 0, held.buttons);
      }
      current = hit;
      fireMouse(hit, 'pointermove', at, held, 0, held.buttons);
      if (compatibility) fireMouse(hit, 'mousemove', at, held, 0, held.buttons);
    }
    await nextFrame();
  }

  if (html5) {
    const dropped = Boolean(html5.over && html5.accepted);
    if (dropped) fireDrag(html5.over as Element, 'drop', at, html5.transfer);
    fireDrag(draggable as Element, 'dragend', at, html5.transfer);
    return { mode: 'html5', dropped };
  }
  fireMouse(current, 'pointerup', at, held);
  if (compatibility) fireMouse(current, 'mouseup', at, held, 1);
  // The click goes to the nearest node that contains both the press and the release.
  const clicked = commonAncestor(source, current);
  if (clicked) fireMouse(clicked, 'click', at, held, 1);
  return { mode: 'pointer' };
}

function fireDrag(el: Element, type: string, at: Point, transfer: DataTransfer | null): Event {
  const cancelable = type !== 'dragleave' && type !== 'dragend';
  const init: MouseEventInit = { bubbles: true, cancelable, composed: true, view: window, buttons: type === 'dragend' ? 0 : 1, ...at };
  if (typeof DragEvent === 'function') {
    return dispatched(el, new DragEvent(type, { ...init, dataTransfer: transfer }));
  }
  const event = new MouseEvent(type, init);
  Object.defineProperty(event, 'dataTransfer', { value: transfer });
  return dispatched(el, event);
}

function dispatched(el: Element, event: Event): Event {
  el.dispatchEvent(event);
  return event;
}

/** The element under the point, or `fallback` where hit testing isn't available. */
function hitTest(at: Point, fallback: Element): Element {
  return document.elementFromPoint?.(at.clientX, at.clientY) ?? fallback;
}

function commonAncestor(a: Element, b: Element): Element | null {
  for (let node: Element | null = a; node; node = node.parentElement) {
    if (node.contains(b)) return node;
  }
  return null;
}

/** Let the page render between moves; timers, unlike animation frames, still run in a hidden window. */
function nextFrame(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 16));
}

/** A mouse press focuses the nearest focusable ancestor, or else clears focus. */
function focusForMouse(el: Element): void {
  const target = el.closest(TABBABLE);
  if (target && !isDisabled(target) && !target.closest('[inert]')) {
    if (document.activeElement !== target) {
      (target as HTMLElement).focus();
    }
  } else if (document.activeElement && document.activeElement !== document.body) {
    (document.activeElement as HTMLElement).blur?.();
  }
}