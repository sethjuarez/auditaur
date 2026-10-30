import { requireEnabled } from './actionability.js';
import { insertFromPaste } from './keyboard.js';

/** What a `paste` step puts on the clipboard. The CLI reads files and sends them base64-encoded. */
export interface PastePayload {
  text?: string;
  html?: string;
  files?: PasteFile[];
}

export interface PasteFile {
  name: string;
  type: string;
  base64: string;
}

export interface PasteResult {
  /** The app called `preventDefault()` on `paste`, so it handled the paste itself. */
  prevented: boolean;
  /** The browser's default action inserted the text or HTML into an editable target. */
  inserted: boolean;
}

export function parsePastePayload(value: string): PastePayload {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw new Error('invalid paste payload: expected JSON');
  }
  const payload = parsed as PastePayload;
  const valid =
    typeof parsed === 'object' &&
    parsed !== null &&
    !Array.isArray(parsed) &&
    (payload.text === undefined || typeof payload.text === 'string') &&
    (payload.html === undefined || typeof payload.html === 'string') &&
    (payload.files === undefined ||
      (Array.isArray(payload.files) &&
        payload.files.every(
          (file) =>
            typeof file?.name === 'string' && typeof file.type === 'string' && typeof file.base64 === 'string',
        )));
  if (!valid) {
    throw new Error('invalid paste payload: expected { text?, html?, files?: [{ name, type, base64 }] }');
  }
  return payload;
}

/**
 * Paste like Ctrl+V in Chromium: focus the target, fire a cancelable `paste` ClipboardEvent whose
 * `clipboardData` holds the text, HTML and files, and, unless the app cancels it, insert into an
 * editable target the way the browser does.
 */
export function pasteInto(target: Element, payload: PastePayload): PasteResult {
  requireEnabled(target);
  (target as HTMLElement).focus?.();
  if ((target as HTMLElement).isContentEditable) {
    caretInside(target);
  }
  const transfer = clipboardData(payload);
  if (!target.dispatchEvent(pasteEvent(transfer))) {
    return { prevented: true, inserted: false };
  }
  const outcome = insertFromPaste(target, payload.text ?? '', payload.html ?? '', transfer);
  return { prevented: false, inserted: outcome === 'done' };
}

/** Keep a caret the drill already placed in the target; otherwise put it at the end. */
function caretInside(el: Element): void {
  const selection = document.getSelection();
  if (!selection || (selection.rangeCount > 0 && selection.anchorNode && el.contains(selection.anchorNode))) {
    return;
  }
  const range = document.createRange();
  range.selectNodeContents(el);
  range.collapse(false);
  selection.removeAllRanges();
  selection.addRange(range);
}

function pasteEvent(transfer: DataTransfer): Event {
  const init = { bubbles: true, cancelable: true, composed: true };
  let event: Event;
  try {
    event = new ClipboardEvent('paste', { ...init, clipboardData: transfer });
  } catch {
    event = new Event('paste', init);
  }
  if ((event as ClipboardEvent).clipboardData !== transfer) {
    Object.defineProperty(event, 'clipboardData', { value: transfer });
  }
  return event;
}

function clipboardData(payload: PastePayload): DataTransfer {
  const files = (payload.files ?? []).map((file) => new File([decodeBase64(file.base64)], file.name, { type: file.type }));
  if (typeof DataTransfer === 'function') {
    try {
      const transfer = new DataTransfer();
      if (payload.text !== undefined) transfer.setData('text/plain', payload.text);
      if (payload.html !== undefined) transfer.setData('text/html', payload.html);
      for (const file of files) transfer.items.add(file);
      return transfer;
    } catch {
      // Fall through to the stand-in for engines that can't construct one.
    }
  }
  return standInTransfer(payload, files);
}

/** The read-only parts of DataTransfer a paste handler uses. */
function standInTransfer(payload: PastePayload, files: File[]): DataTransfer {
  const strings: [string, string][] = [];
  if (payload.text !== undefined) strings.push(['text/plain', payload.text]);
  if (payload.html !== undefined) strings.push(['text/html', payload.html]);
  const items = [
    ...strings.map(([type, data]) => ({
      kind: 'string',
      type,
      getAsFile: () => null,
      getAsString: (callback: (data: string) => void) => setTimeout(() => callback(data), 0),
    })),
    ...files.map((file) => ({
      kind: 'file',
      type: file.type,
      getAsFile: () => file,
      getAsString: () => undefined,
    })),
  ];
  const fileList = Object.assign([...files], { item: (index: number) => files[index] ?? null });
  return {
    types: [...strings.map(([type]) => type), ...(files.length ? ['Files'] : [])],
    getData: (type: string) => strings.find(([kind]) => kind === type.toLowerCase())?.[1] ?? '',
    setData: () => undefined,
    clearData: () => undefined,
    files: fileList,
    items,
    dropEffect: 'none',
    effectAllowed: 'uninitialized',
  } as unknown as DataTransfer;
}

function decodeBase64(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
