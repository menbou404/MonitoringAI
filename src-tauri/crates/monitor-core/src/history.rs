use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{EffectiveState, Job, JobContext, JobDetail, JobSummary, store::safe_id};

pub(crate) const ARCHIVED_NOTE: &str =
    "codex-plugin-cc の一覧から消えました。Claude の会話の終了などで中断された可能性があります。";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    version: u32,
    id: String,
    workspace_root: Option<String>,
    kind: Option<String>,
    kind_label: Option<String>,
    title: Option<String>,
    write: Option<bool>,
    prompt_head: Option<String>,
    summary: Option<String>,
    rendered: Option<String>,
    rendered_truncated: bool,
    error_message: Option<String>,
    status: String,
    effective_state: EffectiveState,
    created_at: Option<DateTime<Utc>>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    thread_id: Option<String>,
    session_id: Option<String>,
    context: Option<JobContext>,
    archived_note: Option<String>,
}

pub(crate) fn head(text: &str, len: usize) -> String {
    text.chars().take(len).collect()
}
fn clipped(text: Option<&str>) -> (Option<String>, bool) {
    let Some(text) = text else {
        return (None, false);
    };
    if text.len() <= 64 * 1024 {
        return (Some(text.to_owned()), false);
    }
    let mut end = 64 * 1024;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (Some(text[..end].to_owned()), true)
}

impl Record {
    fn from_live(summary: &JobSummary, detail: Option<&Job>) -> Self {
        let (rendered, truncated) = clipped(detail.and_then(|j| j.rendered.as_deref()));
        Self {
            version: 1,
            id: summary.id.clone(),
            workspace_root: summary.workspace_root.clone(),
            kind: summary.kind.clone(),
            kind_label: summary.kind_label.clone(),
            title: summary.title.clone(),
            write: summary.write,
            prompt_head: summary.prompt_head.clone(),
            summary: summary.summary.clone(),
            rendered,
            rendered_truncated: truncated,
            error_message: detail.and_then(|j| j.error_message.clone()),
            status: summary.status.clone(),
            effective_state: summary.effective_state.clone(),
            created_at: summary.created_at,
            started_at: summary.started_at,
            completed_at: summary.completed_at,
            thread_id: summary.thread_id.clone(),
            session_id: summary.session_id.clone(),
            context: summary.context.clone(),
            archived_note: None,
        }
    }

    pub(crate) fn summary(&self) -> JobSummary {
        let name = self
            .workspace_root
            .as_deref()
            .and_then(|root| {
                root.trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
            })
            .map(str::to_owned);
        JobSummary {
            id: self.id.clone(),
            workspace_root: self.workspace_root.clone(),
            workspace_name: name,
            kind: self.kind.clone(),
            kind_label: self.kind_label.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            status: self.status.clone(),
            phase: None,
            effective_state: self.effective_state.clone(),
            created_at: self.created_at,
            started_at: self.started_at,
            completed_at: self.completed_at,
            updated_at: None,
            pid: None,
            thread_id: self.thread_id.clone(),
            write: self.write,
            session_id: self.session_id.clone(),
            latest_message: None,
            log_modified_at: None,
            context: self.context.clone(),
            previous_job_id: None,
            archived: true,
            archived_note: self.archived_note.clone(),
            prompt_head: self.prompt_head.clone(),
        }
    }

    pub(crate) fn detail(&self) -> JobDetail {
        JobDetail {
            summary: self.summary(),
            prompt: self.prompt_head.clone(),
            turn_id: None,
            model: None,
            effort: None,
            rendered: self.rendered.clone(),
            error_message: self.error_message.clone(),
            log_file: None,
        }
    }
}

pub(crate) struct History {
    dir: PathBuf,
    records: BTreeMap<String, Record>,
    days: u32,
    last_cleanup: Option<DateTime<Utc>>,
    dirty: BTreeSet<String>,
    deleted: BTreeSet<String>,
}
impl History {
    pub(crate) fn open(dir: PathBuf, days: u32, _now: DateTime<Utc>) -> Self {
        let mut records = BTreeMap::new();
        let mut invalid = 0;
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                match fs::read(&path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
                {
                    Some(record)
                        if record.version == 1
                            && safe_id(&record.id)
                            && path.file_stem().is_some_and(|s| s == record.id.as_str()) =>
                    {
                        records.insert(record.id.clone(), record);
                    }
                    _ => invalid += 1,
                }
            }
        }
        if invalid > 0 {
            tracing::warn!(count = invalid, "history files ignored");
        }
        Self {
            dir,
            records,
            days: days.clamp(1, 90),
            last_cleanup: None,
            dirty: BTreeSet::new(),
            deleted: BTreeSet::new(),
        }
    }

    fn save(&self, id: &str) {
        let Some(record) = self.records.get(id) else {
            return;
        };
        if fs::create_dir_all(&self.dir).is_err() {
            tracing::warn!("history directory unavailable");
            return;
        }
        let path = self.dir.join(format!("{id}.json"));
        let temp = self.dir.join(format!("{id}.json.tmp"));
        if let Ok(bytes) = serde_json::to_vec(record) {
            if fs::write(&temp, bytes).is_ok() {
                if fs::rename(&temp, &path).is_err() {
                    tracing::warn!("history write failed");
                    let _ = fs::remove_file(&temp);
                }
            } else {
                tracing::warn!("history write failed");
            }
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Record> {
        self.records.get(id)
    }
    pub(crate) fn set_context(&mut self, id: &str, context: JobContext) -> bool {
        if let Some(record) = self.records.get_mut(id)
            && record.context.as_ref() != Some(&context)
        {
            record.context = Some(context);
            self.dirty.insert(id.to_owned());
            return true;
        }
        false
    }
    pub(crate) fn summaries(&self) -> impl Iterator<Item = JobSummary> + '_ {
        self.records.values().map(Record::summary)
    }

    pub(crate) fn update_live(&mut self, summary: &JobSummary, detail: Option<&Job>) {
        if !safe_id(&summary.id) {
            return;
        }
        let old = self.records.get(&summary.id);
        let terminal = matches!(
            summary.effective_state,
            EffectiveState::Completed | EffectiveState::Failed | EffectiveState::Cancelled
        );
        let state_changed =
            old.is_some_and(|record| record.effective_state != summary.effective_state);
        let context_changed = old.is_some_and(|record| record.context != summary.context);
        let prompt_changed = old.is_some_and(|record| record.prompt_head != summary.prompt_head);
        let reappeared = old.is_some_and(|record| record.archived_note.is_some());
        let create = old.is_none();
        if !create
            && !context_changed
            && !prompt_changed
            && !reappeared
            && !(terminal && state_changed)
        {
            return;
        }
        let mut record = Record::from_live(summary, detail);
        if let Some(old) = old {
            if record.prompt_head.is_none() {
                record.prompt_head = old.prompt_head.clone();
            }
            if record.rendered.is_none() {
                record.rendered = old.rendered.clone();
                record.rendered_truncated = old.rendered_truncated;
            }
            if record.context.is_none() {
                record.context = old.context.clone();
            }
        }
        self.records.insert(summary.id.clone(), record);
        self.dirty.insert(summary.id.clone());
    }

    pub(crate) fn archive(&mut self, id: &str) {
        if let Some(record) = self.records.get_mut(id)
            && matches!(
                record.effective_state,
                EffectiveState::Queued
                    | EffectiveState::Running
                    | EffectiveState::Quiet
                    | EffectiveState::Orphaned
            )
        {
            record.effective_state = EffectiveState::Cancelled;
            record.archived_note = Some(ARCHIVED_NOTE.into());
            self.dirty.insert(id.to_owned());
        }
    }

    pub(crate) fn cleanup(&mut self, now: DateTime<Utc>, live: &BTreeSet<String>) {
        let cutoff = now - Duration::days(i64::from(self.days));
        let expired: Vec<_> = self
            .records
            .iter()
            .filter(|(id, r)| {
                !live.contains(*id)
                    && r.completed_at
                        .or(r.created_at)
                        .is_some_and(|at| at < cutoff)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.records.remove(&id);
            self.dirty.remove(&id);
            self.deleted.insert(id);
        }
        self.last_cleanup = Some(now);
    }
    pub(crate) fn maybe_cleanup(&mut self, now: DateTime<Utc>, live: &BTreeSet<String>) {
        if self
            .last_cleanup
            .is_none_or(|last| now - last >= Duration::hours(1))
        {
            self.cleanup(now, live);
        }
    }
    pub(crate) fn flush(&mut self) {
        for id in std::mem::take(&mut self.deleted) {
            let _ = fs::remove_file(self.dir.join(format!("{id}.json")));
        }
        for id in std::mem::take(&mut self.dirty) {
            self.save(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Requester, RequesterKind};

    fn record(id: &str, at: DateTime<Utc>) -> Record {
        Record {
            version: 1,
            id: id.into(),
            workspace_root: Some("C:/fiction/work".into()),
            kind: Some("task".into()),
            kind_label: Some("rescue".into()),
            title: Some("Codex Task".into()),
            write: Some(false),
            prompt_head: Some("fictional request".into()),
            summary: Some("fictional summary".into()),
            rendered: None,
            rendered_truncated: false,
            error_message: None,
            status: "running".into(),
            effective_state: EffectiveState::Running,
            created_at: Some(at),
            started_at: None,
            completed_at: None,
            thread_id: Some("thread-fiction".into()),
            session_id: Some("session-fiction".into()),
            context: None,
            archived_note: None,
        }
    }

    #[test]
    fn creation_context_completion_restart_and_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let mut history = History::open(dir.path().to_path_buf(), 30, now);
        let summary = record("task-one", now).summary();
        history.update_live(&summary, None);
        history.flush();
        assert!(dir.path().join("task-one.json").is_file());
        let mut with_context = summary;
        with_context.context = Some(JobContext {
            session_id: "session-fiction".into(),
            session_title: Some("架空の会話".into()),
            requester: Some(Requester {
                kind: RequesterKind::Subagent,
                description: Some("架空の依頼".into()),
                agent_type: Some("codex:codex-rescue".into()),
                depth: 1,
                parent_description: None,
            }),
        });
        history.update_live(&with_context, None);
        let mut completed = with_context;
        completed.effective_state = EffectiveState::Completed;
        completed.status = "completed".into();
        completed.summary = Some("fictional result".into());
        completed.completed_at = Some(now);
        let detail = dir.path().join("detail.json");
        fs::write(&detail, serde_json::json!({"id":"task-one","status":"completed","rendered":"完成","errorMessage":null}).to_string()).unwrap();
        let parsed = crate::parser::read_job(&detail).unwrap();
        history.update_live(&completed, Some(&parsed));
        history.archive("task-one");
        history.flush();
        assert_eq!(
            history.get("task-one").unwrap().effective_state,
            EffectiveState::Completed
        );
        fs::write(dir.path().join("broken.json"), "{").unwrap();
        let reopened = History::open(dir.path().to_path_buf(), 30, now);
        let stored = reopened.get("task-one").unwrap();
        assert_eq!(
            stored.context.as_ref().unwrap().session_title.as_deref(),
            Some("架空の会話")
        );
        assert_eq!(stored.summary.as_deref(), Some("fictional result"));
        assert_eq!(stored.detail().rendered.as_deref(), Some("完成"));
        assert!(reopened.get("broken").is_none());
    }

    #[test]
    fn active_archive_cleanup_and_utf8_rendered_limit() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let mut history = History::open(dir.path().to_path_buf(), 30, now);
        let old = now - Duration::days(31);
        history
            .records
            .insert("task-active".into(), record("task-active", old));
        history
            .records
            .insert("task-expired".into(), record("task-expired", old));
        history.save("task-active");
        history.save("task-expired");
        history.cleanup(now, &BTreeSet::from(["task-active".into()]));
        history.flush();
        assert!(history.get("task-active").is_some());
        assert!(history.get("task-expired").is_none());
        assert!(!dir.path().join("task-expired.json").exists());
        history.archive("task-active");
        assert_eq!(
            history.get("task-active").unwrap().effective_state,
            EffectiveState::Cancelled
        );
        assert_eq!(
            history.get("task-active").unwrap().archived_note.as_deref(),
            Some(ARCHIVED_NOTE)
        );
        let long = "あ".repeat(30_000);
        let (cut, truncated) = clipped(Some(&long));
        assert!(truncated);
        assert!(cut.unwrap().len() <= 64 * 1024);
        assert_eq!(head(&long, 200).chars().count(), 200);
    }
}
