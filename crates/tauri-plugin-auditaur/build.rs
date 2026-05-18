const COMMANDS: &[&str] = &["export_otel_batch"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
