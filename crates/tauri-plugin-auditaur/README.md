# tauri-plugin-auditaur

Development-first Tauri v2 plugin for collecting Auditaur local telemetry.

Use this crate in a Tauri app to create a local Auditaur session database, receive frontend telemetry from `@auditaur/api`, record Rust `tracing` spans/logs, capture Tauri window lifecycle state, and expose the data to `auditaur-cli` and MCP tools.

## Install

```toml
[dependencies]
tauri-plugin-auditaur = "0.2.5"
tracing = "0.1"
tracing-subscriber = "0.3"
```

## Register the plugin

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    tracing_subscriber::registry()
        .with(tauri_plugin_auditaur::tracing_layer())
        .init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_auditaur::Builder::new()
                .service_name("my-tauri-app")
                .session_name("dev")
                .build(),
        )
        .run(tauri::generate_context!())
        .expect("failed to run app");
}
```

## IPC trace bridge

Backend commands can opt into frontend-to-backend trace continuation:

```rust
#[tauri_plugin_auditaur::auditaur_command]
fn load_user(id: String) -> Result<String, String> {
    tracing::info!(user.id = %id, "loading user");
    Ok(id)
}
```

The frontend wrapper sends W3C `traceparent` through the Tauri invoke request headers and keeps the older reserved `auditaurTraceContext` invoke argument as a compatibility fallback. The `auditaur_command` macro wraps `#[tauri::command]`, injects both IPC carriers, prefers the request header, falls back to the reserved argument, and adds the `tracing::instrument` fields Auditaur needs.

If you need to keep an explicit `#[tauri::command]`, use the lower-level `#[tauri_plugin_auditaur::instrument_ipc]` macro with an optional `auditaur_trace_context: Option<IpcTraceContext>` argument.

A healthy invoke trace has a frontend `tauri.invoke load_user` span, a `tauri_ipc_calls` row with the same trace/span ids, and a backend child span with the same trace id and `parent_span_id` set to the frontend span id. App integration tests can assert that shape with:

```rust
tauri_plugin_auditaur::test_helpers::assert_trace_stitched(
    "path/to/telemetry.sqlite",
    "load_user",
);
```

## Safety note

Auditaur is development-first. Release builds are blocked unless `allow_release_builds(true)` is explicitly configured.
