use crate::settings::{self, Settings};
use monitor_core::{EffectiveState, Health, JobSummary, Monitor, MonitorEvent};
use std::{collections::BTreeSet, io, path::PathBuf, sync::Mutex};

pub struct AppState {
    pub monitor: Monitor,
    pub settings_path: PathBuf,
    pub inner: Mutex<AppSnapshot>,
}

impl AppState {
    pub fn update_settings(&self, settings: Settings) -> io::Result<()> {
        let monitor_changed = {
            let mut snapshot = self.inner.lock().expect("app state");
            let changed = snapshot.settings.quiet_minutes != settings.quiet_minutes
                || snapshot.settings.extra_state_roots != settings.extra_state_roots;
            snapshot.settings = settings.clone();
            changed
        };
        if monitor_changed {
            self.monitor.update_config(settings.monitor_config());
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
}

impl AppSnapshot {
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
}
