use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DRIVE_BRIDGE_CAPABILITY: &str = "drive_bridge";
pub const DRIVE_BRIDGE_PROTOCOL_VERSION: u8 = 1;
pub const DRIVE_BRIDGE_DIR: &str = "drive-bridge";
pub const DRIVE_BRIDGE_REQUESTS_DIR: &str = "requests";
pub const DRIVE_BRIDGE_IN_FLIGHT_DIR: &str = "in-flight";
pub const DRIVE_BRIDGE_RESPONSES_DIR: &str = "responses";
pub const DRIVE_BRIDGE_STATUS_FILE: &str = "status.json";
pub const DRIVE_BRIDGE_STALE_FILE_NANOS: i64 = 60_000_000_000;
pub const DRIVE_BRIDGE_REQUEST_EVENT: &str = "auditaur://drive-bridge/request";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveBridgeStatus {
    pub schema_version: u8,
    pub protocol_version: u8,
    pub active: bool,
    pub window_label: Option<String>,
    pub registered_at_unix_nanos: i64,
    pub last_heartbeat_unix_nanos: i64,
    #[serde(default)]
    pub targets: Vec<DriveBridgeTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveBridgeTarget {
    pub target_id: String,
    pub title: String,
    pub window_label: Option<String>,
    pub active: bool,
    pub last_heartbeat_unix_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveBridgeRequest {
    pub schema_version: u8,
    pub protocol_version: u8,
    pub request_id: String,
    pub action: String,
    pub selector: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub values: Vec<String>,
    pub visible_only: bool,
    #[serde(default)]
    pub window_label: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub created_at_unix_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveBridgeResponse {
    pub schema_version: u8,
    pub protocol_version: u8,
    pub request_id: String,
    pub action: String,
    pub selector: Option<String>,
    pub visible_only: bool,
    pub ok: bool,
    pub payload: Value,
    pub error: Option<String>,
    pub completed_at_unix_nanos: i64,
}
