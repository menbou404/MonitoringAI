use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use monitor_core::{
    ModelInfo, ModelsReader, Usage, UsageReader, config_string, replace_config_string,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    model_switch_history::{self, ModelAgent, ModelDefault, ModelSwitchEntry, SwitchTrigger},
    notify,
    settings::{self, AgentModelSettings, ModelCombination, Settings},
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
    pub current_effort: Option<String>,
    #[cfg_attr(
        feature = "ts",
        ts(type = "Array<import('./ModelInfo').ModelInfo> | null")
    )]
    pub models: Option<Vec<ModelInfo>>,
    pub history: Vec<ModelSwitchEntry>,
    pub switched_by_app: bool,
    pub error: Option<String>,
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub(crate) settings_dirty: bool,
}

impl ModelSwitchStatus {
    fn current_default(&self) -> ModelDefault {
        ModelDefault {
            model: self.current_model.clone(),
            effort: self.current_effort.clone(),
        }
    }
}

struct SwitchFiles<'a> {
    home: &'a Path,
    settings: &'a Path,
    history: &'a Path,
}

fn record_change(
    path: &Path,
    status: &mut ModelSwitchStatus,
    before: ModelDefault,
    trigger: SwitchTrigger,
    now: i64,
) {
    let entry = ModelSwitchEntry {
        time: now,
        agent: ModelAgent::Codex,
        trigger,
        before,
        after: status.current_default(),
        usage: status.usage.clone(),
    };
    model_switch_history::record(path, &mut status.history, entry);
}

fn manual_switch(
    files: SwitchFiles<'_>,
    settings: &mut Settings,
    status: &mut ModelSwitchStatus,
    target: &ModelCombination,
    now: i64,
) -> Result<(), String> {
    target.validate()?;
    let path = files.home.join("config.toml");
    let result = (|| -> io::Result<()> {
        let text = read_config(&path)?;
        let before = ModelDefault {
            model: config_string(&text, "model").map_err(io::Error::other)?,
            effort: config_string(&text, "model_reasoning_effort").map_err(io::Error::other)?,
        };
        if !write_model(&path, &text, target)? {
            return Err(io::Error::other("config changed before write"));
        }
        settings.agents.codex.switched_by_app = false;
        status.switched_by_app = false;
        status.current_model = Some(target.model.clone());
        status.current_effort = target.effort.clone().or_else(|| before.effort.clone());
        status.usage = UsageReader::default().read(files.home, now);
        status.error = None;
        record_change(files.history, status, before, SwitchTrigger::Manual, now);
        status.settings_dirty = true;
        match settings::save(files.settings, settings) {
            Ok(()) => status.settings_dirty = false,
            Err(error) => {
                tracing::warn!(%error, "manual model switch ownership could not be saved");
                status.error = Some("既定を変更しましたが、切り替え状態を保存できませんでした。もう一度保存を試します".into());
            }
        }
        Ok(())
    })();
    result.map_err(|error| {
        tracing::warn!(%error, "manual model switch failed");
        "Codexの既定を変更できませんでした。設定ファイルを確認して、もう一度試してください".into()
    })
}

pub fn switch_manual(
    app: &AppHandle,
    agent: ModelAgent,
    target: ModelCombination,
) -> Result<ModelSwitchStatus, String> {
    match agent {
        ModelAgent::Codex => {}
    }
    let home = codex_home().ok_or("Codexの設定フォルダが見つかりません")?;
    let state = app.state::<AppState>();
    let now = unix_now();
    let (status, settings) = {
        let mut snap = state.inner.lock().expect("app state");
        let snapshot = &mut *snap;
        let files = SwitchFiles {
            home: &home,
            settings: &state.settings_path,
            history: &state
                .history_dir
                .with_file_name("model-switch-history.json"),
        };
        manual_switch(
            files,
            &mut snapshot.settings,
            &mut snapshot.model_switch,
            &target,
            now,
        )?;
        (snapshot.model_switch.clone(), snapshot.settings.clone())
    };
    let _ = app.emit_to("main", "settings-changed", settings);
    let _ = app.emit_to("main", "model-switch-changed", &status);
    tray::refresh(app);
    Ok(status)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")))
}

fn decision<'a>(
    settings: &'a AgentModelSettings,
    usage: &Usage,
    current: Option<&str>,
) -> Option<&'a ModelCombination> {
    if !settings.auto_switch || settings.base.model == settings.saver.model {
        return None;
    }
    let high = [usage.five_hour.as_ref(), usage.week.as_ref()]
        .into_iter()
        .flatten()
        .any(|w| w.used_percent >= f64::from(settings.threshold));
    if high && current == Some(&settings.base.model) {
        Some(&settings.saver)
    } else if !high
        && usage.five_hour.is_some()
        && usage.week.is_some()
        && current == Some(&settings.saver.model)
        && settings.switched_by_app
    {
        Some(&settings.base)
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

fn write_model(path: &Path, expected: &str, target: &ModelCombination) -> io::Result<bool> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if read_config(path)? != expected {
        return Ok(false);
    }
    let mut replacement =
        replace_config_string(expected, "model", &target.model).map_err(io::Error::other)?;
    if let Some(effort) = &target.effort {
        replacement = replace_config_string(&replacement, "model_reasoning_effort", effort)
            .map_err(io::Error::other)?;
    }
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
    models: ModelsReader,
    retry_at: i64,
    last_error: Option<String>,
}

impl Controller {
    fn update(
        &mut self,
        files: SwitchFiles<'_>,
        settings: &mut Settings,
        now: i64,
        previous: &ModelSwitchStatus,
    ) -> (ModelSwitchStatus, Option<String>) {
        let usage = self.usage.read(files.home, now);
        let models = self.models.read(files.home);
        let path = files.home.join("config.toml");
        let mut status = ModelSwitchStatus {
            usage,
            models,
            history: previous.history.clone(),
            settings_dirty: previous.settings_dirty,
            switched_by_app: settings.agents.codex.switched_by_app,
            ..Default::default()
        };
        let result = (|| -> io::Result<Option<String>> {
            let text = read_config(&path)?;
            status.current_model = config_string(&text, "model").map_err(io::Error::other)?;
            status.current_effort =
                config_string(&text, "model_reasoning_effort").map_err(io::Error::other)?;
            if now < self.retry_at {
                status.error = self.last_error.clone();
                return Ok(None);
            }
            if status.settings_dirty && now >= self.retry_at {
                settings::save(files.settings, settings)?;
                status.settings_dirty = false;
            }
            if settings.agents.codex.switched_by_app
                && status.current_model.as_deref() != Some(&settings.agents.codex.saver.model)
            {
                settings.agents.codex.switched_by_app = false;
                status.settings_dirty = true;
                settings::save(files.settings, settings)?;
                status.settings_dirty = false;
            }
            let Some(usage) = status.usage.clone() else {
                return Ok(None);
            };
            let Some(target) = decision(
                &settings.agents.codex,
                &usage,
                status.current_model.as_deref(),
            )
            .cloned() else {
                return Ok(None);
            };
            if !write_model(&path, &text, &target)? {
                return Ok(None);
            }
            let before = status.current_default();
            settings.agents.codex.switched_by_app =
                target.model == settings.agents.codex.saver.model;
            status.current_model = Some(target.model.clone());
            if target.effort.is_some() {
                status.current_effort = target.effort.clone();
            }
            let trigger = if settings.agents.codex.switched_by_app {
                SwitchTrigger::AutoSaver
            } else {
                SwitchTrigger::AutoRevert
            };
            record_change(files.history, &mut status, before, trigger, now);
            status.settings_dirty = true;
            settings::save(files.settings, settings)?;
            status.settings_dirty = false;
            let target = &target.model;
            let message = if settings.agents.codex.switched_by_app {
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
        status.switched_by_app = settings.agents.codex.switched_by_app;
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
                let message =
                    (now >= self.retry_at && settings.agents.codex.auto_switch).then(|| {
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
        let now = unix_now();
        let (status, message, changed_settings, changed) = {
            let mut snap = state.inner.lock().expect("app state");
            let old_settings = snap.settings.clone();
            let files = SwitchFiles {
                home: &home,
                settings: &state.settings_path,
                history: &state
                    .history_dir
                    .with_file_name("model-switch-history.json"),
            };
            let previous = snap.model_switch.clone();
            let (status, message) = self.update(files, &mut snap.settings, now, &previous);
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

    fn combination(model: &str, effort: Option<&str>) -> ModelCombination {
        ModelCombination {
            model: model.into(),
            effort: effort.map(str::to_owned),
        }
    }

    impl Controller {
        fn test_update(
            &mut self,
            home: &Path,
            settings_path: &Path,
            settings: &mut Settings,
            now: i64,
        ) -> (ModelSwitchStatus, Option<String>) {
            let history_path = settings_path.with_file_name("model-switch-history.json");
            let previous = ModelSwitchStatus {
                history: model_switch_history::load(&history_path),
                ..Default::default()
            };
            self.update(
                SwitchFiles {
                    home,
                    settings: settings_path,
                    history: &history_path,
                },
                settings,
                now,
                &previous,
            )
        }
    }

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
        let mut s = AgentModelSettings::default();
        assert_eq!(decision(&s, &usage(100.0, 0.0), Some(&s.base.model)), None);
        s.auto_switch = true;
        for (five, week, high) in [
            (89.0, 89.0, false),
            (90.0, 0.0, true),
            (0.0, 90.0, true),
            (91.0, 99.0, true),
        ] {
            assert_eq!(
                decision(&s, &usage(five, week), Some(&s.base.model)),
                high.then_some(&s.saver)
            );
        }
        assert_eq!(decision(&s, &usage(100.0, 100.0), Some("manual")), None);
        assert_eq!(decision(&s, &usage(0.0, 0.0), Some(&s.saver.model)), None);
        s.switched_by_app = true;
        assert_eq!(
            decision(&s, &usage(0.0, 0.0), Some(&s.saver.model)),
            Some(&s.base)
        );
        assert_eq!(decision(&s, &usage(0.0, 0.0), Some("manual")), None);
        assert_eq!(decision(&s, &Usage::default(), Some(&s.saver.model)), None);
    }

    #[test]
    fn atomic_write_preserves_content_and_refuses_stale_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = "model = \"base\" # keep\r\n[profiles.x]\r\nmodel = \"other\"\r\n";
        fs::write(&path, text).unwrap();
        assert!(!write_model(&path, "stale", &combination("saver", None)).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert!(write_model(&path, text, &combination("saver", None)).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            text.replacen("base", "saver", 1)
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn writes_both_keys_or_leaves_effort_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for newline in ["\n", "\r\n"] {
            let text = [
                "# keep",
                "model = 'base' # default",
                "model_reasoning_effort = 'high' # strength",
                "[profiles.x]",
                "model = 'profile'",
                "model_reasoning_effort = 'low'",
            ]
            .join(newline);
            fs::write(&path, &text).unwrap();
            assert!(write_model(&path, &text, &combination("saver", None)).unwrap());
            let changed = fs::read_to_string(&path).unwrap();
            assert_eq!(changed, text.replace("'base'", "\"saver\""));
            assert!(write_model(&path, &changed, &combination("base", Some("medium"))).unwrap());
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                text.replace("'base'", "\"base\"")
                    .replace("'high'", "\"medium\"")
            );
            let tables = format!("# keep{newline}[profiles.x]{newline}model = 'profile'{newline}");
            fs::write(&path, &tables).unwrap();
            assert!(write_model(&path, &tables, &combination("new", Some("low"))).unwrap());
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                format!(
                    "model_reasoning_effort = \"low\"{newline}model = \"new\"{newline}{tables}"
                )
            );
        }
        let text = "model = 'base'\nmodel_reasoning_effort = 1\n";
        fs::write(&path, text).unwrap();
        assert!(write_model(&path, text, &combination("saver", Some("medium"))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn manual_switch_clears_ownership_records_actual_values_and_survives_history_failure() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        fixture(&home, 12.0, 34.0, 1000);
        let path = home.join("config.toml");
        fs::write(
            &path,
            "model = 'gpt-6-luna'\nmodel_reasoning_effort = 'high' # keep\n",
        )
        .unwrap();
        let settings_path = dir.path().join("settings.json");
        let history_path = dir.path().join("history.json");
        let files = || SwitchFiles {
            home: &home,
            settings: &settings_path,
            history: &history_path,
        };
        let mut settings = Settings::default();
        settings.agents.codex.auto_switch = true;
        settings.agents.codex.switched_by_app = true;
        let mut status = ModelSwitchStatus::default();
        manual_switch(
            files(),
            &mut settings,
            &mut status,
            &combination("gpt-6-luna", None),
            100,
        )
        .unwrap();
        assert!(!settings.agents.codex.switched_by_app);
        assert!(!status.switched_by_app);
        assert!(
            !settings::load(&settings_path)
                .unwrap()
                .agents
                .codex
                .switched_by_app
        );
        assert_eq!(status.current_effort.as_deref(), Some("high"));
        assert_eq!(status.history[0].trigger, SwitchTrigger::Manual);
        assert_eq!(status.history[0].before.effort.as_deref(), Some("high"));
        assert_eq!(status.history[0].after.effort.as_deref(), Some("high"));
        assert_eq!(status.history[0].usage, Some(usage(12.0, 34.0)));
        assert_eq!(model_switch_history::load(&history_path), status.history);
        assert_eq!(
            decision(
                &settings.agents.codex,
                &usage(0.0, 0.0),
                status.current_model.as_deref()
            ),
            None
        );
        fs::remove_file(&history_path).unwrap();
        fs::create_dir(&history_path).unwrap();
        manual_switch(
            files(),
            &mut settings,
            &mut status,
            &combination("manual", Some("medium")),
            101,
        )
        .unwrap();
        assert_eq!(status.current_model.as_deref(), Some("manual"));
        assert_eq!(status.current_effort.as_deref(), Some("medium"));
        assert_eq!(status.history[0].time, 101);
        assert_eq!(status.history.len(), 2);
        assert!(status.error.is_none());
        let saved = fs::read_to_string(&path).unwrap();
        assert!(
            manual_switch(
                files(),
                &mut settings,
                &mut status,
                &combination("bad\nmodel", None),
                102
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), saved);
        assert_eq!(status.history.len(), 2);
    }

    #[test]
    fn ownership_save_failure_is_retried_without_losing_manual_history() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("blocked");
        fs::create_dir(&settings_path).unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "model = 'before'\n").unwrap();
        let history_path = dir.path().join("history.json");
        let files = || SwitchFiles {
            home: dir.path(),
            settings: &settings_path,
            history: &history_path,
        };
        let mut settings = Settings::default();
        settings.agents.codex.switched_by_app = true;
        let mut status = ModelSwitchStatus::default();
        manual_switch(
            files(),
            &mut settings,
            &mut status,
            &combination("manual", None),
            100,
        )
        .unwrap();
        assert!(!settings.agents.codex.switched_by_app);
        assert!(status.settings_dirty);
        fs::remove_dir(&settings_path).unwrap();
        let (status, _) = Controller::default().update(files(), &mut settings, 101, &status);
        assert!(!status.settings_dirty);
        assert!(status.error.is_none());
        assert_eq!(status.history.len(), 1);
        assert!(
            !settings::load(&settings_path)
                .unwrap()
                .agents
                .codex
                .switched_by_app
        );
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
        let mut settings = Settings::default();
        settings.agents.codex.auto_switch = true;
        settings.agents.codex.base.effort = Some("high".into());
        settings.agents.codex.saver.effort = Some("low".into());
        fs::write(
            &path,
            format!("model = \"{}\" # keep\n", settings.agents.codex.base.model),
        )
        .unwrap();
        let mut controller = Controller::default();
        let (status, message) = controller.test_update(&home, &settings_path, &mut settings, 100);
        assert_eq!(
            status.current_model.as_deref(),
            Some(settings.agents.codex.saver.model.as_str())
        );
        assert!(message.unwrap().contains("92%（週枠）"));
        assert_eq!(status.current_effort.as_deref(), Some("low"));
        assert_eq!(status.history[0].trigger, SwitchTrigger::AutoSaver);
        assert_eq!(status.history[0].after.effort.as_deref(), Some("low"));
        assert!(
            settings::load(&settings_path)
                .unwrap()
                .agents
                .codex
                .switched_by_app
        );
        assert!(
            controller
                .test_update(&home, &settings_path, &mut settings, 101)
                .1
                .is_none()
        );
        // A new controller uses the persisted ownership flag after a restart.
        let mut settings = settings::load(&settings_path).unwrap();
        let mut controller = Controller::default();
        let (status, message) = controller.test_update(&home, &settings_path, &mut settings, 500);
        assert_eq!(
            status.current_model.as_deref(),
            Some(settings.agents.codex.base.model.as_str())
        );
        assert!(message.unwrap().contains("戻しました"));
        assert_eq!(status.current_effort.as_deref(), Some("high"));
        assert_eq!(status.history.len(), 2);
        assert_eq!(status.history[0].trigger, SwitchTrigger::AutoRevert);
        assert_eq!(status.history[0].before.effort.as_deref(), Some("low"));
        assert!(
            !settings::load(&settings_path)
                .unwrap()
                .agents
                .codex
                .switched_by_app
        );
        fixture(&home, 95.0, 1.0, 1000);
        controller.test_update(&home, &settings_path, &mut settings, 501);
        fs::write(&path, "model = \"manual\"\n").unwrap();
        let (status, message) = controller.test_update(&home, &settings_path, &mut settings, 502);
        assert!(!status.switched_by_app);
        assert!(message.is_none());
        fs::write(
            &path,
            format!("model = \"{}\"\n", settings.agents.codex.saver.model),
        )
        .unwrap();
        assert!(
            controller
                .test_update(&home, &settings_path, &mut settings, 1000)
                .1
                .is_none()
        );
        assert_eq!(
            config_string(&fs::read_to_string(path).unwrap(), "model")
                .unwrap()
                .as_deref(),
            Some(settings.agents.codex.saver.model.as_str())
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
        let mut settings = Settings::default();
        settings.agents.codex.auto_switch = true;
        let mut controller = Controller::default();
        let (status, message) = controller.test_update(&home, &settings_path, &mut settings, 100);
        assert!(status.error.is_some());
        assert!(message.is_some());
        assert!(
            controller
                .test_update(&home, &settings_path, &mut settings, 200)
                .1
                .is_none()
        );
        assert_eq!(controller.retry_at, 400);
        fs::remove_dir(&path).unwrap();
        fs::write(
            &path,
            format!("model = \"{}\"\n", settings.agents.codex.base.model),
        )
        .unwrap();
        let (status, message) = controller.test_update(&home, &settings_path, &mut settings, 399);
        assert!(status.error.is_some());
        assert!(message.is_none());
        assert!(!settings.agents.codex.switched_by_app);
        let (status, message) = controller.test_update(&home, &settings_path, &mut settings, 400);
        assert!(status.error.is_none());
        assert!(message.is_some());
        assert!(settings.agents.codex.switched_by_app);
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
        assert!(write_model(&path, text, &combination("saver", None)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        // Clear only the synthetic fixture's read-only attribute for tempfile cleanup.
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
    }
}
