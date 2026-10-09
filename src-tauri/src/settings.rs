use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub started: bool,
    pub completed: bool,
    pub failed: bool,
    pub orphaned: bool,
    pub quiet: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            started: false,
            completed: true,
            failed: true,
            orphaned: true,
            quiet: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(default)]
pub struct Settings {
    pub notifications: NotificationSettings,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub quiet_minutes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub list_days: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub history_days: u32,
    pub extra_state_roots: Vec<PathBuf>,
    pub autostart: bool,
    pub window: Option<WindowPlacement>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notifications: NotificationSettings::default(),
            quiet_minutes: 10,
            list_days: 3,
            history_days: 30,
            extra_state_roots: Vec::new(),
            autostart: false,
            window: None,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=120).contains(&self.quiet_minutes) {
            return Err("応答なしまでの分数は1〜120にしてください".into());
        }
        if !(1..=30).contains(&self.list_days) {
            return Err("一覧の期間は1〜30日にしてください".into());
        }
        if !(1..=90).contains(&self.history_days) || self.history_days < self.list_days {
            return Err("履歴の保存期間は一覧の期間以上、90日以下にしてください".into());
        }
        if self
            .extra_state_roots
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err("追加の状態ルートには絶対パスを指定してください".into());
        }
        Ok(())
    }

    pub fn monitor_config(&self) -> monitor_core::MonitorConfig {
        monitor_core::MonitorConfig {
            quiet_after: Duration::from_secs(self.quiet_minutes.saturating_mul(60)),
            history_days: self.history_days,
            extra_state_roots: self.extra_state_roots.clone(),
            ..Default::default()
        }
    }

    pub fn monitor_config_with_history(&self, history_dir: PathBuf) -> monitor_core::MonitorConfig {
        monitor_core::MonitorConfig {
            history_dir: Some(history_dir),
            ..self.monitor_config()
        }
    }
}

pub fn load(path: &Path) -> io::Result<Settings> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(settings) => Ok(settings),
            Err(error) => {
                let backup = path.with_file_name("settings.json.bak");
                if backup.exists() {
                    fs::remove_file(&backup)?;
                }
                fs::rename(path, backup)?;
                tracing::warn!(%error, "invalid settings moved to backup");
                Ok(Settings::default())
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error),
    }
}

pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("settings path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(settings).map_err(io::Error::other)?;
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"notifications":{"enabled":false},"future":3}"#).unwrap();
        let settings = load(&path).unwrap();
        assert!(!settings.notifications.enabled);
        assert!(settings.notifications.completed);
        assert_eq!(settings.quiet_minutes, 10);
        assert_eq!(settings.list_days, 3);
        assert_eq!(settings.history_days, 30);
    }
    #[test]
    fn damaged_file_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"{").unwrap();
        assert_eq!(load(&path).unwrap(), Settings::default());
        assert_eq!(
            fs::read(dir.path().join("settings.json.bak")).unwrap(),
            b"{"
        );
    }
    #[test]
    fn save_replaces_previous_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        save(&path, &Settings::default()).unwrap();
        let changed = Settings {
            quiet_minutes: 4,
            ..Default::default()
        };
        save(&path, &changed).unwrap();
        assert_eq!(load(&path).unwrap(), changed);
        assert!(!path.with_extension("json.tmp").exists());
    }
    #[test]
    fn rejects_out_of_range_and_relative_roots() {
        let mut s = Settings {
            quiet_minutes: 0,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.quiet_minutes = 121;
        assert!(s.validate().is_err());
        s.quiet_minutes = 10;
        s.list_days = 31;
        assert!(s.validate().is_err());
        s.list_days = 3;
        s.history_days = 0;
        assert!(s.validate().is_err());
        s.history_days = 91;
        assert!(s.validate().is_err());
        s.history_days = 2;
        assert!(s.validate().is_err());
        s.history_days = 30;
        s.extra_state_roots.push(PathBuf::from("relative"));
        assert!(s.validate().is_err());
    }
    #[test]
    fn old_recent_fields_are_ignored_and_history_reaches_monitor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{"recent_hours":168,"recent_limit":500,"list_days":7,"history_days":45}"#,
        )
        .unwrap();
        let settings = load(&path).unwrap();
        assert_eq!(settings.list_days, 7);
        assert_eq!(settings.history_days, 45);
        assert_eq!(settings.monitor_config().history_days, 45);
    }
}
