import { roleOf } from './locator.js';
import { mouseClick } from './mouse.js';

/** Roles whose `aria-checked` Playwright reads for `check` and `uncheck`. */
const ARIA_CHECKED_ROLES = new Set([
  'checkbox', 'menuitemcheckbox', 'menuitemradio', 'option', 'radio', 'switch', 'treeitem',
]);

/**
 * Whether a checkbox, radio, or ARIA checkable (`role="checkbox"`, `switch`, …) is checked, as
 * Playwright reads it: a native input's `checked`, else `aria-checked="true"`. `mixed` is not
 * checked. Throws for anything else.
 */
export function isChecked(el: Element): boolean {
  if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) {
    return el.checked;
  }
  const role = roleOf(el);
  if (!role || !ARIA_CHECKED_ROLES.has(role)) {
    throw new Error('selector is not a checkbox, radio button or switch');
  }
  return el.getAttribute('aria-checked') === 'true';
}

/**
 * Playwright's `check`/`uncheck`: does nothing when already in the requested state, otherwise
 * clicks like a real mouse, so the app's own click handlers change the state, then verifies it after
 * yielding a task, so state set in a promise callback counts, as it does across Playwright's round trip.
 */
export async function setChecked(el: Element, checked: boolean): Promise<void> {
  if (isChecked(el) === checked) {
    return;
  }
  if (!checked && el instanceof HTMLInputElement && el.type === 'radio') {
    throw new Error('cannot uncheck a radio button');
  }
  mouseClick(el, 'left');
  await new Promise((resolve) => setTimeout(resolve, 0));
  if (isChecked(el) !== checked) {
    throw new Error(`clicking did not ${checked ? 'check' : 'uncheck'} the element`);
  }
}
