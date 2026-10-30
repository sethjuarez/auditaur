import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { emit as tauriEmit, emitTo as tauriEmitTo, listen as tauriListen } from '@tauri-apps/api/event';
import { instrumentConsole } from './console.js';
import { startDriveBridge } from './drive-bridge.js';
import { instrumentErrors } from './errors.js';
import { instrumentedEmit, instrumentedEmitTo, instrumentedListen } from './events.js';
import { AuditaurExporter } from './exporter.js';
import { instrumentedInvoke } from './invoke.js';
import { createAuditaurSpanExporter } from './otel.js';
import { payloadPolicy } from './payload.js';
import type { AuditaurClient, AuditaurFrontendConfig, AuditaurInvokeArgs } from './types.js';

export type { AuditaurClient, AuditaurFrontendConfig } from './types.js';

export async function initAuditaur(config: AuditaurFrontendConfig): Promise<AuditaurClient> {
  if (!config.serviceName.trim()) {
    throw new Error('Auditaur requires a non-empty serviceName.');
  }

  const maxPayloadBytes = config.maxPayloadBytes ?? 16_384;
  const policy = payloadPolicy(config);
  const propagateTauriInvokeTraceContext = config.propagateTauriInvokeTraceContext ?? true;
  const exporter = new AuditaurExporter(
    config.batchIntervalMs ?? 1_000,
    config.maxBatchSize ?? 64,
    undefined,
    config.onExportError,
  );
  const cleanup: Array<() => void> = [];

  if (config.instrumentConsole ?? true) {
    cleanup.push(instrumentConsole(exporter, maxPayloadBytes));
  }
  if (config.instrumentErrors ?? true) {
    cleanup.push(instrumentErrors(exporter));
  }
  if (config.driveBridge) {
    cleanup.push(startDriveBridge(exporter, config.driveBridge));
  }

  return {
    invoke<T>(command: string, args?: AuditaurInvokeArgs): Promise<T> {
      if (config.instrumentTauriInvoke ?? true) {
        return instrumentedInvoke<T>(
          exporter,
          command,
          args,
          policy,
          propagateTauriInvokeTraceContext,
        );
      }
      return tauriInvoke<T>(command, args);
    },
    emit(event: string, payload?: unknown): Promise<void> {
      if (config.instrumentTauriEvents ?? true) {
        return instrumentedEmit(exporter, event, payload, policy);
      }
      return tauriEmit(event, payload);
    },
    emitTo(target: string, event: string, payload?: unknown): Promise<void> {
      if (config.instrumentTauriEvents ?? true) {
        return instrumentedEmitTo(exporter, target, event,         payload, policy);
      }
      return tauriEmitTo(target, event, payload);
    },
    listen<T>(event: string, handler: Parameters<typeof tauriListen<T>>[1]): Promise<() => void> {
      if (config.instrumentTauriEvents ?? true) {
        return instrumentedListen<T>(exporter, event, handler, policy);
      }
      return tauriListen<T>(event, handler);
    },
    createOpenTelemetrySpanExporter() {
      return createAuditaurSpanExporter({ exporter });
    },
    flush() {
      return exporter.flush();
    },
    async shutdown() {
      for (const dispose of cleanup.splice(0)) {
        dispose();
      }
      await exporter.shutdown();
    },
  };
}
