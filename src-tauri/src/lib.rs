//! MonitoringAI's resident Tauri shell.

mod bridge;
mod commands;
mod logging;
mod model_switch;
mod notify;
mod presentation;
mod settings;
mod state;
mod tray;
mod window;

use monitor_core::Monitor;
use state::{AppSnapshot, AppState};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            window::open(app)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_job,
            commands::read_log,
            commands::watch_log,
            commands::mark_seen,
            commands::open_path,
            commands::set_settings,
            commands::set_card_order,
            commands::reset_running_order,
            commands::take_pending_focus,
        ])
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
            let history_dir = handle.path().app_local_data_dir()?.join("history");
            let (monitor, events) =
                Monitor::start(settings.monitor_config_with_history(history_dir.clone()));
            let snapshot = AppSnapshot {
                jobs: monitor.snapshot(),
                health: monitor.health(),
                unseen: BTreeSet::new(),
                settings,
                window_error: false,
                pending_focus: None,
                watched_log: None,
                card_order: BTreeMap::new(),
                model_switch: Default::default(),
            };
            app.manage(AppState {
                monitor,
                settings_path,
                history_dir,
                inner: Mutex::new(snapshot),
            });
            tray::create(handle)?;
            bridge::start(handle.clone(), events);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
                let _ = window.app_handle().state::<AppState>().watch_log(None);
            }
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
