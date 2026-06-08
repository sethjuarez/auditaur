import { afterEach, describe, expect, it, vi } from 'vitest';
import { AuditaurExporter } from './exporter';
import { AUDITAUR_TRACE_CONTEXT_ARG, createAuditaurTraceContext, instrumentedInvoke } from './invoke';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  getCurrentWindow: vi.fn(() => ({ label: 'main' })),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mocks.invoke,
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: mocks.getCurrentWindow,
}));

describe('instrumentedInvoke', () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it('exports the reserved trace context carrier helpers for custom wrappers', () => {
    const context = createAuditaurTraceContext(
      '00112233445566778899aabbccddeeff',
      '0123456789abcdef',
    );

    expect(AUDITAUR_TRACE_CONTEXT_ARG).toBe('auditaurTraceContext');
    expect(context).toEqual({
      traceparent: '00-00112233445566778899aabbccddeeff-0123456789abcdef-01',
    });
  });

  it('sends W3C trace context across Tauri invoke without recording carrier metadata', async () => {
    mocks.invoke.mockResolvedValueOnce('ok').mockResolvedValueOnce(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64);

    const result = await instrumentedInvoke<string>(
      exporter,
      'successful_command',
      { message: 'hello' },
      16_384,
      true,
      true,
    );
    await exporter.flush();
    await exporter.shutdown();

    expect(result).toBe('ok');
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, 'successful_command', {
      message: 'hello',
    auditaurTraceContext: {
        traceparent: expect.stringMatching(
          /^00-[0-9a-f]{32}-[0-9a-f]{16}-01$/,
        ),
      },
    });

    const traceparent = mocks.invoke.mock.calls[0][1].auditaurTraceContext.traceparent as string;
    const [, traceId, spanId] = traceparent.split('-');
    expect(mocks.invoke).toHaveBeenNthCalledWith(
      2,
      'plugin:auditaur|export_otel_batch',
      {
        batch: expect.objectContaining({
          spans: [
            expect.objectContaining({
              traceId,
              spanId,
              attributes: expect.objectContaining({
                'tauri.command.args': { message: 'hello' },
                'tauri.command.args.summary': '{"message":"hello"}',
              }),
            }),
          ],
          tauriIpcCalls: [
            expect.objectContaining({
              traceId,
              spanId,
              argsJson: { message: 'hello' },
            }),
          ],
        }),
      },
    );
  });

  it('adds trace context for no-argument commands', async () => {
    mocks.invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64);

    await instrumentedInvoke<void>(exporter, 'emit_backend_event', undefined, 16_384, true, true);
    await exporter.flush();
    await exporter.shutdown();

    expect(mocks.invoke).toHaveBeenNthCalledWith(1, 'emit_backend_event', {
      auditaurTraceContext: {
        traceparent: expect.stringMatching(
          /^00-[0-9a-f]{32}-[0-9a-f]{16}-01$/,
        ),
      },
    });
    expect(mocks.invoke.mock.calls[1][1].batch.tauriIpcCalls[0].argsJson).toEqual({});
  });

  it('does not overwrite an existing reserved Auditaur arg', async () => {
    mocks.invoke.mockResolvedValueOnce('ok').mockResolvedValueOnce(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64);
    const args = {
      message: 'hello',
      auditaurTraceContext: { user: true },
    };

    await instrumentedInvoke<string>(exporter, 'successful_command', args, 16_384, true, true);
    await exporter.flush();
    await exporter.shutdown();

    expect(mocks.invoke).toHaveBeenNthCalledWith(1, 'successful_command', args);
    expect(mocks.invoke.mock.calls[1][1].batch.tauriIpcCalls[0].argsJson).toEqual(args);
  });

  it('can disable trace context propagation while keeping invoke telemetry', async () => {
    mocks.invoke.mockResolvedValueOnce('ok').mockResolvedValueOnce(undefined);
    const exporter = new AuditaurExporter(1_000_000, 64);
    const args = { message: 'hello' };

    await instrumentedInvoke<string>(exporter, 'successful_command', args, 16_384, true, false);
    await exporter.flush();
    await exporter.shutdown();

    expect(mocks.invoke).toHaveBeenNthCalledWith(1, 'successful_command', args);
    expect(mocks.invoke.mock.calls[1][1].batch.spans[0]).toMatchObject({
      name: 'tauri.invoke successful_command',
      attributes: expect.objectContaining({
        'tauri.command.args': args,
      }),
    });
  });
});
