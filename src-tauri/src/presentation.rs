use crate::settings::NotificationSettings;
use monitor_core::{EffectiveState as E, JobSummary};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Attention,
    Running(u8),
    RunningPlus,
    Done,
    Idle,
}

pub fn is_active(state: &E) -> bool {
    matches!(state, E::Running | E::Queued | E::Quiet)
}

pub fn icon(jobs: &[JobSummary], unseen: &BTreeSet<String>) -> Icon {
    if jobs
        .iter()
        .any(|j| unseen.contains(&j.id) && matches!(j.effective_state, E::Failed | E::Orphaned))
    {
        return Icon::Attention;
    }
    let count = jobs
        .iter()
        .filter(|j| is_active(&j.effective_state))
        .count();
    if count >= 10 {
        return Icon::RunningPlus;
    }
    if count > 0 {
        return Icon::Running(count as u8);
    }
    if jobs
        .iter()
        .any(|j| unseen.contains(&j.id) && j.effective_state == E::Completed)
    {
        return Icon::Done;
    }
    Icon::Idle
}

pub fn phase_name(job: &JobSummary) -> &str {
    match job.effective_state {
        E::Quiet => "応答なし？",
        E::Queued => "待機中",
        E::Orphaned => "停止した可能性",
        E::Completed => "完了",
        E::Failed => "失敗",
        E::Cancelled if job.archived_note.is_some() => "中断された可能性",
        E::Cancelled => "キャンセル",
        E::Unknown => &job.status,
        E::Running => match job.phase.as_deref() {
            Some("queued") => "待機中",
            Some("starting") => "開始中",
            Some("investigating") => "調査中",
            Some("running") => "コマンド実行中",
            Some("verifying") => "検証中",
            Some("editing") => "編集中",
            Some("reviewing") => "レビュー中",
            Some("finalizing") => "まとめ中",
            Some("done") => "完了",
            Some("failed") => "失敗",
            Some("cancelled") => "キャンセル",
            Some(other) => other,
            None => "実行中",
        },
    }
}

pub fn elapsed(seconds: u64) -> String {
    if seconds < 60 {
        "1分未満".into()
    } else if seconds < 3600 {
        format!("{}分", seconds / 60)
    } else {
        format!("{}時間{}分", seconds / 3600, seconds % 3600 / 60)
    }
}

fn elapsed_job(job: &JobSummary, now_ms: i64) -> String {
    let start = job
        .started_at
        .or(job.created_at)
        .map(|t| t.timestamp_millis())
        .unwrap_or(now_ms);
    elapsed((now_ms.saturating_sub(start) / 1000) as u64)
}

fn workspace(job: &JobSummary) -> &str {
    job.workspace_name
        .as_deref()
        .unwrap_or("不明なワークスペース")
}

fn request_description(job: &JobSummary) -> &str {
    job.context
        .as_ref()
        .and_then(|context| context.requester.as_ref())
        .and_then(|requester| requester.description.as_deref())
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            job.prompt_head
                .as_deref()
                .filter(|text| !text.trim().is_empty())
        })
        .or_else(|| job.title.as_deref().filter(|text| !text.trim().is_empty()))
        .unwrap_or("依頼内容なし")
}

pub fn active_jobs(jobs: &[JobSummary]) -> Vec<&JobSummary> {
    let mut active: Vec<_> = jobs
        .iter()
        .filter(|j| is_active(&j.effective_state))
        .collect();
    active.sort_by(|a, b| {
        b.started_at
            .or(b.created_at)
            .cmp(&a.started_at.or(a.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    active
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

#[cfg(test)]
pub fn tooltip(jobs: &[JobSummary], now_ms: i64) -> String {
    tooltip_inner(jobs, now_ms, "", 127)
}

pub fn tooltip_with_usage(jobs: &[JobSummary], now_ms: i64, usage: &str) -> String {
    let mut line = String::new();
    for ch in usage.chars() {
        if utf16_len(&line) + ch.len_utf16() > 85 {
            line.push('…');
            break;
        }
        line.push(ch);
    }
    tooltip_inner(jobs, now_ms, &format!("\n{line}"), 127)
}

fn tooltip_inner(jobs: &[JobSummary], now_ms: i64, suffix: &str, limit: usize) -> String {
    let active = active_jobs(jobs);
    if active.is_empty() {
        return format!("Codex: 待機中{suffix}");
    }
    let prefix = format!("Codex: {}件実行中", active.len());
    let parts: Vec<_> = active
        .iter()
        .map(|j| {
            format!(
                "{}: {} {}",
                workspace(j),
                phase_name(j),
                elapsed_job(j, now_ms)
            )
        })
        .collect();
    for shown in (0..=parts.len()).rev() {
        let tail = if shown < parts.len() {
            format!("ほか {}件", parts.len() - shown)
        } else {
            String::new()
        };
        let mut labels = parts[..shown].to_vec();
        if !tail.is_empty() {
            labels.push(tail);
        }
        let text = format!("{}（{}）{}", prefix, labels.join(" / "), suffix);
        if utf16_len(&text) <= limit {
            return text;
        }
    }
    format!("{prefix}{suffix}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuRow {
    Job { id: String, label: String },
    Disabled(String),
    Separator,
    Warning(String),
    Open,
    Notifications(bool),
    Autostart(bool),
    Exit,
}

pub fn menu_rows(
    jobs: &[JobSummary],
    warning: Option<&str>,
    now_ms: i64,
    notifications: bool,
    autostart: bool,
) -> Vec<MenuRow> {
    let active = active_jobs(jobs);
    let mut rows: Vec<_> = active
        .iter()
        .take(5)
        .map(|j| MenuRow::Job {
            id: j.id.clone(),
            label: format!(
                "{}: {} — {} — {}",
                workspace(j),
                clip_with_ellipsis(request_description(j), 30),
                phase_name(j),
                elapsed_job(j, now_ms)
            ),
        })
        .collect();
    if active.is_empty() {
        rows.push(MenuRow::Disabled("実行中のジョブはありません".into()));
    }
    if active.len() > 5 {
        rows.push(MenuRow::Disabled(format!("ほか {}件", active.len() - 5)));
    }
    rows.push(MenuRow::Separator);
    if let Some(warning) = warning {
        rows.push(MenuRow::Warning(format!("⚠ {warning}")));
    }
    rows.extend([
        MenuRow::Open,
        MenuRow::Notifications(notifications),
        MenuRow::Autostart(autostart),
        MenuRow::Separator,
        MenuRow::Exit,
    ]);
    rows
}

fn clip_with_ellipsis(value: &str, limit: usize) -> String {
    let flattened = value
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect::<String>();
    let mut chars = flattened.trim().chars();
    let text = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{text}…")
    } else {
        text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: &'static str,
    pub body: String,
}

pub fn notification(
    from: Option<&E>,
    to: &E,
    job: &JobSummary,
    detail: Option<&str>,
    settings: &NotificationSettings,
    quiet_minutes: u64,
    now_ms: i64,
) -> Option<Notice> {
    if !settings.enabled || job.archived {
        return None;
    }
    let (enabled, title, suffix) = match to {
        E::Running if from.is_none_or(|s| *s == E::Queued) => {
            (settings.started, "Codexが開始", None)
        }
        E::Completed => (settings.completed, "Codexが完了", job.summary.as_deref()),
        E::Failed => (settings.failed, "Codexが失敗", detail),
        E::Orphaned => (settings.orphaned, "Codexのジョブが停止した可能性", None),
        E::Quiet => (settings.quiet, "Codexが応答していません", None),
        _ => return None,
    };
    if !enabled {
        return None;
    }
    let name = workspace(job);
    let body = if *to == E::Quiet {
        let minutes = job
            .log_modified_at
            .map(|time| (now_ms.saturating_sub(time.timestamp_millis()) / 60_000).max(0) as u64)
            .unwrap_or(quiet_minutes);
        format!("{name}（{minutes}分）")
    } else {
        let first = format!("{name} — {}", clip_text(request_description(job), 80));
        if let Some(value) = suffix.filter(|value| !value.trim().is_empty()) {
            format!("{first}\n{}", clip_text(value, 80))
        } else {
            first
        }
    };
    Some(Notice { title, body })
}

pub fn clip_text(text: &str, limit: usize) -> String {
    text.chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{JobContext, Requester, RequesterKind};
    fn job(id: &str, state: E) -> JobSummary {
        JobSummary {
            id: id.into(),
            workspace_root: None,
            workspace_name: Some(id.into()),
            kind: None,
            kind_label: None,
            title: None,
            summary: Some("result".into()),
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
    #[test]
    fn icon_priority_and_counts() {
        let mut jobs = vec![job("done", E::Completed)];
        let mut unseen = BTreeSet::from(["done".into()]);
        assert_eq!(icon(&jobs, &unseen), Icon::Done);
        jobs.push(job("run", E::Running));
        assert_eq!(icon(&jobs, &unseen), Icon::Running(1));
        jobs.push(job("bad", E::Failed));
        unseen.insert("bad".into());
        assert_eq!(icon(&jobs, &unseen), Icon::Attention);
        unseen.clear();
        assert_eq!(icon(&jobs, &unseen), Icon::Running(1));
        jobs.clear();
        assert_eq!(icon(&jobs, &unseen), Icon::Idle);
        for n in 1..=10 {
            jobs.push(job(&n.to_string(), E::Queued));
        }
        assert_eq!(icon(&jobs, &unseen), Icon::RunningPlus);
    }
    #[test]
    fn tooltip_truncates_on_utf16_boundary() {
        let jobs = (0..8)
            .map(|n| job(&format!("長いワークスペース{n}🚀"), E::Running))
            .collect::<Vec<_>>();
        let text = tooltip(&jobs, 0);
        assert!(utf16_len(&text) <= 127);
        assert!(text.contains("ほか"));
        let usage = "5時間 92%・週 18% / gpt-6-luna";
        let text = tooltip_with_usage(&jobs, 0, usage);
        assert!(utf16_len(&text) <= 127);
        assert!(text.ends_with(usage));
        assert!(text.contains("ほか"));
        let long = tooltip_with_usage(&jobs, 0, &"🚀".repeat(200));
        assert!(utf16_len(&long) <= 127);
        assert_eq!(tooltip(&[], 0), "Codex: 待機中");
    }
    #[test]
    fn menu_limit_and_order() {
        let jobs = (0..7)
            .map(|n| job(&n.to_string(), E::Running))
            .collect::<Vec<_>>();
        let rows = menu_rows(&jobs, Some("error"), 0, true, false);
        assert_eq!(
            rows.iter()
                .filter(|r| matches!(r, MenuRow::Job { .. }))
                .count(),
            5
        );
        assert!(rows.contains(&MenuRow::Disabled("ほか 2件".into())));
        assert_eq!(rows[6], MenuRow::Separator);
        assert!(rows.contains(&MenuRow::Warning("⚠ error".into())));
    }
    #[test]
    fn same_workspace_jobs_have_distinct_request_in_menu() {
        let mut first = job("one", E::Running);
        first.workspace_name = Some("shared".into());
        first.prompt_head = Some("fallback request".into());
        first.context = Some(JobContext {
            session_id: "same".into(),
            session_title: None,
            requester: Some(Requester {
                kind: RequesterKind::Subagent,
                description: Some("first task\nwith details".into()),
                agent_type: None,
                depth: 1,
                parent_description: None,
            }),
        });
        let mut second = job("two", E::Running);
        second.workspace_name = Some("shared".into());
        second.prompt_head = Some("second task".into());
        let rows = menu_rows(&[first, second], None, 0, true, false);
        assert!(
            matches!(&rows[0], MenuRow::Job { label, .. } if label.contains("first task with details"))
        );
        assert!(matches!(&rows[1], MenuRow::Job { label, .. } if label.contains("second task")));
        assert_eq!(
            clip_with_ellipsis(&"あ".repeat(31), 30),
            format!("{}…", "あ".repeat(30))
        );
    }
    #[test]
    fn notification_matrix_and_controls() {
        let job = job("repo", E::Completed);
        let mut s = NotificationSettings::default();
        assert!(notification(None, &E::Running, &job, None, &s, 10, 0).is_none());
        s.started = true;
        assert!(notification(None, &E::Running, &job, Some("start"), &s, 10, 0).is_some());
        assert!(notification(Some(&E::Queued), &E::Running, &job, None, &s, 10, 0).is_some());
        assert!(notification(Some(&E::Quiet), &E::Running, &job, None, &s, 10, 0).is_none());
        for state in [E::Completed, E::Failed, E::Orphaned] {
            assert!(notification(None, &state, &job, Some("error"), &s, 10, 0).is_some());
        }
        assert!(notification(None, &E::Quiet, &job, None, &s, 10, 0).is_none());
        s.quiet = true;
        assert_eq!(
            notification(None, &E::Quiet, &job, None, &s, 10, 0)
                .unwrap()
                .body,
            "repo（10分）"
        );
        s.enabled = false;
        assert!(notification(None, &E::Completed, &job, None, &s, 10, 0).is_none());
        let mut archived = job.clone();
        archived.archived = true;
        s.enabled = true;
        assert!(
            notification(Some(&E::Running), &E::Cancelled, &archived, None, &s, 10, 0).is_none()
        );
        assert!(notification(None, &E::Completed, &archived, None, &s, 10, 0).is_none());
    }
    #[test]
    fn notification_uses_request_and_result_on_separate_lines() {
        let mut target = job("repo", E::Completed);
        target.workspace_name = Some("shared".into());
        target.prompt_head = Some("fix docs\nquickly".into());
        let settings = NotificationSettings::default();
        assert_eq!(
            notification(None, &E::Completed, &target, None, &settings, 10, 0)
                .unwrap()
                .body,
            "shared — fix docs quickly\nresult"
        );
        assert_eq!(
            notification(
                None,
                &E::Failed,
                &target,
                Some("bad\nerror"),
                &settings,
                10,
                0
            )
            .unwrap()
            .body,
            "shared — fix docs quickly\nbad error"
        );
        assert_eq!(
            notification(None, &E::Orphaned, &target, None, &settings, 10, 0)
                .unwrap()
                .body,
            "shared — fix docs quickly"
        );
    }
    #[test]
    fn text_and_elapsed_boundaries() {
        assert_eq!(clip_text("a\nb\rc", 80), "a b c");
        assert_eq!(clip_text(&"あ".repeat(90), 80).chars().count(), 80);
        assert_eq!(elapsed(59), "1分未満");
        assert_eq!(elapsed(60), "1分");
        assert_eq!(elapsed(3600), "1時間0分");
        assert_eq!(phase_name(&job("x", E::Quiet)), "応答なし？");
    }
}
