use crate::{presentation, settings::Settings};
use monitor_core::{EffectiveState, JobSummary};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub fn model_switch(app: &AppHandle, message: &str) {
    if let Err(error) = app
        .notification()
        .builder()
        .title("Codexのモデル切り替え")
        .body(message)
        .show()
    {
        tracing::warn!(%error, "notification failed");
    }
}

pub fn transition(
    app: &AppHandle,
    from: Option<&EffectiveState>,
    to: &EffectiveState,
    job: &JobSummary,
    detail: Option<&str>,
    settings: &Settings,
) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64;
    if let Some(notice) = presentation::notification(
        from,
        to,
        job,
        detail,
        &settings.notifications,
        settings.quiet_minutes,
        now_ms,
    ) && let Err(error) = app
        .notification()
        .builder()
        .title(notice.title)
        .body(notice.body)
        .show()
    {
        tracing::warn!(%error,"notification failed");
    }
}
