//! MonitoringAI Tauri app. See docs/design/app-shell.md and docs/design/tray-notification.md.

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running MonitoringAI");
}
