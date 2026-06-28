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

## Debug drive bridge

For local/debug UI automation, apps can explicitly opt into the Auditaur in-app drive bridge. The bridge lets `auditaur drive` execute Tauri-native selector operations from inside the WebView on every platform:

```ts
await initAuditaur({
  serviceName: 'my-tauri-app',
  driveBridge: {
    windowLabel: 'main',
    pollIntervalMs: 100,
  },
});
```

The bridge is disabled by default, requires the Auditaur Tauri plugin commands permitted by `auditaur:default`, and should only be enabled in development/test builds. It supports `wait`, `exists`, `text`, `click`, `fill`, `type`, `press`, `hover`, `select`, `check`, `uncheck`, `evaluate`, `snapshot`, and `screenshot`; action telemetry is logged without recording filled, typed, or selected text values. Bridge screenshots first try native WebView capture for occlusion-free WebView pixels; selector screenshots crop that WebView image and include `screenshotScope=selector` plus `selectorRect`. If WebView capture fails, Auditaur falls back to native window capture and then to a DOM text summary PNG when OS permissions or window matching prevent native capture. `evaluate` runs arbitrary JavaScript in the WebView, so keep the bridge restricted to development/test sessions. The bridge is intentionally single-window for now: enable it in exactly one WebView per Auditaur session, usually with `windowLabel: 'main'`. If multiple windows enable it in the same session, target selection is unsupported and may be ambiguous.

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
