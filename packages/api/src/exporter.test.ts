import { afterEach, describe, expect, it, vi } from 'vitest';
import { AuditaurExporter } from './exporter';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mocks.invoke,
}));

describe('AuditaurExporter', () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it('reports export failures and requeues retained records', async () => {
    const error = new Error('plugin unavailable');
    const onExportError = vi.fn();
    mocks.invoke.mockRejectedValueOnce(error).mockResolvedValueOnce(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64, undefined, onExportError);

    exporter.addLog({
      sessionId: '',
      timestampUnixNanos: 1,
      attributes: {},
      source: 'frontend',
    });

    await expect(exporter.flush()).rejects.toThrow(error);

    expect(onExportError).toHaveBeenCalledWith({
      error,
      attemptedRecords: 1,
      queuedRecords: 0,
      retainedRecords: 1,
      droppedRecords: 0,
    });

    await exporter.flush();
    await exporter.shutdown();

    expect(mocks.invoke).toHaveBeenLastCalledWith('plugin:auditaur|export_otel_batch', {
      batch: expect.objectContaining({
        spanEvents: [],
        logs: [
          expect.objectContaining({
            timestampUnixNanos: 1,
          }),
        ],
      }),
    });
  });

  it('keeps diagnostic callback failures from replacing export failures', async () => {
    const error = new Error('plugin unavailable');
    mocks.invoke.mockRejectedValueOnce(error).mockResolvedValueOnce(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64, undefined, () => {
      throw new Error('diagnostic failed');
    });

    exporter.addLog({
      sessionId: '',
      timestampUnixNanos: 1,
      attributes: {},
      source: 'frontend',
    });

    await expect(exporter.flush()).rejects.toThrow(error);
    await exporter.flush();
    await exporter.shutdown();
  });

  it('waits for an export already in flight before resolving a flush', async () => {
    let release: () => void = () => undefined;
    mocks.invoke
      .mockImplementationOnce(() => new Promise<void>((resolve) => { release = resolve; }))
      .mockResolvedValue(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64);
    const log = { sessionId: '', timestampUnixNanos: 1, attributes: {}, source: 'frontend' as const };

    exporter.addLog(log);
    const first = exporter.flush();
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(1));
    exporter.addLog({ ...log, timestampUnixNanos: 2 });
    let second = false;
    const flushed = exporter.flush().then(() => { second = true; });

    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(second).toBe(false);
    expect(mocks.invoke).toHaveBeenCalledTimes(1);

    release();
    await first;
    await flushed;
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    expect(mocks.invoke).toHaveBeenLastCalledWith('plugin:auditaur|export_otel_batch', {
      batch: expect.objectContaining({ logs: [expect.objectContaining({ timestampUnixNanos: 2 })] }),
    });
    await exporter.shutdown();
  });
});
