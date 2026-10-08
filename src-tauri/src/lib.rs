//! MonitoringAI's resident Tauri shell.

mod bridge;
mod logging;
mod notify;
mod presentation;
mod settings;
mod state;
mod tray;
mod window;

use monitor_core::Monitor;
use state::{AppSnapshot, AppState};
use std::{collections::BTreeSet, sync::Mutex};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            window::open(app)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .setup(|app| {
            let handle = app.handle();
            let guard = logging::init(&handle.path().app_log_dir()?)?;
            app.manage(guard);
            let settings_path = handle.path().app_config_dir()?.join("settings.json");
            let mut settings = settings::load(&settings_path)?;
            match handle.autolaunch().is_enabled() {
                Ok(enabled) => settings.autostart = enabled,
                Err(error) => tracing::warn!(%error,"autostart query failed"),
            }
            let (monitor, events) = Monitor::start(settings.monitor_config());
            let snapshot = AppSnapshot {
                jobs: monitor.snapshot(),
                health: monitor.health(),
                unseen: BTreeSet::new(),
                settings,
                window_error: false,
            };
            app.manage(AppState {
                monitor,
                settings_path,
                inner: Mutex::new(snapshot),
            });
            tray::create(handle)?;
            bridge::start(handle.clone(), events);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, WindowEvent::CloseRequested { .. })
                && let Some(webview) = window.app_handle().get_webview_window("main")
            {
                window::save_placement(window.app_handle(), &webview);
            }
        })
        .build(tauri::generate_context!())
        .expect("error building MonitoringAI")
        .run(|_app, event| {
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
