use crate::{settings::WindowPlacement, state::AppState};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn placement_visible(place: WindowPlacement, screens: &[Screen]) -> bool {
    if place.width < 720 || place.height < 480 {
        return false;
    }
    screens.iter().any(|s| {
        let left = i64::from(place.x).max(i64::from(s.x));
        let top = i64::from(place.y).max(i64::from(s.y));
        let right =
            (i64::from(place.x) + i64::from(place.width)).min(i64::from(s.x) + i64::from(s.width));
        let bottom = (i64::from(place.y) + i64::from(place.height))
            .min(i64::from(s.y) + i64::from(s.height));
        right - left >= 100 && bottom - top >= 100
    })
}

pub fn open(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window
            .unminimize()
            .and_then(|_| window.show())
            .and_then(|_| window.set_focus())
        {
            tracing::warn!(%error,"window focus failed");
        }
        return;
    }
    if let Err(error) = create(app) {
        tracing::error!(%error,"window open failed");
        app.state::<AppState>()
            .inner
            .lock()
            .expect("app state")
            .window_error = true;
        crate::tray::refresh(app);
    }
}

fn create(app: &AppHandle) -> tauri::Result<()> {
    let place = app
        .state::<AppState>()
        .inner
        .lock()
        .expect("app state")
        .settings
        .window;
    let screens: Vec<_> = app
        .available_monitors()?
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            Screen {
                x: p.x,
                y: p.y,
                width: s.width,
                height: s.height,
            }
        })
        .collect();
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("MonitoringAI")
        .inner_size(1000.0, 700.0)
        .min_inner_size(720.0, 480.0)
        .center()
        .visible(false)
        .build()?;
    if let Some(place) = place.filter(|p| placement_visible(*p, &screens)) {
        window.set_size(PhysicalSize::new(place.width, place.height))?;
        window.set_position(PhysicalPosition::new(place.x, place.y))?;
    }
    window.show()?;
    window.set_focus()?;
    app.state::<AppState>()
        .inner
        .lock()
        .expect("app state")
        .window_error = false;
    crate::tray::refresh(app);
    Ok(())
}

pub fn save_placement(app: &AppHandle, window: &WebviewWindow) {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    let state = app.state::<AppState>();
    let mut settings = state.inner.lock().expect("app state").settings.clone();
    settings.window = Some(WindowPlacement {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    });
    if let Err(error) = state.update_settings(settings) {
        tracing::warn!(%error,"window placement save failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visibility_handles_screen_edges_and_missing_monitor() {
        let screen = Screen {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let mut p = WindowPlacement {
            x: -1800,
            y: 100,
            width: 1000,
            height: 700,
        };
        assert!(placement_visible(p, &[screen]));
        p.x = 2000;
        assert!(!placement_visible(p, &[screen]));
        p.x = -50;
        assert!(!placement_visible(p, &[screen]));
        p.width = 100;
        assert!(!placement_visible(p, &[screen]));
    }
}
