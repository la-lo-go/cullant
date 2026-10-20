// Only the permission and settings commands reach JS. The progress calls
// (`start`, `update`, `finish`, `stop`) stay Rust-side through `run_mobile_plugin`.
const COMMANDS: &[&str] = &["request_permission", "open_settings"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
