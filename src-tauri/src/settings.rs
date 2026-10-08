use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub notifications: NotificationSettings,
    pub quiet_minutes: u64,
    pub recent_hours: u64,
    pub recent_limit: usize,
    pub extra_state_roots: Vec<PathBuf>,
    pub autostart: bool,
    pub window: Option<WindowPlacement>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notifications: NotificationSettings::default(),
            quiet_minutes: 10,
            recent_hours: 24,
            recent_limit: 50,
            extra_state_roots: Vec::new(),
            autostart: false,
            window: None,
        }
    }
}

impl Settings {
    pub fn monitor_config(&self) -> monitor_core::MonitorConfig {
        monitor_core::MonitorConfig {
            quiet_after: Duration::from_secs(self.quiet_minutes.saturating_mul(60)),
            extra_state_roots: self.extra_state_roots.clone(),
            ..Default::default()
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
}
