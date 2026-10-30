import { afterEach, describe, expect, it, vi } from 'vitest';
import { executeDriveBridgeRequest } from './drive-bridge.js';
import { disarmDrillDialogs, withDrillDialogs } from './drill-dialogs.js';
import type { DriveBridgeRequest } from './types.js';

interface OpenOptions {
  title?: string;
  multiple?: boolean;
  filters?: Array<{ name: string; extensions: string[] }>;
}

// Shaped like `@tauri-apps/plugin-dialog`, including its generic `open`.
function fakePlugin() {
  return {
    open: vi.fn(async <T extends OpenOptions>(_options?: T): Promise<string | string[] | null> => 'real-open'),
    save: vi.fn(async (_options?: { defaultPath?: string }): Promise<string | null> => 'real-save'),
    ask: vi.fn(async (_message: string, _options?: string | { title?: string }): Promise<boolean> => true),
    confirm: vi.fn(async (_message: string, _options?: string | { title?: string }): Promise<boolean> => true),
    message: vi.fn(async (_message: string, _options?: string | { title?: string }): Promise<string> => 'Ok'),
  };
}

function bridge(action: string, value?: string): Promise<Record<string, unknown>> {
  const request: DriveBridgeRequest = {
    schemaVersion: 1,
    protocolVersion: 1,
    requestId: action,
    action,
    value,
    visibleOnly: false,
    createdAtUnixNanos: 0,
  };
  return executeDriveBridgeRequest(request);
}

function queue(id: string, kind: string, respond: unknown) {
  return bridge('dialogs.queue', JSON.stringify({ id, kind, respond }));
}

afterEach(() => {
  disarmDrillDialogs();
});

describe('withDrillDialogs', () => {
  it('delegates to the plugin until a drill arms it', async () => {
    const plugin = fakePlugin();
    const dialog = withDrillDialogs(plugin);
    await expect(dialog.open({ multiple: true })).resolves.toBe('real-open');
    await expect(dialog.ask('Sure?', 'Title')).resolves.toBe(true);
    expect(plugin.open).toHaveBeenCalledWith({ multiple: true });
    expect(plugin.ask).toHaveBeenCalledWith('Sure?', 'Title');
  });

  it('answers armed dialogs in order and records what the app asked', async () => {
    const plugin = fakePlugin();
    const dialog = withDrillDialogs(plugin);
    await expect(bridge('dialogs.arm')).resolves.toMatchObject({ armed: true, wrapped: true, queue: [] });
    await queue('s1', 'save', 'C:/out/report.docx');
    await queue('s2', 'confirm', false);

    await expect(dialog.save({ defaultPath: 'report.docx' })).resolves.toBe('C:/out/report.docx');
    await expect(dialog.confirm('Overwrite?', { title: 'Export' })).resolves.toBe(false);
    expect(plugin.save).not.toHaveBeenCalled();
    expect(plugin.confirm).not.toHaveBeenCalled();

    const state = await bridge('dialogs.state');
    expect(state.queue).toEqual([]);
    expect(state.log).toMatchObject([
      { kind: 'save', outcome: 'answered', id: 's1', options: { defaultPath: 'report.docx' } },
      { kind: 'confirm', outcome: 'answered', id: 's2', options: { message: 'Overwrite?', title: 'Export' } },
    ]);
  });

  it('rejects a dialog with nothing queued instead of opening it', async () => {
    const plugin = fakePlugin();
    const dialog = withDrillDialogs(plugin);
    await bridge('dialogs.arm');
    await expect(dialog.message('Saved', 'Done')).rejects.toThrow('no response was queued');
    expect(plugin.message).not.toHaveBeenCalled();
    const state = await bridge('dialogs.state');
    expect(state.log).toMatchObject([{ kind: 'message', outcome: 'unexpected', options: { message: 'Saved', title: 'Done' } }]);
  });

  it('rejects a dialog of another kind than the next queued response, keeping the queue', async () => {
    const dialog = withDrillDialogs(fakePlugin());
    await bridge('dialogs.arm');
    await queue('o1', 'open', ['a.md']);
    await expect(dialog.save()).rejects.toThrow('next queued response is for open');
    const state = await bridge('dialogs.state');
    expect(state.queue).toMatchObject([{ id: 'o1', kind: 'open' }]);
    expect(state.log).toMatchObject([{ kind: 'save', outcome: 'mismatch', expected: 'open', id: 'o1' }]);
  });

  it('passes null through as a cancelled dialog', async () => {
    const dialog = withDrillDialogs(fakePlugin());
    await bridge('dialogs.arm');
    await queue('o1', 'open', null);
    await expect(dialog.open()).resolves.toBeNull();
  });

  it('arming resets, and disarming hands dialogs back to the plugin', async () => {
    const plugin = fakePlugin();
    const dialog = withDrillDialogs(plugin);
    await bridge('dialogs.arm');
    await queue('o1', 'open', 'x');
    await expect(bridge('dialogs.arm')).resolves.toMatchObject({ queue: [], log: [] });
    const final = await bridge('dialogs.disarm');
    expect(final).toMatchObject({ armed: true });
    await expect(dialog.open()).resolves.toBe('real-open');
    await expect(bridge('dialogs.state')).resolves.toMatchObject({ armed: false });
  });

  it('refuses malformed or unarmed queue entries', async () => {
    await expect(queue('o1', 'open', 'x')).rejects.toThrow('not armed');
    await bridge('dialogs.arm');
    await expect(queue('o1', 'print', 'x')).rejects.toThrow('needs `id`, `kind`');
    await expect(bridge('dialogs.queue', JSON.stringify({ id: 'o1', kind: 'open' }))).rejects.toThrow('`respond`');
    await expect(bridge('dialogs.arm', JSON.stringify({ leaseMs: -1 }))).rejects.toThrow('leaseMs');
  });

  it('rejects a dialog whose options miss the queued expect, before answering', async () => {
    const dialog = withDrillDialogs(fakePlugin());
    await bridge('dialogs.arm');
    await bridge(
      'dialogs.queue',
      JSON.stringify({ id: 's1', kind: 'save', respond: 'out.docx', expect: { filters: [{ extensions: ['docx'] }] } }),
    );
    await expect(dialog.save({ filters: [{ name: 'Word', extensions: ['doc'] }] } as never)).rejects.toThrow(
      'options.filters[0].extensions[0]: expected "docx", got "doc"',
    );
    const state = await bridge('dialogs.state');
    expect(state.queue).toHaveLength(1);
    expect(state.log).toMatchObject([{ outcome: 'mismatch', id: 's1', problem: expect.stringContaining('extensions') }]);

    await expect(dialog.save({ filters: [{ name: 'Word', extensions: ['docx'] }] } as never)).resolves.toBe('out.docx');
  });

  it('lapses on its own when the drill stops renewing the lease', async () => {
    vi.useFakeTimers();
    try {
      const plugin = fakePlugin();
      const dialog = withDrillDialogs(plugin);
      await bridge('dialogs.arm', JSON.stringify({ leaseMs: 1_000 }));
      await bridge('dialogs.queue', JSON.stringify({ id: 'o1', kind: 'open', respond: 'x', leaseMs: 5_000 }));
      vi.advanceTimersByTime(4_000);
      await expect(bridge('dialogs.state')).resolves.toMatchObject({ armed: true });
      vi.advanceTimersByTime(2_000);
      await expect(dialog.open()).resolves.toBe('real-open');
      await expect(bridge('dialogs.state')).resolves.toMatchObject({ armed: false, queue: [] });
    } finally {
      vi.useRealTimers();
    }
  });

  it('accepts the real @tauri-apps/plugin-dialog module', () => {
    // Compile-time check: the wrapper keeps the plugin's own signatures.
    type Plugin = typeof import('@tauri-apps/plugin-dialog');
    const typed = (plugin: Plugin) => {
      const dialog = withDrillDialogs(plugin);
      const opened: Promise<string[] | null> = dialog.open({ multiple: true });
      const saved: Promise<string | null> = dialog.save({ defaultPath: 'a.md' });
      const asked: Promise<boolean> = dialog.ask('Sure?', { kind: 'warning' });
      return [opened, saved, asked];
    };
    expect(typed).toBeTypeOf('function');
  });
});
