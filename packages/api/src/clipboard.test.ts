// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest';
import { ELEMENT_DISABLED } from './actionability.js';
import { parsePastePayload, pasteInto } from './clipboard.js';

function capture(el: Element): { paste?: ClipboardEvent; before?: InputEvent; inputs: string[] } {
  const seen: { paste?: ClipboardEvent; before?: InputEvent; inputs: string[] } = { inputs: [] };
  el.addEventListener('paste', (event) => (seen.paste = event as ClipboardEvent));
  el.addEventListener('beforeinput', (event) => (seen.before = event as InputEvent));
  el.addEventListener('input', (event) => seen.inputs.push((event as InputEvent).inputType));
  return seen;
}

describe('pasteInto', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('fires a cancelable paste whose clipboardData holds text, html and files', async () => {
    document.body.innerHTML = '<div id="zone" tabindex="0"></div>';
    const zone = document.querySelector('#zone')!;
    const seen = capture(zone);

    const result = pasteInto(zone, {
      text: 'plain',
      html: '<b>rich</b>',
      files: [{ name: 'a.txt', type: 'text/plain', base64: btoa('hello') }],
    });

    expect(result).toEqual({ prevented: false, inserted: false });
    expect(document.activeElement).toBe(zone);
    const data = seen.paste!.clipboardData!;
    expect(seen.paste!.cancelable).toBe(true);
    expect(data.getData('text/plain')).toBe('plain');
    expect(data.getData('text/html')).toBe('<b>rich</b>');
    expect(data.types).toEqual(expect.arrayContaining(['text/plain', 'text/html']));
    expect(data.files).toHaveLength(1);
    expect(data.files[0].name).toBe('a.txt');
    expect(data.files[0].type).toBe('text/plain');
    expect(await data.files[0].text()).toBe('hello');
  });

  it('leaves the paste to an app that cancels it', () => {
    document.body.innerHTML = '<input id="field" value="ab">';
    const field = document.querySelector('#field') as HTMLInputElement;
    field.addEventListener('paste', (event) => event.preventDefault());
    const seen = capture(field);

    expect(pasteInto(field, { text: 'x' })).toEqual({ prevented: true, inserted: false });
    expect(field.value).toBe('ab');
    expect(seen.before).toBeUndefined();
  });

  it('inserts plain text at the caret of a text field, like the browser does', () => {
    document.body.innerHTML = '<textarea id="field">ab</textarea>';
    const field = document.querySelector('#field') as HTMLTextAreaElement;
    field.setSelectionRange(1, 1);
    const seen = capture(field);

    expect(pasteInto(field, { text: 'x\ny', html: '<i>ignored</i>' })).toEqual({ prevented: false, inserted: true });
    expect(field.value).toBe('ax\nyb');
    expect(seen.before!.inputType).toBe('insertFromPaste');
    expect(seen.before!.data).toBe('x\ny');
    expect(seen.inputs).toHaveLength(1);
  });

  it('flattens line breaks for a single-line input', () => {
    document.body.innerHTML = '<input id="field">';
    const field = document.querySelector('#field') as HTMLInputElement;

    pasteInto(field, { text: 'one\r\ntwo' });

    expect(field.value).toBe('one two');
  });

  it('inserts nothing into a text field when only html was pasted', () => {
    document.body.innerHTML = '<input id="field" value="ab">';
    const field = document.querySelector('#field') as HTMLInputElement;
    const seen = capture(field);

    expect(pasteInto(field, { html: '<b>x</b>' })).toEqual({ prevented: false, inserted: false });
    expect(field.value).toBe('ab');
    expect(seen.before).toBeUndefined();
  });

  it('lets a rich-text editor handle beforeinput with the clipboard data', () => {
    document.body.innerHTML = '<div id="editor" contenteditable="true">doc</div>';
    const editor = document.querySelector('#editor')!;
    editor.addEventListener('beforeinput', (event) => event.preventDefault());
    const seen = capture(editor);

    expect(pasteInto(editor, { text: 'plain', html: '<b>rich</b>' })).toEqual({ prevented: false, inserted: false });
    expect(seen.before!.inputType).toBe('insertFromPaste');
    // Chromium sends data: null here; happy-dom reports it as ''.
    expect(seen.before!.data || null).toBeNull();
    expect(seen.before!.dataTransfer!.getData('text/html')).toBe('<b>rich</b>');
    expect(editor.textContent).toBe('doc');
  });

  it('puts the caret at the end of a contenteditable it was not already in', () => {
    document.body.innerHTML = '<input id="other"><div id="editor" contenteditable="true">doc</div>';
    const editor = document.querySelector('#editor')!;

    pasteInto(editor, { text: 'x' });

    const selection = document.getSelection()!;
    expect(editor.contains(selection.anchorNode)).toBe(true);
    expect(selection.isCollapsed).toBe(true);
  });

  it('refuses a disabled target', () => {
    document.body.innerHTML = '<input id="field" disabled>';

    expect(() => pasteInto(document.querySelector('#field')!, { text: 'x' })).toThrow(ELEMENT_DISABLED);
  });
});

describe('parsePastePayload', () => {
  it('accepts text, html and files', () => {
    expect(parsePastePayload('{"text":"a","files":[{"name":"f","type":"","base64":""}]}')).toEqual({
      text: 'a',
      files: [{ name: 'f', type: '', base64: '' }],
    });
  });

  it.each(['', 'nope', '[]', '{"text":1}', '{"files":[{"name":"f"}]}', 'null'])('rejects %j', (value) => {
    expect(() => parsePastePayload(value)).toThrow('invalid paste payload');
  });
});
