import { TABBABLE, isDisabled } from './keyboard.js';

export type MouseButton = 'right' | 'middle';

/** `MouseEvent.button`, the `buttons` mask while held, and the legacy `which`. */
const BUTTONS: Record<MouseButton, { button: number; buttons: number; which: number }> = {
  middle: { button: 1, buttons: 4, which: 2 },
  right: { button: 2, buttons: 2, which: 3 },
};

/** The bridge's error for a disabled target; the CLI waits for it to become enabled. */
export const ELEMENT_DISABLED = 'element is disabled';

export interface ButtonClickResult {
  button: MouseButton;
  /** The `contextmenu` event's `defaultPrevented`, for a right click. */
  contextMenuPrevented?: boolean;
}

export function isMouseButton(value: string): value is MouseButton {
  return Object.prototype.hasOwnProperty.call(BUTTONS, value);
}

/**
 * Playwright's "enabled" actionability check: not a disabled form control (itself, as the
 * control containing it, or through a disabled fieldset) and not inside `aria-disabled="true"`.
 */
export function isActionDisabled(el: Element): boolean {
  const control = el.closest('button, input, select, textarea, option, optgroup');
  return (control !== null && isDisabled(control)) || el.closest('[aria-disabled="true"]') !== null;
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
  auxclick: { pointer: true, phase: 'click' },
  contextmenu: { pointer: true, phase: 'click' },
};

/**
 * Click with the right or middle button at the element's center, with the events Edge on Windows
 * fires for a real click: pointer and mouse over/enter/move, `pointerdown`, `mousedown`, focus,
 * `pointerup`, `mouseup`, `auxclick`, then `contextmenu` for the right button. As in the browser,
 * cancelling `pointerdown` suppresses `mousedown`, `mouseup` and the focus change, and cancelling
 * `mousedown` suppresses the focus change. Throws {@link ELEMENT_DISABLED} for a disabled target.
 */
export function clickButton(el: Element, button: MouseButton): ButtonClickResult {
  if (isActionDisabled(el)) {
    throw new Error(ELEMENT_DISABLED);
  }
  el.scrollIntoView?.({ block: 'center', inline: 'center' });
  const rect = el.getBoundingClientRect?.();
  const clientX = Math.round((rect?.left ?? 0) + (rect?.width ?? 0) / 2);
  const clientY = Math.round((rect?.top ?? 0) + (rect?.height ?? 0) / 2);
  const held = BUTTONS[button];
  const fire = (type: string) => {
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
      // Mouse events and auxclick count the click; pointer events and contextmenu don't.
      detail: !hover && type !== 'contextmenu' && (!pointer || type === 'auxclick') ? 1 : 0,
    };
    const event =
      pointer && typeof PointerEvent === 'function'
        ? new PointerEvent(type, { ...init, pointerId: 1, pointerType: 'mouse', isPrimary: true })
        : new MouseEvent(type, init);
    // The constructors can't set the legacy `which`, which older handlers check for 3 or 2.
    Object.defineProperty(event, 'which', { value: hover ? 0 : held.which });
    el.dispatchEvent(event);
    return event;
  };
  for (const type of ['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointermove', 'mousemove']) {
    fire(type);
  }
  const compatibility = !fire('pointerdown').defaultPrevented;
  if (compatibility && !fire('mousedown').defaultPrevented) {
    focusForMouse(el);
  }
  fire('pointerup');
  if (compatibility) {
    fire('mouseup');
  }
  fire('auxclick');
  if (button !== 'right') {
    return { button };
  }
  return { button, contextMenuPrevented: fire('contextmenu').defaultPrevented };
}

/** A mouse press focuses the nearest focusable ancestor, or else clears focus. */
function focusForMouse(el: Element): void {
  const target = el.closest(TABBABLE);
  if (target && !isDisabled(target) && !target.closest('[inert]')) {
    (target as HTMLElement).focus();
  } else if (document.activeElement && document.activeElement !== document.body) {
    (document.activeElement as HTMLElement).blur?.();
  }
}