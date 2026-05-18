import { invoke } from '@tauri-apps/api/core';
import type { FrontendErrorRecord, LogRecord, OTelBatch, SpanRecord } from './types';

const EXPORT_COMMAND = 'plugin:auditaur|export_otel_batch';

export class AuditaurExporter {
  private batch: OTelBatch = { spans: [], logs: [], frontendErrors: [] };
  private timer: ReturnType<typeof setInterval> | undefined;

  constructor(
    private readonly batchIntervalMs: number,
    private readonly maxBatchSize: number,
    private readonly command = EXPORT_COMMAND,
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

  addFrontendError(record: FrontendErrorRecord): void {
    this.batch.frontendErrors.push(record);
    void this.flushIfFull();
  }

  async flush(): Promise<void> {
    if (this.size === 0) {
      return;
    }
    const batch = this.batch;
    this.batch = { spans: [], logs: [], frontendErrors: [] };
    try {
      await invoke(this.command, { batch });
    } catch (error) {
      this.batch = mergeBatches(batch, this.batch, this.maxBatchSize * 4);
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
    return this.batch.spans.length + this.batch.logs.length + this.batch.frontendErrors.length;
  }

  private async flushIfFull(): Promise<void> {
    if (this.size >= this.maxBatchSize) {
      await this.flush().catch(() => {
        // The batch is re-queued by flush(); avoid unhandled rejection loops.
      });
    }
  }
}

function mergeBatches(first: OTelBatch, second: OTelBatch, maxRecords: number): OTelBatch {
  const spans = [...first.spans, ...second.spans].slice(-maxRecords);
  const logs = [...first.logs, ...second.logs].slice(-maxRecords);
  const frontendErrors = [...first.frontendErrors, ...second.frontendErrors].slice(-maxRecords);
  return { spans, logs, frontendErrors };
}
