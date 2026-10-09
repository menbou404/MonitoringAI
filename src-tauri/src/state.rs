use crate::settings::{self, Settings};
use monitor_core::{EffectiveState, Health, JobSummary, Monitor, MonitorEvent};
use serde::Serialize;
use std::{collections::BTreeSet, io, path::PathBuf, sync::Mutex};

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
}

impl AppState {
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

    pub fn update_settings(&self, settings: Settings) -> io::Result<()> {
        let monitor_changed = {
            let mut snapshot = self.inner.lock().expect("app state");
            let changed = snapshot.settings.quiet_minutes != settings.quiet_minutes
                || snapshot.settings.extra_state_roots != settings.extra_state_roots;
            snapshot.settings = settings.clone();
            changed
        };
        if monitor_changed {
            self.monitor
                .update_config(settings.monitor_config_with_history(self.history_dir.clone()));
        }
        settings::save(&self.settings_path, &settings)
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
        }
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
