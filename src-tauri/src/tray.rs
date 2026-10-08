use crate::{
    presentation::{self, Icon, MenuRow},
    state::AppState,
    window,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    AppHandle, Manager,
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::ManagerExt;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn bytes(icon: Icon) -> &'static [u8] {
    match icon {
        Icon::Attention => include_bytes!("../icons/tray/attention.png"),
        Icon::Done => include_bytes!("../icons/tray/done.png"),
        Icon::Idle => include_bytes!("../icons/tray/idle.png"),
        Icon::Running(1) => include_bytes!("../icons/tray/running-1.png"),
        Icon::Running(2) => include_bytes!("../icons/tray/running-2.png"),
        Icon::Running(3) => include_bytes!("../icons/tray/running-3.png"),
        Icon::Running(4) => include_bytes!("../icons/tray/running-4.png"),
        Icon::Running(5) => include_bytes!("../icons/tray/running-5.png"),
        Icon::Running(6) => include_bytes!("../icons/tray/running-6.png"),
        Icon::Running(7) => include_bytes!("../icons/tray/running-7.png"),
        Icon::Running(8) => include_bytes!("../icons/tray/running-8.png"),
        Icon::Running(9) => include_bytes!("../icons/tray/running-9.png"),
        Icon::RunningPlus => {
            include_bytes!("../icons/tray/running-9plus.png")
        }
        Icon::Running(_) => include_bytes!("../icons/tray/idle.png"),
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let icon = Image::from_bytes(bytes(Icon::Idle))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Codex: 待機中")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::open(tray.app_handle());
            }
        })
        .on_menu_event(on_menu)
        .build(app)?;
    refresh(app);
    Ok(())
}

pub fn refresh(app: &AppHandle) {
    if let Err(error) = refresh_inner(app) {
        tracing::warn!(%error,"tray refresh failed");
    }
}

fn refresh_inner(app: &AppHandle) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id("main") else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let snap = state.inner.lock().expect("app state").clone();
    let now = now_ms();
    let icon = Image::from_bytes(bytes(presentation::icon(&snap.jobs, &snap.unseen)))?;
    tray.set_icon(Some(icon))?;
    tray.set_tooltip(Some(presentation::tooltip(&snap.jobs, now)))?;
    let menu = Menu::new(app)?;
    for row in presentation::menu_rows(
        &snap.jobs,
        snap.warning().as_deref(),
        now,
        snap.settings.notifications.enabled,
        snap.settings.autostart,
    ) {
        match row {
            MenuRow::Job { id, label } => menu.append(&MenuItem::with_id(
                app,
                format!("job:{id}"),
                label,
                true,
                None::<&str>,
            )?)?,
            MenuRow::Disabled(label) => {
                menu.append(&MenuItem::new(app, label, false, None::<&str>)?)?
            }
            MenuRow::Separator => menu.append(&PredefinedMenuItem::separator(app)?)?,
            MenuRow::Warning(label) => menu.append(&MenuItem::with_id(
                app,
                "warning",
                label,
                true,
                None::<&str>,
            )?)?,
            MenuRow::Open => menu.append(&MenuItem::with_id(
                app,
                "open",
                "ウィンドウを開く",
                true,
                None::<&str>,
            )?)?,
            MenuRow::Notifications(on) => menu.append(&CheckMenuItem::with_id(
                app,
                "notifications",
                "通知",
                true,
                on,
                None::<&str>,
            )?)?,
            MenuRow::Autostart(on) => menu.append(&CheckMenuItem::with_id(
                app,
                "autostart",
                "Windows起動時に起動",
                true,
                on,
                None::<&str>,
            )?)?,
            MenuRow::Exit => {
                menu.append(&MenuItem::with_id(app, "exit", "終了", true, None::<&str>)?)?
            }
        }
    }
    tray.set_menu(Some(menu))?;
    Ok(())
}

fn on_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();
    match id {
        "exit" => app.exit(0),
        "open" | "warning" => window::open(app),
        "notifications" => {
            let state = app.state::<AppState>();
            let mut settings = state.inner.lock().expect("app state").settings.clone();
            settings.notifications.enabled = !settings.notifications.enabled;
            if let Err(error) = state.update_settings(settings) {
                tracing::warn!(%error,"settings save failed");
            }
            refresh(app);
        }
        "autostart" => {
            let manager = app.autolaunch();
            let result = manager.is_enabled().and_then(|enabled| {
                if enabled {
                    manager.disable()
                } else {
                    manager.enable()
                }
            });
            if let Err(error) = result {
                tracing::warn!(%error,"autostart change failed");
            }
            match manager.is_enabled() {
                Ok(enabled) => {
                    let state = app.state::<AppState>();
                    let mut settings = state.inner.lock().expect("app state").settings.clone();
                    settings.autostart = enabled;
                    if let Err(error) = state.update_settings(settings) {
                        tracing::warn!(%error,"settings save failed");
                    }
                }
                Err(error) => tracing::warn!(%error,"autostart query failed"),
            }
            refresh(app);
        }
        id if id.starts_with("job:") => {
            app.state::<AppState>()
                .inner
                .lock()
                .expect("app state")
                .mark_seen(&id[4..]);
            refresh(app);
            window::open(app);
        }
        _ => {}
    }
}
