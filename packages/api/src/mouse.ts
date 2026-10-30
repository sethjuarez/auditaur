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
  const rect = el.getBoundingClientRect?.();
  const clientX = Math.round((rect?.left ?? 0) + (rect?.width ?? 0) / 2);
  const clientY = Math.round((rect?.top ?? 0) + (rect?.height ?? 0) / 2);
  const held = BUTTONS[button];
  const fire = (type: string, count = 0) => {
    const { pointer, phase, boundary } = EVENTS[type];
    const hover = phase === 'hover';
    const init: MouseEventInit = {
      bubbles: !boundary,
      cancelable: !boundary,
      composed: !boundary,
      view: window,
      clientX,
      clientY,
      // Without a button, pointer events report -1 and mouse events the main button.
      button: hover ? (pointer ? -1 : 0) : held.button,
      buttons: phase === 'press' ? held.buttons : 0,
      // Mouse events and the click events count presses; pointer events and contextmenu don't.
      detail: hover || type === 'contextmenu' || (pointer && phase !== 'click') ? 0 : count,
    };
    const event =
      pointer && typeof PointerEvent === 'function'
        ? new PointerEvent(type, { ...init, pointerId: 1, pointerType: 'mouse', isPrimary: phase !== 'click' })
        : new MouseEvent(type, init);
    // The constructors can't set the legacy `which`, which older handlers check for 3 or 2.
    Object.defineProperty(event, 'which', { value: hover ? 0 : held.which });
    el.dispatchEvent(event);
    return event;
  };
  for (const type of ['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointermove', 'mousemove']) {
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