/**
 * Playwright-style ARIA snapshots: a YAML outline of the accessibility tree under an element.
 *
 *   - heading "Settings" [level=1]
 *   - list:
 *     - listitem: Profile
 *   - checkbox "Dark mode" [checked]
 *   - textbox "Name": Ada
 *
 * Roles come from {@link roleOf}; elements without a role are transparent and their text
 * merges into the parent. Hidden elements are skipped.
 */
import { accessibleName, NAME_FROM_CONTENT, normalizeText, roleOf, SKIPPED_TAGS } from './locator.js';

interface AriaNode {
  role: string;
  name: string;
  props: string[];
  value?: string;
  children: Child[];
}

type Child = AriaNode | string;

const TRANSPARENT_ROLES = new Set(['none', 'presentation', 'generic']);
const VALUE_ROLES = new Set(['textbox', 'searchbox', 'combobox', 'spinbutton', 'slider']);
const CHECKED_ROLES = new Set(['checkbox', 'radio', 'switch', 'menuitemcheckbox', 'menuitemradio']);
/** A text-only child is redundant when it repeats the name for these roles. */
const INLINE_NAME_ROLES = new Set([...NAME_FROM_CONTENT].filter((role) => role !== 'listitem'));

/** The ARIA snapshot of `root` (itself included when it has a role). */
export function ariaSnapshot(root: Element): string {
  const lines: string[] = [];
  for (const child of mergeText(visit(root))) {
    render(child, '', lines);
  }
  return lines.join('\n');
}

function visit(el: Element): Child[] {
  if (SKIPPED_TAGS.has(el.tagName) || hidden(el)) {
    return [];
  }
  const role = roleOf(el);
  const block = isBlock(el);
  if (!role || TRANSPARENT_ROLES.has(role)) {
    const children = visitChildren(el);
    return block ? [' ', ...children, ' '] : children;
  }
  // Playwright names list items only from explicit labels, not their content.
  const name = role === 'listitem' && !el.hasAttribute('aria-label') && !el.hasAttribute('aria-labelledby')
    ? ''
    : accessibleName(el);
  const node: AriaNode = { role, name, props: propsOf(el, role), children: [] };
  if (VALUE_ROLES.has(role)) {
    node.value = valueOf(el);
  } else {
    node.children = visitChildren(el);
    const only = node.children.length === 1 ? node.children[0] : undefined;
    if (typeof only === 'string' && INLINE_NAME_ROLES.has(role) && only === node.name) {
      node.children = [];
    }
  }
  return [node];
}

/** Children with adjacent text merged and whitespace normalized. */
function visitChildren(el: Element): Child[] {
  const raw: Child[] = [];
  for (const node of Array.from(el.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      raw.push(node.nodeValue ?? '');
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      raw.push(...visit(node as Element));
    }
  }
  return mergeText(raw);
}

function mergeText(raw: Child[]): Child[] {
  const merged: Child[] = [];
  for (const child of raw) {
    const last = merged[merged.length - 1];
    if (typeof child === 'string' && typeof last === 'string') {
      merged[merged.length - 1] = last + child;
    } else {
      merged.push(child);
    }
  }
  return merged
    .map((child) => (typeof child === 'string' ? normalizeText(child) : child))
    .filter((child) => child !== '');
}

function hidden(el: Element): boolean {
  if (el.hasAttribute('hidden') || el.hasAttribute('inert') || el.getAttribute('aria-hidden') === 'true') {
    return true;
  }
  const style = el.ownerDocument.defaultView?.getComputedStyle(el);
  return !!style && (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse');
}

function isBlock(el: Element): boolean {
  const display = el.ownerDocument.defaultView?.getComputedStyle(el).display;
  return !!display && !display.startsWith('inline') && display !== 'contents';
}

function propsOf(el: Element, role: string): string[] {
  const props: string[] = [];
  if (CHECKED_ROLES.has(role)) {
    const checked = checkedOf(el);
    if (checked === 'mixed') {
      props.push('checked=mixed');
    } else if (checked) {
      props.push('checked');
    }
  }
  if (el.matches(':disabled') || el.getAttribute('aria-disabled') === 'true') {
    props.push('disabled');
  }
  if (el.getAttribute('aria-expanded') === 'true') {
    props.push('expanded');
  }
  if (role === 'heading') {
    const level = Number(el.getAttribute('aria-level')) || Number(/^H([1-6])$/.exec(el.tagName)?.[1]) || 2;
    props.push(`level=${level}`);
  }
  const pressed = el.getAttribute('aria-pressed');
  if (pressed === 'mixed') {
    props.push('pressed=mixed');
  } else if (pressed === 'true') {
    props.push('pressed');
  }
  if (el.getAttribute('aria-selected') === 'true' || (el instanceof HTMLOptionElement && el.selected)) {
    props.push('selected');
  }
  return props;
}

function checkedOf(el: Element): boolean | 'mixed' {
  if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) {
    return el.indeterminate ? 'mixed' : el.checked;
  }
  const aria = el.getAttribute('aria-checked');
  return aria === 'mixed' ? 'mixed' : aria === 'true';
}

function valueOf(el: Element): string {
  if (el instanceof HTMLSelectElement) {
    return normalizeText(el.selectedOptions[0]?.textContent ?? '');
  }
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    return el.value;
  }
  return normalizeText(el.getAttribute('aria-valuetext') ?? el.getAttribute('aria-valuenow') ?? el.textContent ?? '');
}

function render(child: Child, indent: string, lines: string[]): void {
  if (typeof child === 'string') {
    lines.push(`${indent}- text: ${yamlScalar(child)}`);
    return;
  }
  let key = child.role;
  if (child.name) {
    key += ` ${JSON.stringify(child.name)}`;
  }
  for (const prop of child.props) {
    key += ` [${prop}]`;
  }
  if (child.value !== undefined) {
    lines.push(child.value ? `${indent}- ${key}: ${yamlScalar(child.value)}` : `${indent}- ${key}`);
    return;
  }
  const [only] = child.children;
  if (child.children.length === 1 && typeof only === 'string') {
    lines.push(`${indent}- ${key}: ${yamlScalar(only)}`);
    return;
  }
  if (!child.children.length) {
    lines.push(`${indent}- ${key}`);
    return;
  }
  lines.push(`${indent}- ${key}:`);
  for (const grandchild of child.children) {
    render(grandchild, `${indent}  `, lines);
  }
}

/** Quote only when plain YAML would read the text as something else. */
function yamlScalar(text: string): string {
  const needsQuotes = text === ''
    || text !== text.trim()
    || /^[\s\-?:,[\]{}#&*!|>'"%@`]/.test(text)
    || /: |:$| #/.test(text)
    || /^(true|false|yes|no|on|off|null|~|[-+]?(\d[\d_]*)?\.?\d+([eE][-+]?\d+)?|0x[0-9a-fA-F]+)$/i.test(text);
  return needsQuotes ? JSON.stringify(text) : text;
}
