/**
 * Pick the most readable drill locator that uniquely resolves to an element,
 * in the spirit of Playwright's locator generator: test id, role + name, label, placeholder,
 * text, `#id`, role + name + nth, then a CSS path. Agents use it (via the `locate`
 * bridge action) to turn a CSS selector into a locator that survives markup changes.
 */
import { accessibleName, labelsOf, normalizeText, queryLocator, roleOf, type LocatorSpec } from './locator.js';

/** A locator in drill-file form: a CSS string or `{ role, name, exact, nth, ... }`. */
export type DrillLocator = string | Record<string, string | number | boolean>;

const MAX_TEXT_CHARS = 80;
const SIMPLE_ID = /^[A-Za-z][\w-]*$/;

export function generateLocator(el: Element): DrillLocator {
  const testId = el.getAttribute('data-testid');
  if (testId && unique({ kind: 'testId', value: testId }, el)) {
    return { testId };
  }

  const role = roleOf(el);
  const name = role ? accessibleName(el) : '';
  if (role && name) {
    const found = uniqueFlexible({ kind: 'role', value: role, name }, el);
    if (found) {
      return withExact({ role, name }, found);
    }
  }

  for (const raw of labelsOf(el)) {
    const label = normalizeText(raw);
    const found = label && uniqueFlexible({ kind: 'label', value: label }, el);
    if (found) {
      return withExact({ label }, found);
    }
  }

  const placeholder = normalizeText(el.getAttribute('placeholder') ?? '');
  const placeholderMatch = placeholder && uniqueFlexible({ kind: 'placeholder', value: placeholder }, el);
  if (placeholderMatch) {
    return withExact({ placeholder }, placeholderMatch);
  }

  if ((!role || !name) && !isFormControl(el)) {
    const text = normalizeText(el.textContent ?? '');
    const textMatch = text && text.length <= MAX_TEXT_CHARS && uniqueFlexible({ kind: 'text', value: text }, el);
    if (textMatch) {
      return withExact({ text }, textMatch);
    }
  }

  if (el.id && SIMPLE_ID.test(el.id) && document.querySelectorAll(`#${el.id}`).length === 1) {
    return `#${el.id}`;
  }

  if (role && name) {
    const nth = queryLocator({ kind: 'role', value: role, name, exact: true }).indexOf(el);
    if (nth >= 0) {
      return { role, name, exact: true, nth };
    }
  }

  return cssPath(el);
}

type Match = 'loose' | 'exact';

// Content-derived text locators would stop matching once the field is edited.
function isFormControl(el: Element): boolean {
  return (
    el instanceof HTMLInputElement ||
    el instanceof HTMLSelectElement ||
    el instanceof HTMLTextAreaElement ||
    (el as HTMLElement).isContentEditable === true
  );
}

function uniqueFlexible(spec: LocatorSpec, el: Element): Match | undefined {
  if (unique(spec, el)) {
    return 'loose';
  }
  return unique({ ...spec, exact: true }, el) ? 'exact' : undefined;
}

function unique(spec: LocatorSpec, el: Element): boolean {
  const matches = queryLocator(spec);
  return matches.length === 1 && matches[0] === el;
}

function withExact(locator: Record<string, string>, match: Match): DrillLocator {
  return match === 'exact' ? { ...locator, exact: true } : locator;
}

/** `body > main > div:nth-of-type(2) > button` style path anchored at the nearest unique id. */
export function cssPath(el: Element): string {
  const parts: string[] = [];
  let node: Element | null = el;
  while (node && node !== document.documentElement) {
    if (node === document.body) {
      parts.unshift('body');
      return parts.join(' > ');
    }
    if (node.id && SIMPLE_ID.test(node.id) && document.querySelectorAll(`#${node.id}`).length === 1) {
      parts.unshift(`#${node.id}`);
      return parts.join(' > ');
    }
    const tag = node.tagName.toLowerCase();
    const parent: Element | null = node.parentElement;
    const siblings = parent ? Array.from(parent.children).filter((child) => child.tagName === node!.tagName) : [];
    parts.unshift(siblings.length > 1 ? `${tag}:nth-of-type(${siblings.indexOf(node) + 1})` : tag);
    node = parent;
  }
  parts.unshift('html');
  return parts.join(' > ');
}
