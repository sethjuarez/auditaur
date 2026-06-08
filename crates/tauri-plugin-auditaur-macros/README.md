# tauri-plugin-auditaur-macros

Procedural macros for `tauri-plugin-auditaur`.

This crate is an implementation detail of `tauri-plugin-auditaur`. Most users should depend on `tauri-plugin-auditaur` and use the re-exported macro:

```rust
#[tauri_plugin_auditaur::instrument_ipc]
```

## `instrument_ipc`

`instrument_ipc` reduces the repeated `tracing::instrument` ceremony needed to continue frontend Tauri invoke traces into backend command spans.

```rust
use tauri_plugin_auditaur::IpcTraceContext;

#[tauri::command]
#[tauri_plugin_auditaur::instrument_ipc(err)]
fn failing_command(
    reason: String,
    auditaur_trace_context: Option<IpcTraceContext>,
) -> Result<(), String> {
    Err(reason)
}
```

The command remains a normal explicit `#[tauri::command]`. The function must keep an optional parameter named `auditaur_trace_context`.
