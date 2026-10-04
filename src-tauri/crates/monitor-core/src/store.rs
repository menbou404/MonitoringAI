use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};

use crate::{Clock, EffectiveState, Job, JobSummary, MonitorEvent, ProcessProbe};

pub(crate) struct Store {
    sources: BTreeMap<PathBuf, Vec<Job>>,
    summaries: BTreeMap<String, JobSummary>,
    paths: BTreeMap<String, PathBuf>,
    logs: BTreeMap<String, (Option<String>, Option<DateTime<Utc>>)>,
    initialized: bool,
}

impl Store {
    pub(crate) fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
            summaries: BTreeMap::new(),
            paths: BTreeMap::new(),
            logs: BTreeMap::new(),
            initialized: false,
        }
    }

    pub(crate) fn replace_source(&mut self, path: PathBuf, jobs: Vec<Job>) {
        self.sources.insert(path, jobs);
    }

    pub(crate) fn retain_sources(&mut self, found: &BTreeSet<PathBuf>) {
        self.sources.retain(|path, _| found.contains(path));
    }

    pub(crate) fn snapshot(&self) -> Vec<JobSummary> {
        self.summaries.values().cloned().collect()
    }
    pub(crate) fn path_for(&self, id: &str) -> Option<&Path> {
        self.paths.get(id).map(PathBuf::as_path)
    }
    pub(crate) fn summary(&self, id: &str) -> Option<&JobSummary> {
        self.summaries.get(id)
    }
    pub(crate) fn log_candidates(&self) -> Vec<(String, String, PathBuf)> {
        self.sources
            .iter()
            .flat_map(|(source, jobs)| {
                jobs.iter().filter_map(move |job| {
                    if !safe_id(&job.id) {
                        return None;
                    }
                    Some((
                        job.id.clone(),
                        job.status.clone(),
                        source
                            .parent()?
                            .join("jobs")
                            .join(format!("{}.log", job.id)),
                    ))
                })
            })
            .collect()
    }

    pub(crate) fn set_log(
        &mut self,
        id: &str,
        latest: Option<String>,
        modified: Option<DateTime<Utc>>,
    ) {
        self.logs.insert(id.to_owned(), (latest, modified));
    }

    pub(crate) fn reconcile(
        &mut self,
        probe: &dyn ProcessProbe,
        clock: &dyn Clock,
        quiet_after: Duration,
    ) -> Vec<MonitorEvent> {
        let mut next = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for (source, jobs) in &self.sources {
            for job in jobs {
                let previous = self.summaries.get(&job.id);
                let log = self.logs.get(&job.id);
                let log_modified = log.and_then(|(_, at)| *at);
                let state = effective_state(job, log_modified, probe, clock, quiet_after);
                let name = job
                    .workspace_root
                    .as_deref()
                    .and_then(|root| {
                        root.trim_end_matches(['/', '\\'])
                            .rsplit(['/', '\\'])
                            .next()
                    })
                    .map(str::to_owned);
                next.insert(
                    job.id.clone(),
                    JobSummary {
                        id: job.id.clone(),
                        workspace_root: job.workspace_root.clone(),
                        workspace_name: name,
                        kind: job.kind.clone(),
                        kind_label: job.kind_label.clone(),
                        title: job.title.clone(),
                        summary: job.summary.clone(),
                        status: job.status.clone(),
                        phase: job.phase.clone(),
                        effective_state: state,
                        created_at: job.created_at,
                        started_at: job.started_at,
                        completed_at: job.completed_at,
                        updated_at: job.updated_at,
                        pid: job.pid,
                        thread_id: job.thread_id.clone(),
                        write: job.write,
                        session_id: job.session_id.clone(),
                        latest_message: match log {
                            Some((text, _)) => text.clone(),
                            None => previous.and_then(|s| s.latest_message.clone()),
                        },
                        log_modified_at: log_modified,
                    },
                );
                paths.insert(
                    job.id.clone(),
                    source.parent().unwrap_or(source).to_path_buf(),
                );
            }
        }
        let mut events = Vec::new();
        let changed: Vec<_> = next
            .iter()
            .filter(|(id, value)| self.summaries.get(*id) != Some(*value))
            .map(|(_, value)| value.clone())
            .collect();
        let removed: Vec<_> = self
            .summaries
            .keys()
            .filter(|id| !next.contains_key(*id))
            .cloned()
            .collect();
        if !changed.is_empty() || !removed.is_empty() {
            events.push(MonitorEvent::JobsChanged { changed, removed });
        }
        if self.initialized {
            for (id, value) in &next {
                if let Some(old) = self.summaries.get(id) {
                    if old.effective_state != value.effective_state {
                        events.push(MonitorEvent::StateTransition {
                            id: id.clone(),
                            from: old.effective_state.clone(),
                            to: value.effective_state.clone(),
                        });
                    }
                }
            }
        }
        self.initialized = true;
        self.summaries = next;
        self.paths = paths;
        self.logs.retain(|id, _| self.summaries.contains_key(id));
        events
    }
}

pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(crate) fn effective_state(
    job: &Job,
    log_modified: Option<DateTime<Utc>>,
    probe: &dyn ProcessProbe,
    clock: &dyn Clock,
    quiet_after: Duration,
) -> EffectiveState {
    match job.status.as_str() {
        "completed" => EffectiveState::Completed,
        "failed" => EffectiveState::Failed,
        "cancelled" => EffectiveState::Cancelled,
        "queued" | "running" => {
            let Some(info) = job.pid.and_then(|pid| probe.probe(pid)) else {
                return EffectiveState::Orphaned;
            };
            if info
                .created_at
                .zip(job.created_at)
                .is_some_and(|(process, job)| process > job)
                || info
                    .executable_name
                    .as_deref()
                    .is_some_and(|name| !name.eq_ignore_ascii_case("node.exe"))
            {
                return EffectiveState::Orphaned;
            }
            if job.status == "queued" {
                return EffectiveState::Queued;
            }
            if log_modified.is_some_and(|at| {
                clock
                    .now()
                    .signed_duration_since(at)
                    .to_std()
                    .is_ok_and(|age| age >= quiet_after)
            }) {
                EffectiveState::Quiet
            } else {
                EffectiveState::Running
            }
        }
        _ => EffectiveState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessInfo, SystemClock};

    struct Probe(Option<ProcessInfo>);
    impl ProcessProbe for Probe {
        fn probe(&self, _: u32) -> Option<ProcessInfo> {
            self.0.clone()
        }
    }
    fn job(status: &str) -> Job {
        Job {
            id: "fictional".into(),
            status: status.into(),
            phase: None,
            kind: None,
            kind_label: None,
            title: None,
            summary: None,
            workspace_root: Some("C:/fiction/work".into()),
            write: None,
            session_id: None,
            pid: Some(1),
            thread_id: None,
            turn_id: None,
            log_file: None,
            created_at: Some(Utc::now()),
            updated_at: None,
            started_at: None,
            completed_at: None,
            request: None,
            rendered: None,
            error_message: None,
        }
    }
    #[test]
    fn liveness_and_transitions() {
        let now = Utc::now();
        let alive = Probe(Some(ProcessInfo {
            created_at: Some(now - chrono::Duration::minutes(1)),
            executable_name: Some("node.exe".into()),
        }));
        let reused = Probe(Some(ProcessInfo {
            created_at: Some(now + chrono::Duration::minutes(1)),
            executable_name: Some("node.exe".into()),
        }));
        assert_eq!(
            effective_state(
                &job("running"),
                None,
                &Probe(None),
                &SystemClock,
                Duration::from_secs(1)
            ),
            EffectiveState::Orphaned
        );
        assert_eq!(
            effective_state(
                &job("running"),
                None,
                &reused,
                &SystemClock,
                Duration::from_secs(1)
            ),
            EffectiveState::Orphaned
        );
        assert_eq!(
            effective_state(
                &job("queued"),
                None,
                &alive,
                &SystemClock,
                Duration::from_secs(1)
            ),
            EffectiveState::Queued
        );
        assert_eq!(
            effective_state(
                &job("running"),
                Some(now - chrono::Duration::minutes(11)),
                &alive,
                &SystemClock,
                Duration::from_secs(600)
            ),
            EffectiveState::Quiet
        );
        let mut store = Store::new();
        let path = PathBuf::from("fiction/state.json");
        store.replace_source(path.clone(), vec![job("queued")]);
        assert!(
            store
                .reconcile(&alive, &SystemClock, Duration::from_secs(600))
                .iter()
                .all(|e| !matches!(e, MonitorEvent::StateTransition { .. }))
        );
        store.replace_source(path, vec![job("running")]);
        assert!(
            store
                .reconcile(&alive, &SystemClock, Duration::from_secs(600))
                .iter()
                .any(|e| matches!(e, MonitorEvent::StateTransition { .. }))
        );
    }
}
