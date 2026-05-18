use tauri::Emitter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct DogfoodEvent {
    source: &'static str,
    message: String,
}

#[tauri::command]
#[tracing::instrument]
fn successful_command(message: String) -> Result<String, String> {
    tracing::info!(auditaur.example.message = %message, "successful command invoked");
    Ok(format!("Backend received: {message}"))
}

#[tauri::command]
#[tracing::instrument(err)]
fn failing_command(reason: String) -> Result<(), String> {
    let error = format!("Intentional dogfood backend failure: {reason}");
    tracing::error!(error = %error, "failing command rejected request");
    Err(error)
}

#[tauri::command]
#[tracing::instrument(skip(app))]
fn emit_backend_event(app: tauri::AppHandle<tauri::Wry>) -> Result<(), String> {
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
