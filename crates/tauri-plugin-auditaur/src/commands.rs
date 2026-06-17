use auditaur_collector::receiver::OTelBatch;
use tauri::{AppHandle, Runtime, State};

use crate::{error::AuditaurError, state::AuditaurState};

pub const EXPORT_OTEL_BATCH_COMMAND: &str = "export_otel_batch";
pub const REGISTER_DRIVE_BRIDGE_COMMAND: &str = "register_drive_bridge";
pub const POLL_DRIVE_BRIDGE_REQUEST_COMMAND: &str = "poll_drive_bridge_request";
pub const COMPLETE_DRIVE_BRIDGE_REQUEST_COMMAND: &str = "complete_drive_bridge_request";

#[tauri::command]
pub async fn export_otel_batch(
    state: State<'_, AuditaurState>,
    batch: OTelBatch,
) -> Result<(), AuditaurError> {
    state.export_batch(batch)
}

#[tauri::command]
pub async fn register_drive_bridge<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AuditaurState>,
    window_label: Option<String>,
) -> Result<(), AuditaurError> {
    state.register_drive_bridge(window_label)?;
    if let Some((bridge_dir, alive)) = state.start_bridge_notifier_if_needed() {
        crate::start_drive_bridge_request_notifier(&app, bridge_dir, alive);
    }
    Ok(())
}

#[tauri::command]
pub async fn poll_drive_bridge_request(
    state: State<'_, AuditaurState>,
    window_label: Option<String>,
) -> Result<Option<auditaur_core::drive_bridge::DriveBridgeRequest>, AuditaurError> {
    state.poll_drive_bridge_request(window_label)
}

#[tauri::command]
pub async fn complete_drive_bridge_request(
    state: State<'_, AuditaurState>,
    response: auditaur_core::drive_bridge::DriveBridgeResponse,
) -> Result<(), AuditaurError> {
    state.complete_drive_bridge_request(response)
}
