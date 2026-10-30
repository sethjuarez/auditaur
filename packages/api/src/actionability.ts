/**
 * Playwright's "enabled" and "editable" actionability checks for drive actions.
 *
 * A failing check throws a fixed message; the CLI waits for it to clear, as Playwright does,
 * instead of acting on a control a real user couldn't use.
 */

import { isDisabled } from './keyboard.js';

/** The bridge's error for a disabled target; the CLI waits for it to become enabled. */
export const ELEMENT_DISABLED = 'element is disabled';
/** The bridge's error for a readonly target; the CLI waits for it to become editable. */
export const ELEMENT_NOT_EDITABLE = 'element is not editable';

/**
 * Not a disabled form control (itself, as the control containing it, or through a disabled
 * fieldset) and not inside `aria-disabled="true"`.
 */
export function isActionDisabled(el: Element): boolean {
  const control = el.closest('button, input, select, textarea, option, optgroup');
  if (control !== null && isDisabled(control)) {
    return true;
  }
  // An option is disabled by a disabled optgroup or select around it.
  if (control?.matches('option, optgroup')) {
    const owner = control.parentElement?.closest('optgroup, select');
    if (owner && isActionDisabled(owner)) {
      return true;
    }
  }
  return el.closest('[aria-disabled="true"]') !== null;
}

/**
 * A readonly `input` or `textarea`, or an element marked `aria-readonly="true"`. Inside
 * contenteditable, an `aria-readonly` ancestor counts too, because filling or typing into a
 * descendant edits the whole editing host.
 */
export function isReadOnly(el: Element): boolean {
  if ((el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) && el.readOnly) {
    return true;
  }
  if ((el as HTMLElement).isContentEditable) {
    return el.closest('[aria-readonly="true"]') !== null;
  }
  return el.getAttribute('aria-readonly') === 'true';
}

export function requireEnabled(el: Element): void {
  if (isActionDisabled(el)) {
    throw new Error(ELEMENT_DISABLED);
  }
}

export function requireEditable(el: Element): void {
  requireEnabled(el);
  if (isReadOnly(el)) {
    throw new Error(ELEMENT_NOT_EDITABLE);
  }
}
