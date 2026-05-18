import { invoke as tauriInvoke } from '@tauri-apps/api/core';

export interface AuditaurFrontendConfig {
  serviceName: string;
  serviceVersion?: string;
  instrumentConsole?: boolean;
  instrumentErrors?: boolean;
  instrumentTauriInvoke?: boolean;
  instrumentTauriEvents?: boolean;
  captureFullPayloads?: boolean;
  maxPayloadBytes?: number;
  batchIntervalMs?: number;
  maxBatchSize?: number;
}

export interface AuditaurClient {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

export async function initAuditaur(config: AuditaurFrontendConfig): Promise<AuditaurClient> {
  if (!config.serviceName.trim()) {
    throw new Error('Auditaur requires a non-empty serviceName.');
  }

  return {
    invoke<T>(command: string, args?: Record<string, unknown>) {
      return tauriInvoke<T>(command, args);
    },
  };
}
