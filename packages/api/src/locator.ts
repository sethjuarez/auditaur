/**
 * Playwright-style locators for the drive bridge.
 *
 * A selector of the form `locator:{json}` is resolved here; anything else is plain CSS.
 * Locators are strict: an action on a locator that matches more than one element fails
 * unless `nth` picks one.
 */

export const LOCATOR_PREFIX = 'locator:';

export type LocatorKind = 'css' | 'role' | 'text' | 'testId' | 'label' | 'placeholder';

export interface LocatorSpec {
  kind: LocatorKind;
  value: string;
  /** Accessible name filter for `role`. */
  name?: string;
  /** Case-sensitive whole-string match instead of case-insensitive substring. */
  exact?: boolean;
  /** Pick one match; negative counts from the end. */
  nth?: number;
}

const KINDS: readonly LocatorKind[] = ['css', 'role', 'text', 'testId', 'label', 'placeholder'];
const SKIPPED_TAGS = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'HEAD', 'TITLE', 'META', 'LINK']);
const NAME_FROM_CONTENT = new Set([
  'button', 'cell', 'checkbox', 'columnheader', 'gridcell', 'heading', 'link', 'menuitem',
  'menuitemcheckbox', 'menuitemradio', 'option', 'radio', 'row', 'rowheader', 'switch', 'tab',
  'tooltip', 'treeitem', 'listitem',
]);

export function isLocatorSelector(selector: string | undefined): selector is string {
  return !!selector && selector.startsWith(LOCATOR_PREFIX);
}

export function parseLocator(selector: string): LocatorSpec {
  let spec: unknown;
  try {
    spec = JSON.parse(selector.slice(LOCATOR_PREFIX.length));
  } catch (error) {
    throw new Error(`invalid locator: ${error instanceof Error ? error.message : String(error)}`);
  }
  const candidate = spec as Partial<LocatorSpec> | null;
  if (!candidate || typeof candidate !== 'object' || !KINDS.includes(candidate.kind as LocatorKind)) {
    throw new Error(`invalid locator: unknown kind ${JSON.stringify(candidate?.kind)}`);
  }
  if (typeof candidate.value !== 'string') {
    throw new Error('invalid locator: `value` must be a string');
  }
  if (candidate.nth !== undefined && !Number.isInteger(candidate.nth)) {
    throw new Error('invalid locator: `nth` must be an integer');
  }
  return candidate as LocatorSpec;
}

export function describeLocator(spec: LocatorSpec): string {
  const flag = spec.exact ? 's' : 'i';
  let text: string;
  switch (spec.kind) {
    case 'css':
      text = spec.value;
      break;
    case 'role':
      text = `role=${spec.value}${spec.name !== undefined ? `[name=${JSON.stringify(spec.name)}${flag}]` : ''}`;
      break;
    case 'testId':
      text = `testId=${spec.value}`;
      break;
    default:
      text = `${spec.kind}=${JSON.stringify(spec.value)}${flag}`;
  }
  return spec.nth !== undefined ? `${text} >> nth=${spec.nth}` : text;
}

/** Every element the locator matches, in document order, before `nth`. */
export function queryLocator(spec: LocatorSpec, root: ParentNode = document): Element[] {
  switch (spec.kind) {
    case 'css':
      return Array.from(root.querySelectorAll(spec.value));
    case 'testId':
      return Array.from(root.querySelectorAll('[data-testid]')).filter(
        (el) => el.getAttribute('data-testid') === spec.value,
      );
    case 'placeholder':
      return Array.from(root.querySelectorAll('[placeholder]')).filter((el) =>
        textMatches(el.getAttribute('placeholder') ?? '', spec.value, spec.exact),
      );
    case 'label':
      return allElements(root).filter((el) =>
        labelsOf(el).some((label) => textMatches(label, spec.value, spec.exact)),
      );
    case 'text':
      return textMatchesIn(root, spec);
    case 'role':
      return allElements(root).filter(
        (el) =>
          roleOf(el) === spec.value
          && !hiddenFromAccessibility(el)
          && (spec.name === undefined || textMatches(accessibleName(el), spec.name, spec.exact)),
      );
  }
}

/**
 * Resolve the single element a locator points at. Throws on a strict-mode violation;
 * returns `undefined` when nothing (or nothing visible, with `visibleOnly`) matches.
 */
export function resolveLocator(
  spec: LocatorSpec,
  visibleOnly: boolean,
  isVisible: (el: Element) => boolean,
): Element | undefined {
  const matches = queryLocator(spec);
  let el: Element | undefined;
  if (spec.nth !== undefined) {
    el = spec.nth < 0 ? matches[matches.length + spec.nth] : matches[spec.nth];
  } else if (matches.length > 1) {
    throw new Error(
      `strict mode violation: \`${describeLocator(spec)}\` resolved to ${matches.length} elements; add \`nth\` or narrow the locator`,
    );
  } else {
    el = matches[0];
  }
  return el && (!visibleOnly || isVisible(el)) ? el : undefined;
}

export function normalizeText(value: string): string {
  return value.replace(/\s+/g, ' ').trim();
}

function textMatches(actual: string, expected: string, exact: boolean | undefined): boolean {
  const a = normalizeText(actual);
  const e = normalizeText(expected);
  return exact ? a === e : a.toLowerCase().includes(e.toLowerCase());
}

function allElements(root: ParentNode): Element[] {
  return Array.from(root.querySelectorAll('*')).filter((el) => !SKIPPED_TAGS.has(el.tagName));
}

/** Text a user could read: like `textContent`, minus script/style and similar. */
export function elementText(el: Element, cache?: Map<Element, string>): string {
  const cached = cache?.get(el);
  if (cached !== undefined) {
    return cached;
  }
  let text: string;
  if (SKIPPED_TAGS.has(el.tagName)) {
    text = '';
  } else if (el instanceof HTMLInputElement && ['button', 'submit', 'reset'].includes(el.type)) {
    text = el.value;
  } else {
    text = '';
    for (const node of Array.from(el.childNodes)) {
      if (node.nodeType === Node.TEXT_NODE) {
        text += node.nodeValue ?? '';
      } else if (node.nodeType === Node.ELEMENT_NODE) {
        text += elementText(node as Element, cache);
      }
    }
  }
  cache?.set(el, text);
  return text;
}

/** The innermost elements whose text matches, like Playwright's text engine. */
function textMatchesIn(root: ParentNode, spec: LocatorSpec): Element[] {
  const cache = new Map<Element, string>();
  const matches = (el: Element) => textMatches(elementText(el, cache), spec.value, spec.exact);
  return allElements(root).filter(
    (el) => matches(el) && !Array.from(el.children).some((child) => matches(child)),
  );
}

/** Hidden from assistive tech: `hidden`, `aria-hidden`, `inert`, or CSS display/visibility. */
function hiddenFromAccessibility(el: Element): boolean {
  if (el.closest('[hidden],[aria-hidden="true"],[inert]')) {
    return true;
  }
  const view = el.ownerDocument.defaultView;
  if (!view) {
    return false;
  }
  const style = view.getComputedStyle(el);
  if (style.visibility === 'hidden' || style.visibility === 'collapse') {
    return true;
  }
  for (let node: Element | null = el; node; node = node.parentElement) {
    if (view.getComputedStyle(node).display === 'none') {
      return true;
    }
  }
  return false;
}

/** Name-from-content text: skips hidden descendants and uses `alt` for images. */
function nameFromContent(el: Element): string {
  let text = '';
  for (const node of Array.from(el.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      text += node.nodeValue ?? '';
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const child = node as Element;
      if (SKIPPED_TAGS.has(child.tagName) || hiddenFromAccessibility(child)) {
        continue;
      }
      const label = child.getAttribute('aria-label');
      if (label && normalizeText(label)) {
        text += ` ${label} `;
      } else if (child instanceof HTMLImageElement) {
        text += ` ${child.alt} `;
      } else if (child instanceof HTMLInputElement && ['button', 'submit', 'reset'].includes(child.type)) {
        text += ` ${child.value} `;
      } else {
        text += nameFromContent(child);
      }
    }
  }
  return text;
}

export function roleOf(el: Element): string | undefined {
  const explicit = el.getAttribute('role')?.trim().split(/\s+/)[0];
  if (explicit) {
    return explicit;
  }
  const tag = el.tagName.toLowerCase();
  switch (tag) {
    case 'a':
    case 'area':
      return el.hasAttribute('href') ? 'link' : undefined;
    case 'button':
      return 'button';
    case 'input':
      return inputRole(el as HTMLInputElement);
    case 'textarea':
      return 'textbox';
    case 'select': {
      const select = el as HTMLSelectElement;
      return select.multiple || select.size > 1 ? 'listbox' : 'combobox';
    }
    case 'option':
      return 'option';
    case 'h1':
    case 'h2':
    case 'h3':
    case 'h4':
    case 'h5':
    case 'h6':
      return 'heading';
    case 'img':
      return el.getAttribute('alt') === '' ? 'presentation' : 'img';
    case 'ul':
    case 'ol':
      return 'list';
    case 'li':
      return 'listitem';
    case 'nav':
      return 'navigation';
    case 'main':
      return 'main';
    case 'header':
      return 'banner';
    case 'footer':
      return 'contentinfo';
    case 'aside':
      return 'complementary';
    case 'form':
      return 'form';
    case 'dialog':
      return 'dialog';
    case 'table':
      return 'table';
    case 'tr':
      return 'row';
    case 'td':
      return 'cell';
    case 'th':
      return 'columnheader';
    case 'progress':
      return 'progressbar';
    case 'hr':
      return 'separator';
    case 'article':
      return 'article';
    case 'section':
      return el.hasAttribute('aria-label') || el.hasAttribute('aria-labelledby') ? 'region' : undefined;
    default:
      return undefined;
  }
}

function inputRole(input: HTMLInputElement): string | undefined {
  switch ((input.getAttribute('type') ?? 'text').toLowerCase()) {
    case 'button':
    case 'submit':
    case 'reset':
    case 'image':
      return 'button';
    case 'checkbox':
      return 'checkbox';
    case 'radio':
      return 'radio';
    case 'range':
      return 'slider';
    case 'number':
      return 'spinbutton';
    case 'search':
      return 'searchbox';
    case 'hidden':
    case 'password':
    case 'file':
    case 'color':
    case 'date':
    case 'datetime-local':
    case 'month':
    case 'time':
    case 'week':
      return undefined;
    default:
      return 'textbox';
  }
}

function labelledByText(el: Element): string | undefined {
  const ids = el.getAttribute('aria-labelledby')?.trim();
  if (!ids) {
    return undefined;
  }
  return ids
    .split(/\s+/)
    .map((id) => el.ownerDocument.getElementById(id)?.textContent ?? '')
    .join(' ');
}

function nativeLabels(el: Element): string[] {
  const labels = (el as HTMLInputElement).labels;
  return labels ? Array.from(labels).map((label) => label.textContent ?? '') : [];
}

/** Every label-ish text that names a form control, for `getByLabel`. */
function labelsOf(el: Element): string[] {
  const labels = nativeLabels(el);
  const labelledBy = labelledByText(el);
  if (labelledBy !== undefined) {
    labels.push(labelledBy);
  }
  const ariaLabel = el.getAttribute('aria-label');
  if (ariaLabel) {
    labels.push(ariaLabel);
  }
  return labels;
}

/** A simplified accessible-name computation (aria-labelledby, aria-label, labels, content, title). */
export function accessibleName(el: Element): string {
  const labelledBy = labelledByText(el);
  if (labelledBy !== undefined && normalizeText(labelledBy)) {
    return normalizeText(labelledBy);
  }
  const ariaLabel = el.getAttribute('aria-label');
  if (ariaLabel && normalizeText(ariaLabel)) {
    return normalizeText(ariaLabel);
  }
  if (el instanceof HTMLInputElement && ['button', 'submit', 'reset'].includes(el.type)) {
    return normalizeText(el.value || (el.type === 'submit' ? 'Submit' : el.type === 'reset' ? 'Reset' : ''));
  }
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
    const labels = nativeLabels(el).map(normalizeText).filter(Boolean);
    if (labels.length) {
      return labels.join(' ');
    }
  }
  if (el instanceof HTMLImageElement && el.alt) {
    return normalizeText(el.alt);
  }
  const role = roleOf(el);
  if (role && NAME_FROM_CONTENT.has(role)) {
    const text = normalizeText(nameFromContent(el));
    if (text) {
      return text;
    }
  }
  const title = el.getAttribute('title');
  if (title) {
    return normalizeText(title);
  }
  const placeholder = el.getAttribute('placeholder');
  return placeholder ? normalizeText(placeholder) : '';
}
