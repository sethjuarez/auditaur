// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ariaSnapshot } from './aria-snapshot.js';
import { executeDriveBridgeRequest, setDriveBridgeNativeScreenshotInvokerForTests } from './drive-bridge.js';
import type { DriveBridgeRequest } from './types.js';

function snap(html: string, selector?: string): string {
  document.body.innerHTML = html;
  return ariaSnapshot(selector ? document.querySelector(selector)! : document.body);
}

function request(fields: Partial<DriveBridgeRequest>): DriveBridgeRequest {
  return {
    schemaVersion: 1,
    protocolVersion: 1,
    requestId: 'r1',
    action: 'ping',
    visibleOnly: false,
    createdAtUnixNanos: 0,
    ...fields,
  };
}

afterEach(() => {
  setDriveBridgeNativeScreenshotInvokerForTests(undefined);
  vi.restoreAllMocks();
});

describe('ariaSnapshot', () => {
  it('outlines roles, names, and props like Playwright', () => {
    expect(
      snap(`
        <h1>Settings</h1>
        <nav aria-label="Primary"><ul><li><a href="/p">Profile</a></li><li>Billing</li></ul></nav>
        <label><input type="checkbox" checked> Dark mode</label>
        <button disabled>Save</button>
        <button aria-pressed="true" aria-expanded="true">Menu</button>
        <h3 aria-level="4">Deep</h3>
      `),
    ).toBe(
      [
        '- heading "Settings" [level=1]',
        '- navigation "Primary":',
        '  - list:',
        '    - listitem:',
        '      - link "Profile"',
        '    - listitem: Billing',
        '- checkbox "Dark mode" [checked]',
        '- text: Dark mode',
        '- button "Save" [disabled]',
        '- button "Menu" [expanded] [pressed]',
        '- heading "Deep" [level=4]',
      ].join('\n'),
    );
  });

  it('shows form values instead of content', () => {
    expect(
      snap(`
        <label for="n">Name</label><input id="n" value="Ada">
        <textarea aria-label="Bio">Hi there</textarea>
        <select aria-label="Plan"><option>Free</option><option selected>Pro</option></select>
        <input aria-label="Empty">
      `),
    ).toBe(
      [
        '- text: Name',
        '- textbox "Name": Ada',
        '- textbox "Bio": Hi there',
        '- combobox "Plan": Pro',
        '- textbox "Empty"',
      ].join('\n'),
    );
  });

  it('merges text through roleless elements and skips hidden ones', () => {
    expect(
      snap(`
        <div>Hello <span>big</span> <b>world</b></div>
        <div hidden>secret</div>
        <div style="display:none">gone</div>
        <div aria-hidden="true"><button>Ghost</button></div>
        <p>Para <em>graph</em></p>
      `),
    ).toBe(['- text: Hello big world', '- paragraph: Para graph'].join('\n'));
  });

  it('keeps block boundaries between roleless text', () => {
    expect(snap('<div><div>One</div><div>Two</div></div>')).toBe('- text: One Two');
  });

  it('includes the root itself when it has a role', () => {
    expect(snap('<main><ul id="l"><li>A</li><li>B</li></ul></main>', '#l')).toBe(
      ['- list:', '  - listitem: A', '  - listitem: B'].join('\n'),
    );
  });

  it('quotes text only when YAML would misread it', () => {
    expect(snap('<p>true</p><p>42</p><p>a: b</p><p>- dash</p><p>plain words</p>')).toBe(
      [
        '- paragraph: "true"',
        '- paragraph: "42"',
        '- paragraph: "a: b"',
        '- paragraph: "- dash"',
        '- paragraph: plain words',
      ].join('\n'),
    );
  });

  it('renders mixed checkbox state and named nodes with children', () => {
    expect(
      snap(`
        <div role="checkbox" aria-checked="mixed" aria-label="All"></div>
        <button><img alt="Icon" src="x.png"> Upload</button>
      `),
    ).toBe(
      [
        '- checkbox "All" [checked=mixed]',
        '- button "Icon Upload":',
        '  - img "Icon"',
        '  - text: Upload',
      ].join('\n'),
    );
  });
});

describe('drive bridge ariaSnapshot action', () => {
  it('snapshots a locator target and enforces strict mode', async () => {
    document.body.innerHTML = '<ul><li>A</li></ul><button>One</button><button>Two</button>';
    await expect(
      executeDriveBridgeRequest(request({ action: 'ariaSnapshot', selector: 'locator:{"kind":"role","value":"list"}' })),
    ).resolves.toEqual({ snapshot: '- list:\n  - listitem: A' });
    await expect(
      executeDriveBridgeRequest(request({ action: 'ariaSnapshot', selector: 'locator:{"kind":"role","value":"button"}' })),
    ).rejects.toThrow(/strict mode violation/);
    await expect(
      executeDriveBridgeRequest(request({ action: 'ariaSnapshot', selector: '#missing' })),
    ).rejects.toThrow(/selector not found/);
  });
});

describe('drive bridge screenshot masks', () => {
  it('paints masks and freezes the page only during capture', async () => {
    document.body.innerHTML = '<p class="clock">12:00</p><p class="clock">12:01</p><p class="clock" hidden>x</p>';
    vi.spyOn(Element.prototype, 'getClientRects').mockImplementation(function (this: Element) {
      return (this.hasAttribute('hidden') ? [] : [{}]) as unknown as DOMRectList;
    });
    let duringCapture: { masks: number; style: boolean } | undefined;
    setDriveBridgeNativeScreenshotInvokerForTests(async () => {
      duringCapture = {
        masks: document.querySelectorAll('[data-auditaur-screenshot="mask"]').length,
        style: !!document.querySelector('style[data-auditaur-screenshot="stabilize"]'),
      };
      return { format: 'png', pngBase64: 'AA==', width: 1, height: 1, screenshotBackend: 'test' };
    });
    const reply = await executeDriveBridgeRequest(
      request({ action: 'screenshot', value: 'expect', values: ['.clock'] }),
    );
    expect(reply).toMatchObject({ masked: 2, stabilized: true, screenshotBackend: 'test' });
    expect(duringCapture).toEqual({ masks: 2, style: true });
    expect(document.querySelectorAll('[data-auditaur-screenshot]').length).toBe(0);
  });

  it('finishes finite animations and replays infinite ones, even when capture fails', async () => {
    const animation = (iterations: number, playState: string) => ({
      playState,
      effect: { getTiming: () => ({ iterations }) },
      cancel: vi.fn(),
      finish: vi.fn(),
      play: vi.fn(),
    });
    const finite = animation(1, 'running');
    const spinner = animation(Infinity, 'running');
    const paused = animation(Infinity, 'paused');
    (document as unknown as { getAnimations: () => unknown[] }).getAnimations = () => [finite, spinner, paused];
    for (const outcome of ['ok', 'error']) {
      vi.clearAllMocks();
      setDriveBridgeNativeScreenshotInvokerForTests(async () => {
        expect(spinner.play).not.toHaveBeenCalled();
        if (outcome === 'error') {
          throw new Error('capture failed');
        }
        return { format: 'png', pngBase64: 'AA==', width: 1, height: 1, screenshotBackend: 'test' };
      });
      await executeDriveBridgeRequest(request({ action: 'screenshot', value: 'expect' })).catch(() => undefined);
      expect(finite.finish).toHaveBeenCalledOnce();
      expect(finite.play).not.toHaveBeenCalled();
      expect(spinner.cancel).toHaveBeenCalledOnce();
      expect(spinner.play).toHaveBeenCalledOnce();
      expect(paused.play).not.toHaveBeenCalled();
      expect(document.querySelectorAll('[data-auditaur-screenshot]').length).toBe(0);
    }
    delete (document as unknown as { getAnimations?: unknown }).getAnimations;
  });

  it('leaves plain screenshots untouched', async () => {
    setDriveBridgeNativeScreenshotInvokerForTests(async () => ({
      format: 'png',
      pngBase64: 'AA==',
      width: 1,
      height: 1,
      screenshotBackend: 'test',
    }));
    const reply = await executeDriveBridgeRequest(request({ action: 'screenshot' }));
    expect(reply).not.toHaveProperty('masked');
  });
});
