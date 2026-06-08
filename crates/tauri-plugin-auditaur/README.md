# tauri-plugin-auditaur

Development-first Tauri v2 plugin for collecting Auditaur local telemetry.

Use this crate in a Tauri app to create a local Auditaur session database, receive frontend telemetry from `@auditaur/api`, record Rust `tracing` spans/logs, capture Tauri window lifecycle state, and expose the data to `auditaur-cli` and MCP tools.

## Install

```toml
[dependencies]
tauri-plugin-auditaur = "0.1"
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
use tauri_plugin_auditaur::IpcTraceContext;

#[tauri::command]
#[tauri_plugin_auditaur::instrument_ipc]
fn load_user(
    id: String,
    auditaur_trace_context: Option<IpcTraceContext>,
) -> Result<String, String> {
    tracing::info!(user.id = %id, "loading user");
    Ok(id)
}
```

The frontend wrapper sends W3C `traceparent` through the reserved `auditaurTraceContext` invoke argument. The `instrument_ipc` macro keeps normal `#[tauri::command]` compatibility while adding the `tracing::instrument` fields Auditaur needs.

## Safety note

Auditaur is development-first. Release builds are blocked unless `allow_release_builds(true)` is explicitly configured.
