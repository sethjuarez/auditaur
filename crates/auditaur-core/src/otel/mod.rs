pub const SERVICE_NAME: &str = "service.name";
pub const SERVICE_VERSION: &str = "service.version";
pub const SERVICE_INSTANCE_ID: &str = "service.instance.id";
pub const TELEMETRY_SDK_NAME: &str = "telemetry.sdk.name";
pub const TELEMETRY_SDK_LANGUAGE: &str = "telemetry.sdk.language";
pub const TELEMETRY_SDK_VERSION: &str = "telemetry.sdk.version";
pub const AUDITAUR_SESSION_ID: &str = "auditaur.session.id";

pub fn is_auditaur_attribute(key: &str) -> bool {
    key.starts_with("auditaur.") || key.starts_with("tauri.")
}
