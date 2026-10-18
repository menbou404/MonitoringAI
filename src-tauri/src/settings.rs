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
pub struct ModelCombination {
    pub model: String,
    pub effort: Option<String>,
}

impl ModelCombination {
    pub fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty()
            || self.model.chars().any(char::is_control)
            || self
                .effort
                .as_ref()
                .is_some_and(|s| s.trim().is_empty() || s.chars().any(char::is_control))
        {
            return Err("モデル名と推論の強さを確認してください".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentModelSettings {
    pub auto_switch: bool,
    pub threshold: u32,
    pub base: ModelCombination,
    pub saver: ModelCombination,
    pub switched_by_app: bool,
}

impl Default for AgentModelSettings {
    fn default() -> Self {
        Self {
            auto_switch: false,
            threshold: 90,
            base: ModelCombination {
                model: "gpt-6.1-sol".into(),
                effort: None,
            },
            saver: ModelCombination {
                model: "gpt-6-luna".into(),
                effort: None,
            },
            switched_by_app: false,
        }
    }
}

impl AgentModelSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=100).contains(&self.threshold) {
            return Err("モデルを切り替えるしきい値は1〜100にしてください".into());
        }
        self.base.validate()?;
        self.saver.validate()?;
        if self.base.model == self.saver.model {
            return Err("基本と節約用には異なるモデル名を入力してください".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(default)]
pub struct AgentSettings {
    pub codex: AgentModelSettings,
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
    #[serde(rename = "runningFolderOrder")]
    #[cfg_attr(feature = "ts", ts(rename = "runningFolderOrder"))]
    pub running_folder_order: Vec<String>,
    pub extra_state_roots: Vec<PathBuf>,
    pub autostart: bool,
    pub agents: AgentSettings,
    pub window: Option<WindowPlacement>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notifications: NotificationSettings::default(),
            quiet_minutes: 10,
            list_days: 3,
            history_days: 30,
            running_folder_order: Vec::new(),
            extra_state_roots: Vec::new(),
            autostart: false,
            agents: AgentSettings::default(),
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
        self.agents.codex.validate()
    }

    pub fn monitor_config(&self) -> monitor_core::MonitorConfig {
        monitor_core::MonitorConfig {
            codex_home: crate::model_switch::codex_home(),
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

fn deserialize_settings(bytes: &[u8]) -> serde_json::Result<Settings> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    if let Some(root) = value.as_object_mut() {
        // Also accept the snake_case names actually written by the first release.
        let mut migrated = serde_json::Map::new();
        for (camel, snake, target) in [
            ("autoModelSwitch", "auto_model_switch", "autoSwitch"),
            (
                "modelSwitchThreshold",
                "model_switch_threshold",
                "threshold",
            ),
            ("baseModel", "base_model", "base"),
            ("saverModel", "saver_model", "saver"),
            (
                "modelSwitchedByApp",
                "model_switched_by_app",
                "switchedByApp",
            ),
        ] {
            let old = root.remove(camel).or_else(|| root.remove(snake));
            if let Some(old) = old {
                migrated.insert(
                    target.into(),
                    if target == "base" || target == "saver" {
                        serde_json::json!({"model":old,"effort":null})
                    } else {
                        old
                    },
                );
            }
        }
        if !migrated.is_empty() {
            let agents = root
                .entry("agents")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(agents) = agents.as_object_mut() {
                let codex = agents
                    .entry("codex")
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(codex) = codex.as_object_mut() {
                    for (key, old) in migrated {
                        codex.entry(key).or_insert(old);
                    }
                }
            }
        }
    }
    serde_json::from_value(value)
}

pub fn load(path: &Path) -> io::Result<Settings> {
    match fs::read(path) {
        Ok(bytes) => match deserialize_settings(&bytes) {
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
        assert!(settings.running_folder_order.is_empty());
        assert!(!settings.agents.codex.auto_switch);
        assert_eq!(settings.agents.codex.threshold, 90);
        assert_eq!(settings.agents.codex.base.model, "gpt-6.1-sol");
        assert_eq!(settings.agents.codex.saver.model, "gpt-6-luna");
        assert!(!settings.agents.codex.switched_by_app);
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
    #[test]
    fn running_folder_order_survives_save_and_old_settings_default_to_auto() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"list_days":3}"#).unwrap();
        assert!(load(&path).unwrap().running_folder_order.is_empty());
        let settings = Settings {
            running_folder_order: vec!["C:\\Work".into(), "unknown:フォルダが不明".into()],
            ..Settings::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(
            load(&path).unwrap().running_folder_order,
            settings.running_folder_order
        );
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("runningFolderOrder"));
    }

    #[test]
    fn validates_model_settings_and_persists_switch_ownership() {
        let mut settings = Settings::default();
        settings.agents.codex.auto_switch = true;
        settings.agents.codex.switched_by_app = true;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        for threshold in [0, 101] {
            settings.agents.codex.threshold = threshold;
            assert!(settings.validate().is_err());
        }
        settings.agents.codex.threshold = 90;
        for model in ["", " ", "bad\nmodel", "gpt-6-luna"] {
            settings.agents.codex.base.model = model.into();
            assert!(settings.validate().is_err());
        }
        settings.agents.codex.base.model = "base".into();
        settings.agents.codex.base.effort = Some("bad\neffort".into());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn migrates_both_legacy_spellings_and_drops_flat_fields_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for legacy in [
            serde_json::json!({"autoModelSwitch":true,"modelSwitchThreshold":85,"baseModel":"old-base","saverModel":"old-saver","modelSwitchedByApp":true}),
            serde_json::json!({"auto_model_switch":true,"model_switch_threshold":85,"base_model":"old-base","saver_model":"old-saver","model_switched_by_app":true}),
        ] {
            fs::write(&path, legacy.to_string()).unwrap();
            let settings = load(&path).unwrap();
            let codex = &settings.agents.codex;
            assert!(codex.auto_switch);
            assert_eq!(codex.threshold, 85);
            assert_eq!(codex.base.model, "old-base");
            assert_eq!(codex.saver.model, "old-saver");
            assert_eq!(codex.base.effort, None);
            assert!(codex.switched_by_app);
            save(&path, &settings).unwrap();
            let saved: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            for key in legacy.as_object().unwrap().keys() {
                assert!(saved.get(key).is_none());
            }
            assert_eq!(saved["agents"]["codex"]["base"]["model"], "old-base");
            assert_eq!(load(&path).unwrap(), settings);
        }
    }

    #[test]
    fn nested_settings_win_over_legacy_and_missing_fields_default() {
        let settings = deserialize_settings(br#"{"baseModel":"legacy","modelSwitchThreshold":80,"agents":{"codex":{"base":{"model":"new","effort":"high"},"threshold":95}}}"#).unwrap();
        assert_eq!(settings.agents.codex.base.model, "new");
        assert_eq!(settings.agents.codex.base.effort.as_deref(), Some("high"));
        assert_eq!(settings.agents.codex.threshold, 95);
        assert_eq!(settings.agents.codex.saver.model, "gpt-6-luna");
        assert!(!settings.agents.codex.auto_switch);
    }
}
