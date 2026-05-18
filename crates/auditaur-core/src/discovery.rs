use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryFile {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub service_name: String,
    pub service_version: Option<String>,
    pub app_identifier: Option<String>,
    pub pid: u32,
    pub started_at: String,
    pub database_path: String,
    pub capabilities: Vec<String>,
    pub last_heartbeat_at: String,
}
