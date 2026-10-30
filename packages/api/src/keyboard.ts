/**
 * Synthetic keyboard input that matches what Playwright's `press` produces on a US layout:
 * the same `key`, `code`, `keyCode` and `location`, and the browser's default action for the
 * keys drills rely on (focus moves, activation, text entry), run only when the app doesn't
 * call `preventDefault()`. Events are still untrusted (`isTrusted: false`).
 */

const MODIFIER_FLAGS = {
  Alt: 'altKey',
  Control: 'ctrlKey',
  Meta: 'metaKey',
  Shift: 'shiftKey',
} as const;
export type Modifier = keyof typeof MODIFIER_FLAGS;

export interface KeyDefinition {
  key: string;
  code: string;
  keyCode: number;
  location: number;
  /** The key's value with Shift held, for keys whose value changes. */
  shiftKey?: string;
  /** Text the key enters in an editable element. */
  text?: string;
}

const NAMED: Record<string, [code: string, keyCode: number, location?: number]> = {
  Backspace: ['Backspace', 8],
  Tab: ['Tab', 9],
  Enter: ['Enter', 13],
  Shift: ['ShiftLeft', 16, 1],
  Control: ['ControlLeft', 17, 1],
  Alt: ['AltLeft', 18, 1],
  Pause: ['Pause', 19],
  CapsLock: ['CapsLock', 20],
  Escape: ['Escape', 27],
  PageUp: ['PageUp', 33],
  PageDown: ['PageDown', 34],
  End: ['End', 35],
  Home: ['Home', 36],
  ArrowLeft: ['ArrowLeft', 37],
  ArrowUp: ['ArrowUp', 38],
  ArrowRight: ['ArrowRight', 39],
  ArrowDown: ['ArrowDown', 40],
  Insert: ['Insert', 45],
  Delete: ['Delete', 46],
  Meta: ['MetaLeft', 91, 1],
  ContextMenu: ['ContextMenu', 93],
};
for (let n = 1; n <= 12; n += 1) {
  NAMED[`F${n}`] = [`F${n}`, 111 + n];
}

// [unshifted, shifted, code, keyCode]
const PRINTABLE: Array<[string, string, string, number]> = [
  [' ', ' ', 'Space', 32],
  ['`', '~', 'Backquote', 192],
  ['-', '_', 'Minus', 189],
  ['=', '+', 'Equal', 187],
  ['[', '{', 'BracketLeft', 219],
  [']', '}', 'BracketRight', 221],
  ['\\', '|', 'Backslash', 220],
  [';', ':', 'Semicolon', 186],
  ["'", '"', 'Quote', 222],
  [',', '<', 'Comma', 188],
  ['.', '>', 'Period', 190],
  ['/', '?', 'Slash', 191],
];
const DIGIT_SHIFTED = ')!@#$%^&*(';
for (let n = 0; n <= 9; n += 1) {
  PRINTABLE.push([String(n), DIGIT_SHIFTED[n], `Digit${n}`, 48 + n]);
}
for (let n = 0; n < 26; n += 1) {
  const lower = String.fromCharCode(97 + n);
  PRINTABLE.push([lower, lower.toUpperCase(), `Key${lower.toUpperCase()}`, 65 + n]);
}

const BY_KEY = new Map<string, KeyDefinition>();
const BY_CODE = new Map<string, KeyDefinition>();
for (const [key, [code, keyCode, location = 0]] of Object.entries(NAMED)) {
  const definition: KeyDefinition = { key, code, keyCode, location };
  if (key === 'Enter') {
    definition.text = '\r';
  }
  BY_KEY.set(key, definition);
  BY_CODE.set(code, definition);
}
// As in Playwright, Shift changes a key named by its code (`Shift+KeyA` is `A`); a key named by
// its character keeps it (`Shift+a` is `a`).
for (const [key, shifted, code, keyCode] of PRINTABLE) {
  BY_KEY.set(key, { key, code, keyCode, location: 0, text: key });
  BY_CODE.set(code, { key, code, keyCode, location: 0, shiftKey: shifted, text: key });
  if (shifted !== key) {
    BY_KEY.set(shifted, { key: shifted, code, keyCode, location: 0, text: shifted });
  }
}

/**
 * The definition for a key name, as Playwright accepts them: a `KeyboardEvent.key` value
 * (`Enter`, `a`, `!`), a `KeyboardEvent.code` value (`KeyA`, `Space`, `ShiftLeft`), or any
 * single character (entered as text, with no `code`).
 */
export function keyDefinition(name: string): KeyDefinition {
  const known = BY_KEY.get(name) ?? BY_CODE.get(name);
  if (known) {
    return known;
  }
  if (Array.from(name).length === 1) {
    return { key: name, code: '', keyCode: 0, location: 0, text: name };
  }
  throw new Error(`unknown key \`${name}\`; use a key such as Enter, Tab, ArrowDown, Space or a, or a code such as KeyA`);
}

/** Split a Playwright-style chord such as `Control+Shift+s` into modifiers and a key. */
export function parseKeyChord(chord: string, isMac = isMacPlatform()): { modifiers: Modifier[]; key: string } {
  // Playwright's tokenizer: `+` splits only after a token, so `+` and `Shift++` name the `+` key.
  const parts: string[] = [];
  let token = '';
  for (const char of chord) {
    if (char === '+' && token) {
      parts.push(token);
      token = '';
    } else {
      token += char;
    }
  }
  parts.push(token);
  const key = parts.pop() ?? '';
  if (!key) {
    throw new Error(`invalid key chord \`${chord}\``);
  }
  const modifiers = parts.map((part) => {
    const name = part === 'ControlOrMeta' ? (isMac ? 'Meta' : 'Control') : part;
    if (!(name in MODIFIER_FLAGS)) {
      throw new Error(`invalid key chord \`${chord}\`: unknown modifier \`${part}\``);
    }
    return name as Modifier;
  });
  return { modifiers, key };
}

function isMacPlatform(): boolean {
  return typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform ?? '');
}

export interface PressResult {
  key: string;
  code: string;
  /** True when the app called `preventDefault()` on the key's events. */
  defaultPrevented: boolean;
  /** The browser behavior emulated, or `none`. */
  defaultAction: string;
}

/** Press `chord` on `target`, like Playwright's `locator.press`. */
export function pressChord(target: Element, chord: string): PressResult {
  const { modifiers, key: name } = parseKeyChord(chord);
  const base = keyDefinition(name);
  const flags = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
  const send = (element: Element, type: string, definition: KeyDefinition, charCode = 0): boolean =>
    dispatchKey(element, type, definition, { ...flags }, charCode);

  for (const modifier of modifiers) {
    flags[MODIFIER_FLAGS[modifier]] = true;
    send(target, 'keydown', keyDefinition(modifier));
  }
  const definition: KeyDefinition =
    flags.shiftKey && base.shiftKey ? { ...base, key: base.shiftKey, text: base.shiftKey } : base;
  const shortcut = flags.ctrlKey || flags.metaKey || flags.altKey;
  const text = shortcut ? undefined : definition.text;

  const focusedBefore = document.activeElement;
  let prevented = !send(target, 'keydown', definition);
  if (!prevented && text) {
    prevented = !send(target, 'keypress', definition, text.charCodeAt(0));
  }
  let defaultAction = 'none';
  if (!prevented) {
    const outcome = keydownDefault(target, definition, flags, text);
    defaultAction = outcome.action;
    prevented = Boolean(outcome.prevented);
  }
  // If the app or Tab moved focus, keyup goes to the newly focused element, as in a browser.
  const focusMoved = document.activeElement !== focusedBefore;
  const upTarget = focusMoved && document.activeElement ? document.activeElement : target;
  const upPrevented = !send(upTarget, 'keyup', definition);
  if (!prevented && !upPrevented && !focusMoved && definition.key === ' ' && !shortcut && activatesOnSpace(target)) {
    (target as HTMLElement).click();
    defaultAction = 'click';
  }
  for (const modifier of [...modifiers].reverse()) {
    flags[MODIFIER_FLAGS[modifier]] = false;
    send(upTarget, 'keyup', keyDefinition(modifier));
  }
  return { key: definition.key, code: definition.code, defaultPrevented: prevented || upPrevented, defaultAction };
}

function dispatchKey(
  target: Element,
  type: string,
  definition: KeyDefinition,
  flags: Record<string, boolean>,
  charCode: number,
): boolean {
  const keyCode = type === 'keypress' ? charCode : definition.keyCode;
  const init = {
    key: definition.key,
    code: definition.code,
    location: definition.location,
    keyCode,
    which: keyCode,
    charCode,
    repeat: false,
    ...flags,
    bubbles: true,
    cancelable: true,
    composed: true,
  };
  const event = new KeyboardEvent(type, init);
  // Engines differ on whether the legacy fields are honored in the init dictionary.
  for (const field of ['keyCode', 'which', 'charCode'] as const) {
    if ((event as unknown as Record<string, unknown>)[field] !== init[field]) {
      Object.defineProperty(event, field, { value: init[field] });
    }
  }
  return target.dispatchEvent(event);
}

type Flags = { altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean };

/** The browser's default action for a keydown/keypress the app didn't prevent. */
function keydownDefault(target: Element, definition: KeyDefinition, flags: Flags, text: string | undefined): {
  action: string;
  prevented?: boolean;
} {
  const shortcut = flags.ctrlKey || flags.metaKey || flags.altKey;
  const editing = (outcome: EditOutcome, action: string) =>
    outcome === 'done' ? { action } : { action: 'none', prevented: outcome === 'prevented' };
  if (definition.key === 'Tab' && !shortcut) {
    return { action: moveFocus(target, flags.shiftKey ? -1 : 1) ? 'focus' : 'none' };
  }
  if (definition.key === 'Enter' && !shortcut) {
    if (activatesOnEnter(target)) {
      (target as HTMLElement).click();
      return { action: 'click' };
    }
    if (target instanceof HTMLInputElement) {
      if (isEditable(target)) {
        // Chromium announces the (no-op) line break before submitting.
        target.dispatchEvent(
          new InputEvent('beforeinput', { inputType: 'insertLineBreak', data: null, bubbles: true, cancelable: true, composed: true }),
        );
      }
      return { action: submitImplicitly(target) ? 'submit' : 'none' };
    }
    if (isEditable(target)) {
      return editing(insertLineBreak(target), 'insertLineBreak');
    }
    return { action: 'none' };
  }
  if ((flags.ctrlKey || flags.metaKey) && !flags.altKey && definition.code === 'KeyA' && isEditable(target)) {
    selectAll(target);
    return { action: 'selectAll' };
  }
  if ((definition.key === 'Backspace' || definition.key === 'Delete') && !shortcut && isEditable(target)) {
    return editing(deleteText(target, definition.key === 'Delete'), 'delete');
  }
  if (text && isEditable(target)) {
    return editing(insertText(target, text), 'insertText');
  }
  return { action: 'none' };
}

const TEXT_INPUT_TYPES = new Set(['', 'text', 'search', 'url', 'tel', 'email', 'password', 'number']);

function isEditable(el: Element): boolean {
  if (el instanceof HTMLTextAreaElement) {
    return !isDisabled(el) && !el.readOnly;
  }
  if (el instanceof HTMLInputElement) {
    return TEXT_INPUT_TYPES.has(el.type ?? '') && !isDisabled(el) && !el.readOnly;
  }
  return Boolean((el as HTMLElement).isContentEditable);
}

function activatesOnEnter(el: Element): boolean {
  const tag = el.tagName?.toLowerCase();
  if (tag === 'button' || tag === 'summary') {
    return !isDisabled(el);
  }
  if (el instanceof HTMLInputElement) {
    return ['button', 'submit', 'reset', 'image'].includes(el.type) && !isDisabled(el);
  }
  return tag === 'a' && Boolean(el.hasAttribute?.('href'));
}

/** Disabled itself or through a disabled `fieldset`. */
function isDisabled(el: Element): boolean {
  if ((el as HTMLButtonElement).disabled) {
    return true;
  }
  // Controls in a disabled fieldset's first legend stay enabled (unless an outer fieldset disables them).
  for (let fieldset = el.closest?.('fieldset[disabled]'); fieldset; fieldset = fieldset.parentElement?.closest('fieldset[disabled]') ?? null) {
    const legend = fieldset.querySelector(':scope > legend');
    if (!legend?.contains(el)) {
      return true;
    }
  }
  return false;
}

function activatesOnSpace(el: Element): boolean {
  const tag = el.tagName?.toLowerCase();
  if (tag === 'button' || tag === 'summary') {
    return !isDisabled(el);
  }
  if (el instanceof HTMLInputElement) {
    return ['checkbox', 'radio', 'button', 'submit', 'reset', 'image'].includes(el.type) && !isDisabled(el);
  }
  return false;
}

/** Enter in a text field submits its form: through its default button, or directly when the
 * form has no submit button and this is its only text field (HTML implicit submission). */
function submitImplicitly(input: HTMLInputElement): boolean {
  const form = input.form;
  if (!form || !isEditable(input)) {
    return false;
  }
  const submit = Array.from(form.elements).find(
    (el) =>
      (el instanceof HTMLButtonElement && (el.type === 'submit' || !el.getAttribute('type'))) ||
      (el instanceof HTMLInputElement && (el.type === 'submit' || el.type === 'image')),
  ) as HTMLButtonElement | HTMLInputElement | undefined;
  if (submit) {
    if (submit.disabled) {
      return false;
    }
    submit.click();
    return true;
  }
  const fields = Array.from(form.elements).filter((el) => el instanceof HTMLInputElement && isEditable(el));
  if (fields.length !== 1) {
    return false;
  }
  form.requestSubmit();
  return true;
}

const TABBABLE =
  'a[href], area[href], button, input, select, textarea, iframe, summary, [tabindex], [contenteditable]:not([contenteditable="false"])';

/** Sequential focus navigation: positive `tabIndex` first in order, then document order. */
function moveFocus(from: Element, direction: 1 | -1): boolean {
  const candidates = radioTabStops(Array.from(document.querySelectorAll(TABBABLE)).filter(isTabbable), from);
  const ordered = [
    ...candidates.filter((el) => (el as HTMLElement).tabIndex > 0).sort((a, b) => (a as HTMLElement).tabIndex - (b as HTMLElement).tabIndex),
    ...candidates.filter((el) => (el as HTMLElement).tabIndex === 0),
  ];
  if (!ordered.length) {
    return false;
  }
  const current = ordered.indexOf(from);
  const next =
    current === -1
      ? ordered[direction === 1 ? 0 : ordered.length - 1]
      : ordered[(current + direction + ordered.length) % ordered.length];
  (next as HTMLElement).focus();
  return document.activeElement === next;
}

/** A radio group is one tab stop: its checked radio, the focused one, or else its first. */
function radioTabStops(elements: Element[], from: Element): Element[] {
  const stops = new Map<string, Element>();
  const groupOf = (el: Element) =>
    el instanceof HTMLInputElement && el.type === 'radio' && el.name ? `${el.name}\u0000${el.form?.id ?? ''}` : null;
  for (const el of elements) {
    const group = groupOf(el);
    if (group && (!stops.has(group) || el === from || ((el as HTMLInputElement).checked && stops.get(group) !== from))) {
      stops.set(group, el);
    }
  }
  return elements.filter((el) => {
    const group = groupOf(el);
    return !group || stops.get(group) === el;
  });
}

function isTabbable(el: Element): boolean {
  const html = el as HTMLElement;
  if (html.tabIndex < 0 || isDisabled(el) || el.closest('[inert]')) {
    return false;
  }
  if (el instanceof HTMLInputElement && el.type === 'hidden') {
    return false;
  }
  if (!el.getClientRects().length) {
    return false;
  }
  for (let current: Element | null = el; current; current = current.parentElement) {
    const style = getComputedStyle(current);
    if (style.display === 'none' || style.visibility === 'hidden') {
      return false;
    }
  }
  return true;
}

type EditOutcome = 'done' | 'prevented' | 'failed';
type TextField = HTMLInputElement | HTMLTextAreaElement;

/** Replace `[start, end)` of a field's value. Email and number fields have no selection API,
 * so they edit at the end. */
function replaceRange(el: TextField, text: string, start: number, end: number): void {
  try {
    el.setRangeText(text, start, end, 'end');
  } catch {
    const value = el.value;
    el.value = value.slice(0, Math.min(start, value.length)) + text + value.slice(Math.min(end, value.length));
  }
}

function selection(el: TextField): [number, number] {
  try {
    const start = el.selectionStart ?? el.value.length;
    return [start, el.selectionEnd ?? start];
  } catch {
    return [el.value.length, el.value.length];
  }
}

/** Edit like the browser: a cancelable `beforeinput` first (editors such as ProseMirror and Lexical
 * handle it themselves), then the browser's own editing via `execCommand`, which fires `input`. */
function edit(
  el: Element,
  inputType: string,
  data: string | null,
  command: string,
  value: string | undefined,
  fallback: () => boolean,
): EditOutcome {
  const before = new InputEvent('beforeinput', { inputType, data, bubbles: true, cancelable: true, composed: true });
  if (!el.dispatchEvent(before)) {
    return 'prevented';
  }
  return execEditing(command, value) || fallback() ? 'done' : 'failed';
}

/** Insert text the way the browser's editing does. */
export function insertText(el: Element, text: string): EditOutcome {
  return edit(el, 'insertText', text, 'insertText', text, () => {
    if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) {
      return false;
    }
    const [start, end] = selection(el);
    replaceRange(el, text, start, end);
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
    return true;
  });
}

function insertLineBreak(el: Element): EditOutcome {
  if (el instanceof HTMLTextAreaElement) {
    return edit(el, 'insertLineBreak', null, 'insertLineBreak', undefined, () => {
      const [start, end] = selection(el);
      replaceRange(el, '\n', start, end);
      el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertLineBreak' }));
      return true;
    });
  }
  return edit(el, 'insertParagraph', null, 'insertParagraph', undefined, () => false);
}

function deleteText(el: Element, forward: boolean): EditOutcome {
  const inputType = forward ? 'deleteContentForward' : 'deleteContentBackward';
  return edit(el, inputType, null, forward ? 'forwardDelete' : 'delete', undefined, () => {
    if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) {
      return false;
    }
    let [start, end] = selection(el);
    if (start === end) {
      start = forward ? start : Math.max(0, start - 1);
      end = forward ? Math.min(el.value.length, end + 1) : end;
    }
    if (start === end) {
      return false;
    }
    replaceRange(el, '', start, end);
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType }));
    return true;
  });
}

/** Select an editable element's whole content. */
export function selectAll(el: Element): void {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    el.select();
    return;
  }
  document.getSelection()?.selectAllChildren(el);
}

/** `document.execCommand` on the focused element; deprecated, but still the only way a page
 * can make the browser itself edit, and supported by every webview. */
function execEditing(command: string, value?: string): boolean {
  try {
    return typeof document.execCommand === 'function' && document.execCommand(command, false, value);
  } catch {
    return false;
  }
}
