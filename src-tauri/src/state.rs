use crate::settings::{self, Settings};
use monitor_core::{EffectiveState, Health, JobSummary, Monitor, MonitorEvent};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
    sync::Mutex,
};

pub struct AppState {
    pub monitor: Monitor,
    pub settings_path: PathBuf,
    pub history_dir: PathBuf,
    pub inner: Mutex<AppSnapshot>,
}

#[derive(Clone, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct UiSnapshot {
    #[cfg_attr(feature = "ts", ts(type = "Array<import('./JobSummary').JobSummary>"))]
    pub jobs: Vec<JobSummary>,
    pub unseen: Vec<String>,
    #[cfg_attr(feature = "ts", ts(type = "import('./Health').Health"))]
    pub health: Health,
    #[cfg_attr(feature = "ts", ts(type = "import('./Settings').Settings"))]
    pub settings: Settings,
    #[cfg_attr(feature = "ts", ts(type = "Record<string, Array<string>>"))]
    pub card_order: BTreeMap<String, Vec<String>>,
    pub model_switch: crate::model_switch::ModelSwitchStatus,
}

impl AppState {
    pub fn update_model_settings(
        &self,
        mut settings: crate::settings::AgentModelSettings,
    ) -> io::Result<Settings> {
        let mut snapshot = self.inner.lock().expect("app state");
        settings.switched_by_app = snapshot.settings.agents.codex.switched_by_app;
        let mut next = snapshot.settings.clone();
        next.agents.codex = settings;
        settings::save(&self.settings_path, &next)?;
        snapshot.settings = next.clone();
        Ok(next)
    }

    pub fn snapshot(&self) -> UiSnapshot {
        let snap = self.inner.lock().expect("app state");
        snap.ui_snapshot()
    }

    pub fn watch_log(&self, id: Option<String>) -> Result<(), String> {
        let mut snap = self.inner.lock().expect("app state");
        if snap.watched_log == id {
            return Ok(());
        }
        let previous = snap.replace_watched_log(id.clone())?;
        if let Some(previous) = previous {
            self.monitor.unsubscribe_log(&previous);
        }
        if let Some(next) = &id {
            self.monitor.subscribe_log(next);
        }
        Ok(())
    }

    pub fn update_settings(&self, mut settings: Settings) -> io::Result<()> {
        let monitor_changed = {
            let mut snapshot = self.inner.lock().expect("app state");
            let changed = snapshot.settings.quiet_minutes != settings.quiet_minutes
                || snapshot.settings.history_days != settings.history_days
                || snapshot.settings.extra_state_roots != settings.extra_state_roots;
            settings.agents.codex.switched_by_app = snapshot.settings.agents.codex.switched_by_app;
            settings::save(&self.settings_path, &settings)?;
            snapshot.settings = settings.clone();
            changed
        };
        if monitor_changed {
            self.monitor
                .update_config(settings.monitor_config_with_history(self.history_dir.clone()));
        }
        Ok(())
    }

    pub fn set_card_order(&self, folder_key: &str, ids: Vec<String>) -> Result<(), String> {
        self.inner
            .lock()
            .expect("app state")
            .set_card_order(folder_key, ids)
    }

    pub fn reset_running_order(&self) -> io::Result<Settings> {
        let mut snapshot = self.inner.lock().expect("app state");
        let mut settings = snapshot.settings.clone();
        settings.running_folder_order.clear();
        settings::save(&self.settings_path, &settings)?;
        Ok(snapshot.reset_running_order())
    }
}

#[derive(Clone)]
pub struct AppSnapshot {
    pub settings: Settings,
    pub jobs: Vec<JobSummary>,
    pub health: Health,
    pub unseen: BTreeSet<String>,
    pub window_error: bool,
    pub pending_focus: Option<String>,
    pub watched_log: Option<String>,
    pub card_order: BTreeMap<String, Vec<String>>,
    pub model_switch: crate::model_switch::ModelSwitchStatus,
}

pub fn folder_key(job: &JobSummary) -> String {
    job.workspace_root
        .as_deref()
        .filter(|root| !root.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "unknown:{}",
                job.workspace_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or("フォルダが不明")
            )
        })
}

fn active(job: &JobSummary) -> bool {
    matches!(
        job.effective_state,
        EffectiveState::Queued
            | EffectiveState::Running
            | EffectiveState::Quiet
            | EffectiveState::Orphaned
    )
}

impl AppSnapshot {
    pub fn replace_watched_log(&mut self, id: Option<String>) -> Result<Option<String>, String> {
        if let Some(id) = &id
            && !self.jobs.iter().any(|job| &job.id == id)
        {
            return Err("ジョブが見つかりません".into());
        }
        if self.watched_log == id {
            return Ok(None);
        }
        Ok(std::mem::replace(&mut self.watched_log, id))
    }

    pub fn ui_snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            jobs: self.jobs.clone(),
            unseen: self.unseen.iter().cloned().collect(),
            health: self.health.clone(),
            settings: self.settings.clone(),
            card_order: self.card_order.clone(),
            model_switch: self.model_switch.clone(),
        }
    }

    pub fn set_card_order(&mut self, key: &str, ids: Vec<String>) -> Result<(), String> {
        if ids.is_empty() {
            self.card_order.remove(key);
            return Ok(());
        }
        let mut unique = BTreeSet::new();
        if !ids.iter().all(|id| unique.insert(id))
            || ids.iter().any(|id| {
                !self
                    .jobs
                    .iter()
                    .any(|job| job.id == *id && active(job) && key == folder_key(job))
            })
        {
            return Err("カードの順番に別のフォルダか終了したジョブが含まれています".into());
        }
        self.card_order.insert(key.to_owned(), ids);
        Ok(())
    }

    pub fn reset_running_order(&mut self) -> Settings {
        self.settings.running_folder_order.clear();
        self.card_order.clear();
        self.settings.clone()
    }

    pub fn take_pending_focus(&mut self) -> Option<String> {
        self.pending_focus.take()
    }

    pub fn apply(&mut self, event: &MonitorEvent, jobs: Vec<JobSummary>) {
        match event {
            MonitorEvent::StateTransition { id, to, .. } => {
                if matches!(
                    to,
                    EffectiveState::Completed | EffectiveState::Failed | EffectiveState::Orphaned
                ) {
                    self.unseen.insert(id.clone());
                } else {
                    self.unseen.remove(id);
                }
            }
            MonitorEvent::JobsChanged { removed, .. } => {
                for id in removed {
                    self.unseen.remove(id);
                    if self.watched_log.as_deref() == Some(id) {
                        self.watched_log = None;
                    }
                }
            }
            MonitorEvent::HealthChanged(health) => self.health = health.clone(),
            MonitorEvent::LogAppended { .. } => {}
        }
        self.jobs = jobs;
        let active_folders: BTreeMap<_, _> = self
            .jobs
            .iter()
            .filter(|job| active(job))
            .map(|job| (job.id.clone(), folder_key(job)))
            .collect();
        self.card_order.retain(|folder, ids| {
            ids.retain(|id| active_folders.get(id) == Some(folder));
            !ids.is_empty()
        });
    }

    pub fn mark_seen(&mut self, id: &str) {
        self.unseen.remove(id);
    }

    pub fn warning(&self) -> Option<String> {
        if self.window_error {
            return Some("ウィンドウを開けませんでした".into());
        }
        if let Some(error) = &self.health.last_error {
            return Some(error.clone());
        }
        if self.health.plugin_version.is_some() && !self.health.version_supported {
            return Some("プラグインのバージョンを確認してください".into());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_job(id: &str, root: Option<&str>, state: EffectiveState) -> JobSummary {
        JobSummary {
            id: id.into(),
            workspace_root: root.map(str::to_owned),
            workspace_name: Some("Work".into()),
            kind: None,
            kind_label: None,
            title: None,
            summary: None,
            status: "running".into(),
            phase: None,
            effective_state: state,
            created_at: None,
            started_at: None,
            completed_at: None,
            updated_at: None,
            pid: None,
            thread_id: None,
            write: None,
            session_id: None,
            latest_message: None,
            log_modified_at: None,
            context: None,
            previous_job_id: None,
            archived: false,
            archived_note: None,
            prompt_head: None,
            model: None,
            effort: None,
        }
    }
    fn test_snapshot() -> AppSnapshot {
        AppSnapshot {
            settings: Settings::default(),
            jobs: vec![],
            health: Health {
                plugin_version: None,
                version_supported: false,
                watched_roots: vec![],
                last_error: None,
            },
            unseen: BTreeSet::new(),
            window_error: false,
            pending_focus: None,
            watched_log: None,
            card_order: BTreeMap::new(),
            model_switch: Default::default(),
        }
    }
    #[test]
    fn card_order_rejects_other_folders_and_removes_finished_jobs() {
        let mut state = test_snapshot();
        state.jobs = vec![
            test_job("a", Some("C:\\Work"), EffectiveState::Running),
            test_job("b", Some("C:\\Work"), EffectiveState::Quiet),
            test_job("other", Some("D:\\Work"), EffectiveState::Running),
            test_job("unknown", None, EffectiveState::Queued),
        ];
        assert_eq!(folder_key(&state.jobs[3]), "unknown:Work");
        assert!(
            state
                .set_card_order("C:\\Work", vec!["a".into(), "other".into()])
                .is_err()
        );
        assert!(
            state
                .set_card_order("C:\\Work", vec!["a".into(), "a".into()])
                .is_err()
        );
        state
            .set_card_order("C:\\Work", vec!["b".into(), "a".into()])
            .unwrap();
        assert_eq!(state.ui_snapshot().card_order["C:\\Work"], ["b", "a"]);
        let mut jobs = state.jobs.clone();
        jobs[1].effective_state = EffectiveState::Completed;
        state.apply(
            &MonitorEvent::JobsChanged {
                changed: vec![],
                removed: vec![],
            },
            jobs,
        );
        assert_eq!(state.card_order["C:\\Work"], ["a"]);
        state.set_card_order("C:\\Work", vec![]).unwrap();
        assert!(state.card_order.is_empty());
    }
    #[test]
    fn reset_clears_folder_settings_and_card_memory() {
        let mut state = test_snapshot();
        state.settings.running_folder_order = vec!["C:\\Work".into()];
        state.card_order.insert("C:\\Work".into(), vec!["a".into()]);
        let settings = state.reset_running_order();
        assert!(settings.running_folder_order.is_empty());
        assert!(state.card_order.is_empty());
    }

    #[test]
    fn settings_updates_preserve_internal_switch_flag_and_fail_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let config = monitor_core::MonitorConfig {
            state_roots: vec![],
            installed_plugins_path: dir.path().join("version.json"),
            claude_projects_dir: None,
            ..Default::default()
        };
        let (monitor, _) = Monitor::start(config);
        let mut snapshot = test_snapshot();
        snapshot.settings.agents.codex.switched_by_app = true;
        let mut app = AppState {
            monitor,
            settings_path: dir.path().join("settings.json"),
            history_dir: dir.path().join("history"),
            inner: Mutex::new(snapshot),
        };
        app.update_settings(Settings::default()).unwrap();
        assert!(app.snapshot().settings.agents.codex.switched_by_app);
        assert!(
            settings::load(&app.settings_path)
                .unwrap()
                .agents
                .codex
                .switched_by_app
        );
        let before = app.snapshot().settings;
        let mut model_settings = before.agents.codex.clone();
        model_settings.switched_by_app = false;
        model_settings.threshold = 80;
        model_settings.base.effort = Some("high".into());
        let saved = app.update_model_settings(model_settings.clone()).unwrap();
        assert!(saved.agents.codex.switched_by_app);
        assert_eq!(saved.agents.codex.threshold, 80);
        assert_eq!(saved.agents.codex.base.effort.as_deref(), Some("high"));
        assert_eq!(saved.notifications, before.notifications);
        assert_eq!(saved.running_folder_order, before.running_folder_order);
        app.settings_path = dir.path().join("blocked.json");
        std::fs::create_dir(&app.settings_path).unwrap();
        let mut changed = Settings::default();
        changed.agents.codex.base.model = "another-model".into();
        assert!(app.update_settings(changed).is_err());
        model_settings.threshold = 70;
        assert!(app.update_model_settings(model_settings).is_err());
        assert_eq!(app.snapshot().settings.agents.codex.threshold, 80);
        assert_eq!(
            app.snapshot().settings.agents.codex.base.model,
            "gpt-6.1-sol"
        );
    }
    #[test]
    fn unseen_tracks_only_new_terminal_transitions() {
        let mut state = AppSnapshot {
            settings: Settings::default(),
            jobs: vec![],
            health: Health {
                plugin_version: None,
                version_supported: false,
                watched_roots: vec![],
                last_error: None,
            },
            unseen: BTreeSet::new(),
            window_error: false,
            pending_focus: None,
            watched_log: None,
            card_order: BTreeMap::new(),
            model_switch: Default::default(),
        };
        state.apply(
            &MonitorEvent::StateTransition {
                id: "a".into(),
                from: None,
                to: EffectiveState::Completed,
            },
            vec![],
        );
        assert!(state.unseen.contains("a"));
        state.mark_seen("a");
        assert!(state.unseen.is_empty());
        state.apply(
            &MonitorEvent::StateTransition {
                id: "b".into(),
                from: None,
                to: EffectiveState::Running,
            },
            vec![],
        );
        assert!(state.unseen.is_empty());
    }
    #[test]
    fn pending_focus_is_taken_once() {
        let mut state = AppSnapshot {
            settings: Settings::default(),
            jobs: vec![],
            health: Health {
                plugin_version: None,
                version_supported: false,
                watched_roots: vec![],
                last_error: None,
            },
            unseen: BTreeSet::new(),
            window_error: false,
            pending_focus: Some("job-a".into()),
            watched_log: None,
            card_order: BTreeMap::new(),
            model_switch: Default::default(),
        };
        assert_eq!(state.take_pending_focus().as_deref(), Some("job-a"));
        assert_eq!(state.take_pending_focus(), None);
        state.watched_log = Some("old".into());
        assert_eq!(
            state.replace_watched_log(None).unwrap().as_deref(),
            Some("old")
        );
        assert!(state.watched_log.is_none());
        assert!(state.replace_watched_log(Some("unknown".into())).is_err());
        state.jobs.push(JobSummary {
            id: "next".into(),
            workspace_root: None,
            workspace_name: None,
            kind: None,
            kind_label: None,
            title: None,
            summary: None,
            status: "running".into(),
            phase: None,
            effective_state: EffectiveState::Running,
            created_at: None,
            started_at: None,
            completed_at: None,
            updated_at: None,
            pid: None,
            thread_id: None,
            write: None,
            session_id: None,
            latest_message: None,
            log_modified_at: None,
            context: None,
            previous_job_id: None,
            archived: false,
            archived_note: None,
            prompt_head: None,
            model: None,
            effort: None,
        });
        assert_eq!(
            state.replace_watched_log(Some("next".into())).unwrap(),
            None
        );
        assert_eq!(state.watched_log.as_deref(), Some("next"));
        assert_eq!(
            state.replace_watched_log(None).unwrap().as_deref(),
            Some("next")
        );
    }
}
