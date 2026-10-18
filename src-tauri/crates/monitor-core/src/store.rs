use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};

use crate::{
    Clock, EffectiveState, Job, JobContext, JobSummary, MonitorEvent, ProcessProbe,
    history::{History, head},
};

pub(crate) struct Store {
    sources: BTreeMap<PathBuf, Vec<Job>>,
    summaries: BTreeMap<String, JobSummary>,
    paths: BTreeMap<String, PathBuf>,
    logs: BTreeMap<String, (Option<String>, Option<DateTime<Utc>>)>,
    contexts: BTreeMap<String, JobContext>,
    detail_seen: BTreeSet<String>,
    models: BTreeMap<String, crate::JobModel>,
    initialized: bool,
}

impl Store {
    pub(crate) fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
            summaries: BTreeMap::new(),
            paths: BTreeMap::new(),
            logs: BTreeMap::new(),
            contexts: BTreeMap::new(),
            detail_seen: BTreeSet::new(),
            models: BTreeMap::new(),
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
    pub(crate) fn set_context(&mut self, id: &str, context: JobContext) {
        self.contexts.insert(id.to_owned(), context);
    }
    pub(crate) fn context_jobs(&self) -> Vec<(Job, PathBuf, Option<JobContext>)> {
        self.sources
            .iter()
            .flat_map(|(path, jobs)| {
                jobs.iter().filter_map(|job| {
                    job.session_id.as_ref()?;
                    Some((
                        job.clone(),
                        path.parent()?.to_path_buf(),
                        self.contexts.get(&job.id).cloned().or_else(|| {
                            self.summaries.get(&job.id).and_then(|s| s.context.clone())
                        }),
                    ))
                })
            })
            .collect()
    }
    pub(crate) fn model_jobs(&self) -> Vec<Job> {
        self.sources.values().flatten().cloned().collect()
    }
    pub(crate) fn set_models(&mut self, models: BTreeMap<String, crate::JobModel>) {
        self.models = models;
    }
    pub(crate) fn detail_candidates(&self) -> Vec<(String, PathBuf)> {
        self.sources
            .iter()
            .flat_map(|(source, jobs)| {
                jobs.iter().filter_map(|job| {
                    if !safe_id(&job.id) {
                        return None;
                    }
                    let previous = self.summaries.get(&job.id);
                    let terminal =
                        matches!(job.status.as_str(), "completed" | "failed" | "cancelled");
                    if self.detail_seen.contains(&job.id)
                        && previous.is_some_and(|p| {
                            p.prompt_head.is_some() && (!terminal || p.status == job.status)
                        })
                    {
                        return None;
                    }
                    Some((
                        job.id.clone(),
                        source
                            .parent()?
                            .join("jobs")
                            .join(format!("{}.json", job.id)),
                    ))
                })
            })
            .collect()
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
        mut history: Option<&mut History>,
        details: &BTreeMap<String, Job>,
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
                        model: self.models.get(&job.id).and_then(|m| m.model.clone()),
                        effort: self.models.get(&job.id).and_then(|m| m.effort.clone()),
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
                        context: self.contexts.get(&job.id).cloned().or_else(|| {
                            history
                                .as_ref()
                                .and_then(|h| h.get(&job.id))
                                .and_then(|r| r.summary().context)
                        }),
                        previous_job_id: None,
                        archived: false,
                        archived_note: None,
                        prompt_head: details
                            .get(&job.id)
                            .and_then(|d| {
                                d.request
                                    .as_ref()
                                    .and_then(|r| r.prompt.as_deref())
                                    .or(d.summary.as_deref())
                            })
                            .map(|s| head(s, 200))
                            .or_else(|| previous.and_then(|s| s.prompt_head.clone()))
                            .or_else(|| {
                                job.request
                                    .as_ref()
                                    .and_then(|r| r.prompt.as_deref())
                                    .or(job.summary.as_deref())
                                    .map(|s| head(s, 200))
                            }),
                    },
                );
                paths.insert(
                    job.id.clone(),
                    source.parent().unwrap_or(source).to_path_buf(),
                );
            }
        }
        if let Some(history) = history.as_mut() {
            for (id, summary) in &next {
                history.update_live(summary, details.get(id));
            }
            for (id, old) in &self.summaries {
                if !old.archived && !next.contains_key(id) {
                    history.archive(id);
                }
            }
            let live: BTreeSet<_> = next.keys().cloned().collect();
            history.maybe_cleanup(clock.now(), &live);
            for archived in history.summaries() {
                next.entry(archived.id.clone()).or_insert(archived);
            }
        }
        let threads: Vec<_> = next
            .values()
            .filter_map(|s| Some((s.id.clone(), s.thread_id.clone()?, s.created_at?)))
            .collect();
        for summary in next.values_mut() {
            summary.previous_job_id =
                summary
                    .thread_id
                    .as_ref()
                    .zip(summary.created_at)
                    .and_then(|(thread, at)| {
                        threads
                            .iter()
                            .filter(|(id, candidate, time)| {
                                id != &summary.id && candidate == thread && *time < at
                            })
                            .max_by_key(|(_, _, time)| time)
                            .map(|(id, _, _)| id.clone())
                    });
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
                let from = self
                    .summaries
                    .get(id)
                    .map(|old| old.effective_state.clone());
                if from.as_ref() != Some(&value.effective_state)
                    && (!value.archived || from.is_some())
                {
                    events.push(MonitorEvent::StateTransition {
                        id: id.clone(),
                        from,
                        to: value.effective_state.clone(),
                    });
                }
            }
        }
        self.initialized = true;
        self.summaries = next;
        self.paths = paths;
        self.detail_seen.extend(details.keys().cloned());
        self.detail_seen.retain(|id| self.paths.contains_key(id));
        self.logs
            .retain(|id, _| self.summaries.get(id).is_some_and(|s| !s.archived));
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
            let reference = if job.status == "running" {
                job.started_at
            } else {
                job.updated_at
            };
            if info
                .created_at
                .zip(reference)
                .is_some_and(|(process, baseline)| {
                    process.signed_duration_since(baseline) >= chrono::Duration::seconds(1)
                })
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
                &Job {
                    started_at: Some(now),
                    ..job("running")
                },
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
                .reconcile(
                    &alive,
                    &SystemClock,
                    Duration::from_secs(600),
                    None,
                    &BTreeMap::new()
                )
                .iter()
                .all(|e| !matches!(e, MonitorEvent::StateTransition { .. }))
        );
        store.replace_source(path, vec![job("running")]);
        assert!(
            store
                .reconcile(
                    &alive,
                    &SystemClock,
                    Duration::from_secs(600),
                    None,
                    &BTreeMap::new()
                )
                .iter()
                .any(|e| matches!(e, MonitorEvent::StateTransition { .. }))
        );
    }

    #[test]
    fn process_time_uses_status_timestamp_with_one_second_tolerance() {
        let baseline = Utc::now();
        let created = baseline - chrono::Duration::seconds(5);
        let process = |at| {
            Probe(Some(ProcessInfo {
                created_at: Some(at),
                executable_name: Some("node.exe".into()),
            }))
        };
        for status in ["queued", "running"] {
            let mut current = job(status);
            current.created_at = Some(created);
            current.updated_at = Some(baseline);
            current.started_at = Some(baseline);
            for at in [
                created + chrono::Duration::seconds(1),
                baseline,
                baseline + chrono::Duration::milliseconds(999),
            ] {
                assert_eq!(
                    effective_state(
                        &current,
                        None,
                        &process(at),
                        &SystemClock,
                        Duration::from_secs(600)
                    ),
                    if status == "queued" {
                        EffectiveState::Queued
                    } else {
                        EffectiveState::Running
                    }
                );
            }
            assert_eq!(
                effective_state(
                    &current,
                    None,
                    &process(baseline + chrono::Duration::seconds(1)),
                    &SystemClock,
                    Duration::from_secs(600)
                ),
                EffectiveState::Orphaned
            );
            if status == "queued" {
                current.updated_at = None;
            } else {
                current.started_at = None;
            }
            assert_eq!(
                effective_state(
                    &current,
                    None,
                    &process(baseline + chrono::Duration::hours(1)),
                    &SystemClock,
                    Duration::from_secs(600)
                ),
                if status == "queued" {
                    EffectiveState::Queued
                } else {
                    EffectiveState::Running
                }
            );
        }
    }
}
