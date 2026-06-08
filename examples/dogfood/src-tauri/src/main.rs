use tauri::Emitter;
use tauri_plugin_auditaur::IpcTraceContext;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct DogfoodEvent {
    source: &'static str,
    message: String,
}

#[tauri::command]
#[tauri_plugin_auditaur::instrument_ipc]
fn successful_command(
    message: String,
    auditaur_trace_context: Option<IpcTraceContext>,
) -> Result<String, String> {
    tracing::info!(auditaur.example.message = %message, "successful command invoked");
    Ok(format!("Backend received: {message}"))
}

#[tauri::command]
#[tauri_plugin_auditaur::instrument_ipc(err)]
fn failing_command(
    reason: String,
    auditaur_trace_context: Option<IpcTraceContext>,
) -> Result<(), String> {
    let error = format!("Intentional dogfood backend failure: {reason}");
    tracing::error!(error = %error, "failing command rejected request");
    Err(error)
}

#[tauri::command]
#[tauri_plugin_auditaur::instrument_ipc(skip(app))]
fn emit_backend_event(
    app: tauri::AppHandle<tauri::Wry>,
    auditaur_trace_context: Option<IpcTraceContext>,
) -> Result<(), String> {
    let payload = DogfoodEvent {
        source: "backend",
        message: "hello from Rust".to_string(),
    };
    app.emit("dogfood:backend-event", payload)
        .map_err(|error| error.to_string())?;
    tracing::info!("backend event emitted");
    Ok(())
}

fn main() {
    tracing_subscriber::registry()
        .with(tauri_plugin_auditaur::tracing_layer())
        .init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_auditaur::Builder::new()
                .service_name("auditaur-dogfood-backend")
                .session_name("dogfood-manual-smoke")
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            successful_command,
            failing_command,
            emit_backend_event
        ])
        .setup(|app| {
            tracing::info!(
                app.identifier = %app.config().identifier,
                "Auditaur dogfood app started"
            );
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Auditaur dogfood app");
}
