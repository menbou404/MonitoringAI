use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use monitor_core::{Usage, UsageReader, config_model, replace_config_model};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    notify,
    settings::{self, Settings},
    state::AppState,
    tray,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct ModelSwitchStatus {
    #[cfg_attr(feature = "ts", ts(type = "import('./Usage').Usage | null"))]
    pub usage: Option<Usage>,
    pub current_model: Option<String>,
    pub switched_by_app: bool,
    pub error: Option<String>,
}

pub fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")))
}

fn decision<'a>(settings: &'a Settings, usage: &Usage, current: Option<&str>) -> Option<&'a str> {
    if !settings.auto_model_switch {
        return None;
    }
    let high = [usage.five_hour.as_ref(), usage.week.as_ref()]
        .into_iter()
        .flatten()
        .any(|w| w.used_percent >= f64::from(settings.model_switch_threshold));
    if high && current == Some(&settings.base_model) {
        Some(&settings.saver_model)
    } else if !high
        && usage.five_hour.is_some()
        && usage.week.is_some()
        && current == Some(&settings.saver_model)
        && settings.model_switched_by_app
    {
        Some(&settings.base_model)
    } else {
        None
    }
}

fn read_config(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

fn write_model(path: &Path, expected: &str, model: &str) -> io::Result<bool> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if read_config(path)? != expected {
        return Ok(false);
    }
    let replacement = replace_config_model(expected, model).map_err(io::Error::other)?;
    let temp = path.with_file_name(format!(
        ".monitoringai-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(replacement.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if read_config(path)? != expected {
            return Ok(false);
        }
        fs::rename(&temp, path)?;
        Ok(true)
    })();
    let _ = fs::remove_file(&temp);
    result
}

#[derive(Default)]
pub struct Controller {
    usage: UsageReader,
    retry_at: i64,
    settings_dirty: bool,
    last_error: Option<String>,
}

impl Controller {
    fn update(
        &mut self,
        home: &Path,
        settings_path: &Path,
        settings: &mut Settings,
        now: i64,
    ) -> (ModelSwitchStatus, Option<String>) {
        let usage = self.usage.read(home, now);
        let path = home.join("config.toml");
        let mut status = ModelSwitchStatus {
            usage,
            switched_by_app: settings.model_switched_by_app,
            ..Default::default()
        };
        let result = (|| -> io::Result<Option<String>> {
            let text = read_config(&path)?;
            status.current_model = config_model(&text).map_err(io::Error::other)?;
            if now < self.retry_at {
                status.error = self.last_error.clone();
                return Ok(None);
            }
            if self.settings_dirty && now >= self.retry_at {
                settings::save(settings_path, settings)?;
                self.settings_dirty = false;
            }
            if settings.model_switched_by_app
                && status.current_model.as_deref() != Some(&settings.saver_model)
            {
                settings.model_switched_by_app = false;
                self.settings_dirty = true;
                settings::save(settings_path, settings)?;
                self.settings_dirty = false;
            }
            let Some(usage) = &status.usage else {
                return Ok(None);
            };
            let Some(target) =
                decision(settings, usage, status.current_model.as_deref()).map(str::to_owned)
            else {
                return Ok(None);
            };
            if !write_model(&path, &text, &target)? {
                return Ok(None);
            }
            settings.model_switched_by_app = target == settings.saver_model;
            status.current_model = Some(target.clone());
            self.settings_dirty = true;
            settings::save(settings_path, settings)?;
            self.settings_dirty = false;
            let message = if settings.model_switched_by_app {
                let (percent, window) = [
                    (usage.five_hour.as_ref(), "5時間枠"),
                    (usage.week.as_ref(), "週枠"),
                ]
                .into_iter()
                .filter_map(|(w, label)| w.map(|w| (w.used_percent, label)))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((0.0, ""));
                let percent = percent.round();
                format!(
                    "Codexの使用率が {percent:.0}%（{window}）になったため、モデルを {target} に切り替えました"
                )
            } else {
                format!("使用率が下がったため、モデルを {target} に戻しました")
            };
            Ok(Some(message))
        })();
        status.switched_by_app = settings.model_switched_by_app;
        match result {
            Ok(message) => {
                if now >= self.retry_at {
                    self.last_error = None;
                }
                (status, message)
            }
            Err(_) => {
                status.error = Some(
                    "Codexのモデル設定を読み書きできませんでした。5分後にもう一度試します".into(),
                );
                let message = (now >= self.retry_at && settings.auto_model_switch).then(|| {
                    tracing::warn!("Codex model switch failed; retry in five minutes");
                    status.error.clone().expect("error message")
                });
                self.last_error = status.error.clone();
                if now >= self.retry_at {
                    self.retry_at = now + 300;
                }
                (status, message)
            }
        }
    }

    pub fn tick(&mut self, app: &AppHandle) {
        let Some(home) = codex_home() else { return };
        let state = app.state::<AppState>();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let (status, message, changed_settings, changed) = {
            let mut snap = state.inner.lock().expect("app state");
            let old_settings = snap.settings.clone();
            let (status, message) =
                self.update(&home, &state.settings_path, &mut snap.settings, now);
            let changed = snap.model_switch != status;
            snap.model_switch = status.clone();
            let changed_settings = (old_settings != snap.settings).then(|| snap.settings.clone());
            (status, message, changed_settings, changed)
        };
        if let Some(settings) = changed_settings {
            let _ = app.emit_to("main", "settings-changed", settings);
        }
        if let Some(message) = message
            && state
                .inner
                .lock()
                .expect("app state")
                .settings
                .notifications
                .enabled
        {
            notify::model_switch(app, &message);
        }
        if changed {
            let _ = app.emit_to("main", "model-switch-changed", status);
            tray::refresh(app);
        }
    }
}

pub fn usage_line(status: &ModelSwitchStatus) -> String {
    let usage = status.usage.as_ref();
    let percent = |window: Option<&monitor_core::UsageWindow>| {
        window
            .map(|w| format!("{:.0}%", w.used_percent.round()))
            .unwrap_or_else(|| "—".into())
    };
    format!(
        "5時間 {}・週 {} / {}",
        percent(usage.and_then(|u| u.five_hour.as_ref())),
        percent(usage.and_then(|u| u.week.as_ref())),
        status.current_model.as_deref().unwrap_or("モデル不明")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::UsageWindow;

    fn usage(five: f64, week: f64) -> Usage {
        Usage {
            five_hour: Some(UsageWindow {
                used_percent: five,
                resets_at: 1000,
            }),
            week: Some(UsageWindow {
                used_percent: week,
                resets_at: 1000,
            }),
        }
    }

    #[test]
    fn threshold_both_windows_and_manual_models() {
        let mut s = Settings::default();
        assert_eq!(decision(&s, &usage(100.0, 0.0), Some(&s.base_model)), None);
        s.auto_model_switch = true;
        for (five, week, high) in [
            (89.0, 89.0, false),
            (90.0, 0.0, true),
            (0.0, 90.0, true),
            (91.0, 99.0, true),
        ] {
            assert_eq!(
                decision(&s, &usage(five, week), Some(&s.base_model)),
                high.then_some(s.saver_model.as_str())
            );
        }
        assert_eq!(decision(&s, &usage(100.0, 100.0), Some("manual")), None);
        assert_eq!(decision(&s, &usage(0.0, 0.0), Some(&s.saver_model)), None);
        s.model_switched_by_app = true;
        assert_eq!(
            decision(&s, &usage(0.0, 0.0), Some(&s.saver_model)),
            Some(s.base_model.as_str())
        );
        assert_eq!(decision(&s, &usage(0.0, 0.0), Some("manual")), None);
        assert_eq!(decision(&s, &Usage::default(), Some(&s.saver_model)), None);
    }

    #[test]
    fn atomic_write_preserves_content_and_refuses_stale_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = "model = \"base\" # keep\r\n[profiles.x]\r\nmodel = \"other\"\r\n";
        fs::write(&path, text).unwrap();
        assert!(!write_model(&path, "stale", "saver").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert!(write_model(&path, text, "saver").unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            text.replacen("base", "saver", 1)
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    fn fixture(home: &Path, five: f64, week: f64, reset: i64) {
        let dir = home.join("sessions/2026/10/16");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("rollout-fiction-thread.jsonl"),
            serde_json::json!({
                "type":"event_msg", "payload":{"type":"token_count", "rate_limits":{
                    "primary":{"used_percent":five,"window_minutes":300,"resets_at":reset},
                    "secondary":{"used_percent":week,"window_minutes":10080,"resets_at":reset}
                }}
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn controller_switches_persists_reverts_after_reset_and_respects_manual_edits() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        fixture(&home, 2.0, 92.0, 500);
        let path = home.join("config.toml");
        let settings_path = dir.path().join("settings.json");
        let mut settings = Settings {
            auto_model_switch: true,
            ..Default::default()
        };
        fs::write(
            &path,
            format!("model = \"{}\" # keep\n", settings.base_model),
        )
        .unwrap();
        let mut controller = Controller::default();
        let (status, message) = controller.update(&home, &settings_path, &mut settings, 100);
        assert_eq!(
            status.current_model.as_deref(),
            Some(settings.saver_model.as_str())
        );
        assert!(message.unwrap().contains("92%（週枠）"));
        assert!(
            settings::load(&settings_path)
                .unwrap()
                .model_switched_by_app
        );
        assert!(
            controller
                .update(&home, &settings_path, &mut settings, 101)
                .1
                .is_none()
        );
        // A new controller uses the persisted ownership flag after a restart.
        let mut settings = settings::load(&settings_path).unwrap();
        let mut controller = Controller::default();
        let (status, message) = controller.update(&home, &settings_path, &mut settings, 500);
        assert_eq!(
            status.current_model.as_deref(),
            Some(settings.base_model.as_str())
        );
        assert!(message.unwrap().contains("戻しました"));
        assert!(
            !settings::load(&settings_path)
                .unwrap()
                .model_switched_by_app
        );
        fixture(&home, 95.0, 1.0, 1000);
        controller.update(&home, &settings_path, &mut settings, 501);
        fs::write(&path, "model = \"manual\"\n").unwrap();
        let (status, message) = controller.update(&home, &settings_path, &mut settings, 502);
        assert!(!status.switched_by_app);
        assert!(message.is_none());
        fs::write(&path, format!("model = \"{}\"\n", settings.saver_model)).unwrap();
        assert!(
            controller
                .update(&home, &settings_path, &mut settings, 1000)
                .1
                .is_none()
        );
        assert_eq!(
            config_model(&fs::read_to_string(path).unwrap())
                .unwrap()
                .as_deref(),
            Some(settings.saver_model.as_str())
        );
    }

    #[test]
    fn failure_notifies_once_and_retries_after_five_minutes() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        fixture(&home, 95.0, 0.0, 1000);
        let path = home.join("config.toml");
        fs::create_dir(&path).unwrap();
        let settings_path = dir.path().join("settings.json");
        let mut settings = Settings {
            auto_model_switch: true,
            ..Default::default()
        };
        let mut controller = Controller::default();
        let (status, message) = controller.update(&home, &settings_path, &mut settings, 100);
        assert!(status.error.is_some());
        assert!(message.is_some());
        assert!(
            controller
                .update(&home, &settings_path, &mut settings, 200)
                .1
                .is_none()
        );
        assert_eq!(controller.retry_at, 400);
        fs::remove_dir(&path).unwrap();
        fs::write(&path, format!("model = \"{}\"\n", settings.base_model)).unwrap();
        let (status, message) = controller.update(&home, &settings_path, &mut settings, 399);
        assert!(status.error.is_some());
        assert!(message.is_none());
        assert!(!settings.model_switched_by_app);
        let (status, message) = controller.update(&home, &settings_path, &mut settings, 400);
        assert!(status.error.is_none());
        assert!(message.is_some());
        assert!(settings.model_switched_by_app);
    }

    #[test]
    #[cfg(windows)]
    fn failed_rename_leaves_original_and_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = "model = \"base\"\n";
        fs::write(&path, text).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(write_model(&path, text, "saver").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        // Clear only the synthetic fixture's read-only attribute for tempfile cleanup.
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
    }
}
