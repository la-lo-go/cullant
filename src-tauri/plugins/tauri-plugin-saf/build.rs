// The SAF plugin exposes no JS-facing commands — every command is invoked
// Rust-side through `run_mobile_plugin`, so the command list is empty and no
// ACL permission stubs are needed (same approach as tauri-plugin-fs's
// `getFileDescriptor`).
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
