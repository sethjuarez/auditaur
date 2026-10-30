// @vitest-environment happy-dom
//
// Runs the shipped drive bridge (dist/) after a consumer's bundler has processed it. Bundlers
// rewrite code (esbuild lowers block-level functions to `let`, AR-9), so source tests alone can
// pass while the bundled bridge throws. Needs `npm run build` first.
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { build as esbuild } from 'esbuild';
import { rolldown } from 'rolldown';
import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';

type Execute = (request: Record<string, unknown>) => Promise<Record<string, unknown>>;

const ENTRY = resolve(import.meta.dirname, '../dist/drive-bridge.js');
const TAURI_STUB = `
export const invoke = async () => null;
export const listen = async () => () => {};
export const emit = async () => {};
export const emitTo = async () => {};
export const getCurrentWindow = () => ({ label: 'main' });
`;

// Inside the package so vitest's module runner can load the bundles; node_modules is gitignored.
const scratch = resolve(import.meta.dirname, '../node_modules/.cache/bundle-test');
mkdirSync(scratch, { recursive: true });
const stub = join(scratch, 'tauri-stub.mjs');
const isTauri = (id: string) => id.startsWith('@tauri-apps/');

const bundlers: Record<string, (outfile: string) => Promise<void>> = {
  // Vite <= 7 pre-bundles dependencies with esbuild.
  async esbuild(outfile) {
    await esbuild({
      entryPoints: [ENTRY],
      bundle: true,
      format: 'esm',
      target: 'es2020',
      outfile,
      logLevel: 'silent',
      plugins: [{ name: 'tauri-stub', setup: (b) => b.onResolve({ filter: /^@tauri-apps\// }, () => ({ path: stub })) }],
    });
  },
  // Vite 8 uses rolldown.
  async rolldown(outfile) {
    const bundle = await rolldown({
      input: ENTRY,
      logLevel: 'silent',
      plugins: [{ name: 'tauri-stub', resolveId: (id: string) => (isTauri(id) ? stub : null) }],
    });
    await bundle.write({ file: outfile, format: 'esm' });
    await bundle.close();
  },
};

const executors: Record<string, Execute> = {};

beforeAll(async () => {
  writeFileSync(stub, TAURI_STUB);
  for (const [name, bundle] of Object.entries(bundlers)) {
    const outfile = join(scratch, `${name}.mjs`);
    await bundle(outfile);
    const module = await import(/* @vite-ignore */ pathToFileURL(outfile).href);
    executors[name] = module.executeDriveBridgeRequest as Execute;
  }
});

afterAll(() => rmSync(scratch, { recursive: true, force: true }));

beforeEach(() => {
  document.body.innerHTML = `
    <button id="save">Save</button>
    <input id="name" aria-label="Name">
    <button>Edit</button><button>Edit</button>`;
});

const request = (action: string, selector?: string, value?: string) => ({
  schemaVersion: 1,
  protocolVersion: 1,
  requestId: action,
  action,
  selector,
  value,
  visibleOnly: false,
  createdAtUnixNanos: 0,
});

describe.each(Object.keys(bundlers))('drive bridge bundled by %s', (name) => {
  const execute = (action: string, selector?: string, value?: string) => executors[name](request(action, selector, value));

  it('reports missing selectors instead of throwing', async () => {
    await expect(execute('exists', '#missing')).resolves.toMatchObject({ exists: false, error: expect.stringContaining('#missing') });
    await expect(execute('text', '#missing')).resolves.toMatchObject({ found: false, text: null });
    await expect(execute('inspect', '#missing')).resolves.toMatchObject({ count: 0, element: null });
  });

  it('resolves css and locator selectors', async () => {
    await expect(execute('exists', '#save')).resolves.toMatchObject({ exists: true });
    await expect(execute('text', 'locator:{"kind":"role","value":"button","name":"Save"}')).resolves.toMatchObject({ text: 'Save' });
    await expect(execute('inspect', 'locator:{"kind":"role","value":"button","name":"Edit"}')).resolves.toMatchObject({
      count: 2,
      strict: expect.stringContaining('strict mode violation'),
    });
  });

  it('runs actions', async () => {
    let clicks = 0;
    document.querySelector('#save')!.addEventListener('click', () => (clicks += 1));
    await execute('click', '#save');
    expect(clicks).toBe(1);

    await execute('fill', '#name', 'Ada');
    expect((document.querySelector('#name') as HTMLInputElement).value).toBe('Ada');

    const keys: string[] = [];
    document.querySelector('#name')!.addEventListener('keydown', (event) => {
      const { key, ctrlKey } = event as KeyboardEvent;
      keys.push(`${key}${ctrlKey ? '+ctrl' : ''}`);
    });
    await execute('press', '#name', 'Control+s');
    expect(keys).toEqual(['Control+ctrl', 's+ctrl']);

    const codes: string[] = [];
    document.querySelector('#save')!.addEventListener('keydown', (event) => {
      const { key, code, keyCode } = event as KeyboardEvent;
      codes.push(`${JSON.stringify(key)} ${code} ${keyCode}`);
    });
    await expect(execute('press', '#save', 'Space')).resolves.toMatchObject({ code: 'Space', defaultAction: 'click' });
    expect(codes).toEqual(['" " Space 32']);
    expect(clicks).toBe(2);
  });

  it('snapshots the page', async () => {
    const reply = await execute('ariaSnapshot');
    expect(reply.snapshot).toContain('button "Save"');
  });
});
