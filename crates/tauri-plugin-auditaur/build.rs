const COMMANDS: &[&str] = &[
    "export_otel_batch",
    "register_drive_bridge",
    "poll_drive_bridge_request",
    "complete_drive_bridge_request",
    "capture_drive_bridge_screenshot",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
