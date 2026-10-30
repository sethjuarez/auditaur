import { isActionDisabled } from './actionability.js';

/** Thrown until every requested option exists; the runner retries it, like Playwright. */
export const OPTION_NOT_FOUND = 'option not found';
/** Thrown while a matched option is disabled; the runner retries it, like Playwright. */
export const OPTION_DISABLED = 'option is disabled';

/** Playwright's `normalizeWhiteSpace`, applied to both sides when matching a label. */
function normalize(text: string): string {
  return text.replace(/[\u200b\u00ad]/g, '').trim().replace(/\s+/g, ' ');
}

/** `HTMLOptionElement.label`: the attribute, else the text with ASCII whitespace collapsed. */
function labelOf(option: HTMLOptionElement): string {
  if (typeof option.label === 'string') return option.label;
  return option.getAttribute('label') ?? (option.textContent ?? '').replace(/[\t\n\f\r ]+/g, ' ').trim();
}

/**
 * Playwright's `selectOption` for string values: each value matches an option by value or label.
 * A single select takes the first matching option; a multiple select takes the first option for
 * each value. Nothing changes until every value has an enabled option; then the selection is
 * replaced and `input` and `change` fire. Returns the selected values.
 */
export function selectOptions(el: Element, values: string[]): string[] {
  if (!(el instanceof HTMLSelectElement)) {
    throw new Error('selector is not a select element');
  }
  let remaining = values.slice();
  const chosen: HTMLOptionElement[] = [];
  for (const option of Array.from(el.options)) {
    const matches = (value: string) =>
      value === option.value || value === labelOf(option) || normalize(value) === normalize(labelOf(option));
    if (!remaining.some(matches)) {
      continue;
    }
    if (isActionDisabled(option)) {
      throw new Error(OPTION_DISABLED);
    }
    chosen.push(option);
    if (!el.multiple) {
      remaining = [];
      break;
    }
    remaining = remaining.filter((value) => !matches(value));
  }
  if (remaining.length) {
    throw new Error(OPTION_NOT_FOUND);
  }
  for (const option of Array.from(el.options)) {
    option.selected = chosen.includes(option);
  }
  el.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return chosen.map((option) => option.value);
}
