// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { keyDefinition, pressChord } from './keyboard.js';

function $(selector: string): HTMLElement {
  return document.querySelector(selector) as HTMLElement;
}

describe('keyDefinition', () => {
  it('matches Playwright US-layout definitions', () => {
    expect(keyDefinition('Space')).toMatchObject({ key: ' ', code: 'Space', keyCode: 32 });
    expect(keyDefinition(' ')).toMatchObject({ key: ' ', code: 'Space', keyCode: 32 });
    expect(keyDefinition('ArrowDown')).toMatchObject({ key: 'ArrowDown', code: 'ArrowDown', keyCode: 40 });
    expect(keyDefinition('Tab')).toMatchObject({ key: 'Tab', code: 'Tab', keyCode: 9 });
    expect(keyDefinition('Enter')).toMatchObject({ key: 'Enter', code: 'Enter', keyCode: 13, text: '\r' });
    expect(keyDefinition('a')).toMatchObject({ key: 'a', code: 'KeyA', keyCode: 65, text: 'a' });
    expect(keyDefinition('KeyA')).toMatchObject({ key: 'a', code: 'KeyA', keyCode: 65, shiftKey: 'A' });
    expect(keyDefinition('a').shiftKey).toBeUndefined();
    expect(keyDefinition('A')).toMatchObject({ key: 'A', code: 'KeyA', keyCode: 65 });
    expect(keyDefinition('!')).toMatchObject({ key: '!', code: 'Digit1', keyCode: 49 });
    expect(keyDefinition('Control')).toMatchObject({ code: 'ControlLeft', keyCode: 17, location: 1 });
    expect(keyDefinition('F5')).toMatchObject({ code: 'F5', keyCode: 116 });
    expect(keyDefinition('é')).toMatchObject({ key: 'é', code: '', keyCode: 0, text: 'é' });
    expect(() => keyDefinition('Spacebar')).toThrow('unknown key `Spacebar`');
  });
});

describe('pressChord', () => {
  beforeEach(() => {
    // happy-dom has no layout; treat every rendered element as having a box.
    vi.spyOn(Element.prototype, 'getClientRects').mockImplementation(
      () => [{ width: 1, height: 1 }] as unknown as DOMRectList,
    );
  });
  afterEach(() => {
    vi.restoreAllMocks();
    Reflect.deleteProperty(document, 'execCommand');
    document.body.innerHTML = '';
  });

  it('sends key, code and keyCode the way real input does (AR-25)', () => {
    document.body.innerHTML = '<button id="handle">Move row 1</button>';
    const seen: string[] = [];
    for (const type of ['keydown', 'keypress', 'keyup']) {
      $('#handle').addEventListener(type, (event) => {
        const { key, code, keyCode } = event as KeyboardEvent;
        seen.push(`${type} ${JSON.stringify(key)} ${code} ${keyCode}`);
      });
    }
    pressChord($('#handle'), 'ArrowDown');
    pressChord($('#handle'), 'Space');
    expect(seen).toEqual([
      'keydown "ArrowDown" ArrowDown 40',
      'keyup "ArrowDown" ArrowDown 40',
      'keydown " " Space 32',
      'keypress " " Space 32',
      'keyup " " Space 32',
    ]);
  });

  it('applies Shift to keys named by code, like Playwright', () => {
    document.body.innerHTML = '<div id="t" tabindex="0"></div>';
    const keys: string[] = [];
    $('#t').addEventListener('keydown', (event) => keys.push((event as KeyboardEvent).key));
    pressChord($('#t'), 'Shift+KeyA');
    pressChord($('#t'), 'Shift+Digit1');
    pressChord($('#t'), 'Shift+a');
    expect(keys).toEqual(['Shift', 'A', 'Shift', '!', 'Shift', 'a']);
  });

  it('activates buttons on Space keyup and Enter keydown, unless prevented', () => {
    document.body.innerHTML = '<button id="b">Go</button><div id="d" role="button" tabindex="0">Go</div>';
    let clicks = 0;
    $('#b').addEventListener('click', () => (clicks += 1));
    $('#d').addEventListener('click', () => (clicks += 100));
    expect(pressChord($('#b'), 'Space')).toMatchObject({ defaultAction: 'click', defaultPrevented: false });
    expect(pressChord($('#b'), 'Enter')).toMatchObject({ defaultAction: 'click' });
    // Browsers don't activate ARIA buttons; apps handle their keys.
    expect(pressChord($('#d'), 'Space')).toMatchObject({ defaultAction: 'none' });
    expect(clicks).toBe(2);

    $('#b').addEventListener('keydown', (event) => event.preventDefault());
    expect(pressChord($('#b'), 'Space')).toMatchObject({ defaultAction: 'none', defaultPrevented: true });
    expect(clicks).toBe(2);
  });

  it('toggles checkboxes on Space', () => {
    document.body.innerHTML = '<input id="c" type="checkbox">';
    pressChord($('#c'), 'Space');
    expect(($('#c') as HTMLInputElement).checked).toBe(true);
  });

  it('moves focus on Tab and Shift+Tab, firing blur (AR-25)', () => {
    document.body.innerHTML = `
      <input id="title">
      <button id="off" disabled>x</button>
      <button id="neg" tabindex="-1">x</button>
      <div style="display:none"><input id="hidden"></div>
      <input id="hidden-type" type="hidden">
      <a id="link" href="#">link</a>
      <div id="inert" inert><button>x</button></div>
      <button id="last">Last</button>`;
    const blurred: string[] = [];
    $('#title').addEventListener('blur', () => blurred.push('title'));
    $('#title').focus();

    expect(pressChord($('#title'), 'Tab')).toMatchObject({ defaultAction: 'focus' });
    expect(document.activeElement?.id).toBe('link');
    expect(blurred).toEqual(['title']);

    pressChord($('#link'), 'Tab');
    expect(document.activeElement?.id).toBe('last');
    pressChord($('#last'), 'Tab');
    expect(document.activeElement?.id).toBe('title');
    pressChord($('#title'), 'Shift+Tab');
    expect(document.activeElement?.id).toBe('last');
  });

  it('orders positive tabindex first', () => {
    document.body.innerHTML = '<input id="a"><input id="b" tabindex="2"><input id="c" tabindex="1">';
    $('#a').focus();
    pressChord($('#a'), 'Tab');
    expect(document.activeElement?.id).toBe('c');
    pressChord($('#c'), 'Tab');
    expect(document.activeElement?.id).toBe('b');
    pressChord($('#b'), 'Tab');
    expect(document.activeElement?.id).toBe('a');
  });

  it('leaves focus alone when the app prevents Tab', () => {
    document.body.innerHTML = '<input id="a"><input id="b">';
    $('#a').addEventListener('keydown', (event) => event.preventDefault());
    $('#a').focus();
    expect(pressChord($('#a'), 'Tab')).toMatchObject({ defaultAction: 'none', defaultPrevented: true });
    expect(document.activeElement?.id).toBe('a');
  });

  it('submits a form on Enter through its default button', () => {
    document.body.innerHTML = '<form id="f"><input id="name"><input id="email"><button id="save">Save</button></form>';
    let clicks = 0;
    $('#save').addEventListener('click', (event) => {
      clicks += 1;
      event.preventDefault();
    });
    expect(pressChord($('#name'), 'Enter')).toMatchObject({ defaultAction: 'submit' });
    expect(clicks).toBe(1);
  });

  it('edits text fields: characters, Backspace and select-all', () => {
    document.body.innerHTML = '<input id="i">';
    const input = $('#i') as HTMLInputElement;
    const inputs: string[] = [];
    input.addEventListener('input', () => inputs.push(input.value));
    input.focus();
    expect(pressChord(input, 'a')).toMatchObject({ defaultAction: 'insertText' });
    pressChord(input, 'Shift+KeyB');
    pressChord(input, 'Space');
    expect(input.value).toBe('aB ');
    pressChord(input, 'Backspace');
    expect(input.value).toBe('aB');
    pressChord(input, 'Control+a');
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 2]);
    pressChord(input, 'x');
    expect(input.value).toBe('x');
    expect(inputs).toEqual(['a', 'aB', 'aB ', 'aB', 'x']);
  });

  it('lets editors take over through beforeinput', () => {
    document.body.innerHTML = '<input id="i">';
    const input = $('#i') as HTMLInputElement;
    const seen: string[] = [];
    input.addEventListener('beforeinput', (event) => {
      seen.push(`${(event as InputEvent).inputType} ${(event as InputEvent).data}`);
      event.preventDefault();
    });
    expect(pressChord(input, 'q')).toMatchObject({ defaultAction: 'none', defaultPrevented: true });
    expect(seen).toEqual(['insertText q']);
    expect(input.value).toBe('');
  });

  it('inserts a line break on Enter in a textarea', () => {
    document.body.innerHTML = '<textarea id="t">a</textarea>';
    const area = $('#t') as HTMLTextAreaElement;
    area.setSelectionRange(1, 1);
    expect(pressChord(area, 'Enter')).toMatchObject({ defaultAction: 'insertLineBreak' });
    expect(area.value).toBe('a\n');
  });

  it('activates input buttons on Enter', () => {
    document.body.innerHTML = '<input id="go" type="submit" value="Go">';
    let clicks = 0;
    $('#go').addEventListener('click', () => (clicks += 1));
    expect(pressChord($('#go'), 'Enter')).toMatchObject({ defaultAction: 'click' });
    expect(clicks).toBe(1);
  });

  it('sends keyup to where the app moved focus, without activating the old control', () => {
    document.body.innerHTML = '<button id="b">Open</button><input id="search">';
    let clicks = 0;
    $('#b').addEventListener('click', () => (clicks += 1));
    $('#b').addEventListener('keydown', () => $('#search').focus());
    const ups: string[] = [];
    $('#search').addEventListener('keyup', (event) => ups.push((event as KeyboardEvent).code));
    $('#b').focus();
    pressChord($('#b'), 'Space');
    expect(ups).toEqual(['Space']);
    expect(clicks).toBe(0);
  });

  it('edits email fields when the browser editing command is unavailable', () => {
    document.body.innerHTML = '<input id="e" type="email" value="a@b.c">';
    Object.defineProperty(document, 'execCommand', { value: () => false, configurable: true });
    const email = $('#e') as HTMLInputElement;
    pressChord(email, 'o');
    expect(email.value).toBe('a@b.co');
    pressChord(email, 'Backspace');
    expect(email.value).toBe('a@b.c');
  });

  it('treats a radio group as one tab stop and skips disabled fieldsets', () => {
    document.body.innerHTML = `
      <input id="first">
      <input type="radio" name="size" id="s"><input type="radio" name="size" id="m" checked><input type="radio" name="size" id="l">
      <fieldset disabled><input id="locked"></fieldset>
      <input id="after">`;
    $('#first').focus();
    pressChord($('#first'), 'Tab');
    expect(document.activeElement?.id).toBe('m');
    pressChord($('#m'), 'Tab');
    expect(document.activeElement?.id).toBe('after');
  });

  it('does not type shortcut keys into fields', () => {
    document.body.innerHTML = '<input id="i">';
    const input = $('#i') as HTMLInputElement;
    expect(pressChord(input, 'Control+s')).toMatchObject({ defaultAction: 'none' });
    expect(input.value).toBe('');
  });
});
