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

pub fn tooltip(jobs: &[JobSummary], now_ms: i64) -> String {
    let active = active_jobs(jobs);
    if active.is_empty() {
        return "Codex: 待機中".into();
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
        let text = format!("{}（{}）", prefix, labels.join(" / "));
        if utf16_len(&text) <= 127 {
            return text;
        }
    }
    prefix
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
                "{}{} — {} — {}",
                workspace(j),
                j.summary
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!("「{}」", clip_menu_summary(s)))
                    .unwrap_or_default(),
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

fn clip_menu_summary(value: &str) -> String {
    let flattened = value
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect::<String>();
    let mut chars = flattened.trim().chars();
    let text = chars.by_ref().take(20).collect::<String>();
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
    if !settings.enabled {
        return None;
    }
    let (enabled, title, suffix) = match to {
        E::Running if from.is_none_or(|s| *s == E::Queued) => (
            settings.started,
            "Codexが開始",
            detail.or(job.summary.as_deref()),
        ),
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
    } else if let Some(value) = suffix {
        format!("{name} — {}", clip_text(value, 80))
    } else {
        name.into()
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
        first.summary = Some("first task\nwith details".into());
        let mut second = job("two", E::Running);
        second.workspace_name = Some("shared".into());
        second.summary = Some("second task".into());
        let rows = menu_rows(&[first, second], None, 0, true, false);
        assert!(
            matches!(&rows[0], MenuRow::Job { label, .. } if label.contains("first task with deta…"))
        );
        assert!(matches!(&rows[1], MenuRow::Job { label, .. } if label.contains("second task")));
        assert_eq!(
            clip_menu_summary(&"あ".repeat(21)),
            format!("{}…", "あ".repeat(20))
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
