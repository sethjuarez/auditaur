use tauri_plugin_auditaur_macros::auditaur_command;

struct CopilotAuthStatus;

#[auditaur_command]
async fn copilot_auth_status() -> CopilotAuthStatus {
    CopilotAuthStatus
}

fn main() {}
