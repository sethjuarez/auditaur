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
});
