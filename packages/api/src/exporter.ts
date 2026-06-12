import { invoke } from '@tauri-apps/api/core';
import type { AuditaurExportFailure, FrontendErrorRecord, LogRecord, OTelBatch, SpanEventRecord, SpanRecord, TauriEventRecord, TauriIpcCallRecord } from './types';

const EXPORT_COMMAND = 'plugin:auditaur|export_otel_batch';

export class AuditaurExporter {
  private batch: OTelBatch = emptyBatch();
  private timer: ReturnType<typeof setInterval> | undefined;

  constructor(
    private readonly batchIntervalMs: number,
    private readonly maxBatchSize: number,
    private readonly command = EXPORT_COMMAND,
    private readonly onExportError?: (failure: AuditaurExportFailure) => void,
  ) {
    this.timer = setInterval(() => {
      void this.flush().catch(() => {
        // Keep exporter failures out of window.unhandledrejection instrumentation loops.
      });
    }, batchIntervalMs);
  }

  addLog(record: LogRecord): void {
    this.batch.logs.push(record);
    void this.flushIfFull();
  }

  addSpan(record: SpanRecord): void {
    this.batch.spans.push(record);
    void this.flushIfFull();
  }

  addSpanEvent(record: SpanEventRecord): void {
    this.batch.spanEvents.push(record);
    void this.flushIfFull();
  }

  addFrontendError(record: FrontendErrorRecord): void {
    this.batch.frontendErrors.push(record);
    void this.flushIfFull();
  }

  addTauriIpcCall(record: TauriIpcCallRecord): void {
    this.batch.tauriIpcCalls.push(record);
    void this.flushIfFull();
  }

  addTauriEvent(record: TauriEventRecord): void {
    this.batch.tauriEvents.push(record);
    void this.flushIfFull();
  }

  async flush(): Promise<void> {
    if (this.size === 0) {
      return;
    }
    const batch = this.batch;
    this.batch = emptyBatch();
    try {
      await invoke(this.command, { batch });
    } catch (error) {
      const queuedRecords = batchSize(this.batch);
      const attemptedRecords = batchSize(batch);
      this.batch = mergeBatches(batch, this.batch, this.maxBatchSize * 4);
      const retainedRecords = this.size;
      this.reportExportError({
        error,
        attemptedRecords,
        queuedRecords,
        retainedRecords,
        droppedRecords: Math.max(0, attemptedRecords + queuedRecords - retainedRecords),
      });
      throw error;
    }
  }

  async shutdown(): Promise<void> {
    if (this.timer !== undefined) {
      clearInterval(this.timer);
      this.timer = undefined;
    }
    await this.flush();
  }

  private get size(): number {
    return batchSize(this.batch);
  }

  private async flushIfFull(): Promise<void> {
    if (this.size >= this.maxBatchSize) {
      await this.flush().catch(() => {
        // The batch is re-queued by flush(); avoid unhandled rejection loops.
      });
    }
  }

  private reportExportError(failure: AuditaurExportFailure): void {
    try {
      this.onExportError?.(failure);
    } catch {
      // Export diagnostics must not replace the original export failure.
    }
  }
}

function batchSize(batch: OTelBatch): number {
  return batch.spans.length
    + batch.spanEvents.length
    + batch.logs.length
    + batch.frontendErrors.length
    + batch.tauriIpcCalls.length
    + batch.tauriEvents.length;
}

function mergeBatches(first: OTelBatch, second: OTelBatch, maxRecords: number): OTelBatch {
  const spans = [...first.spans, ...second.spans].slice(-maxRecords);
  const spanEvents = [...first.spanEvents, ...second.spanEvents].slice(-maxRecords);
  const logs = [...first.logs, ...second.logs].slice(-maxRecords);
  const frontendErrors = [...first.frontendErrors, ...second.frontendErrors].slice(-maxRecords);
  const tauriIpcCalls = [...first.tauriIpcCalls, ...second.tauriIpcCalls].slice(-maxRecords);
  const tauriEvents = [...first.tauriEvents, ...second.tauriEvents].slice(-maxRecords);
  return { spans, spanEvents, logs, frontendErrors, tauriIpcCalls, tauriEvents };
}

function emptyBatch(): OTelBatch {
  return { spans: [], spanEvents: [], logs: [], frontendErrors: [], tauriIpcCalls: [], tauriEvents: [] };
}
