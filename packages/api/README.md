# @auditaur/api

Frontend telemetry helpers for Auditaur-enabled Tauri apps.

Use this package in a Tauri webview to capture frontend console logs, errors, wrapped Tauri IPC calls, Tauri events, and optional OpenTelemetry JS spans. Telemetry is sent to the `tauri-plugin-auditaur` backend plugin, which stores it in Auditaur's local SQLite session database.

## Install

```powershell
npm install @auditaur/api
```

## Quick start

```ts
import { initAuditaur } from '@auditaur/api';

const auditaur = await initAuditaur({
  serviceName: 'my-tauri-app',
  onExportError(failure) {
    console.warn('Auditaur export failed', failure.error);
  },
});

export const invoke = auditaur.invoke;
```

By default, Auditaur instruments console logs, frontend errors, Tauri invokes, Tauri events, and W3C trace context propagation for wrapped invokes. Full payload capture is disabled by default.

## Custom invoke wrappers

If you build your own Tauri invoke wrapper, use the exported trace context helpers so backend commands annotated with `#[tauri_plugin_auditaur::instrument_ipc]` can continue the frontend trace:

```ts
import { AUDITAUR_TRACE_CONTEXT_ARG, createAuditaurTraceContext } from '@auditaur/api';

const args = {
  id: 'user-1',
  [AUDITAUR_TRACE_CONTEXT_ARG]: createAuditaurTraceContext(),
};
```

## OpenTelemetry JS spans

Attach Auditaur's exporter to an existing OpenTelemetry JS tracer provider:

```ts
import { BasicTracerProvider, SimpleSpanProcessor } from '@opentelemetry/sdk-trace-base';
import { initAuditaur } from '@auditaur/api';

const auditaur = await initAuditaur({ serviceName: 'my-tauri-app' });

const provider = new BasicTracerProvider({
  spanProcessors: [
    new SimpleSpanProcessor(auditaur.createOpenTelemetrySpanExporter()),
  ],
});
```

See the Auditaur docs for full setup with the Rust Tauri plugin and CLI.
