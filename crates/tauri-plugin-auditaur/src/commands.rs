use auditaur_collector::receiver::OTelBatch;
use tauri::State;

use crate::{error::AuditaurError, state::AuditaurState};

pub const EXPORT_OTEL_BATCH_COMMAND: &str = "export_otel_batch";

#[tauri::command]
pub async fn export_otel_batch(
    state: State<'_, AuditaurState>,
    batch: OTelBatch,
) -> Result<(), AuditaurError> {
    state.export_batch(batch)
}
