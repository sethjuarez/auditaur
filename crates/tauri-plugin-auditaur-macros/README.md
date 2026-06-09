# tauri-plugin-auditaur-macros

Procedural macros for `tauri-plugin-auditaur`.

This crate is an implementation detail of `tauri-plugin-auditaur`. Most users should depend on `tauri-plugin-auditaur` and use the re-exported macros:

```rust
#[tauri_plugin_auditaur::auditaur_command]
```

## `auditaur_command`

`auditaur_command` wraps `#[tauri::command]`, injects Auditaur's IPC carrier arguments, and adds the `tracing::instrument` fields needed to continue frontend Tauri invoke traces into backend command spans. It reads the Tauri invoke `traceparent` header first and falls back to the reserved `auditaurTraceContext` argument.

```rust
#[tauri_plugin_auditaur::auditaur_command(err)]
fn failing_command(reason: String) -> Result<(), String> {
    Err(reason)
}
```

## `instrument_ipc`

`instrument_ipc` is the lower-level compatibility macro for commands that keep an explicit `#[tauri::command]` and receive the legacy reserved invoke argument.

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
