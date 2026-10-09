use crate::{
    settings::Settings,
    state::{AppState, UiSnapshot},
    tray,
};
use monitor_core::{JobDetail, JobSummary, LogChunk};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> UiSnapshot {
    state.snapshot()
}

#[tauri::command]
pub fn get_job(state: State<'_, AppState>, id: String) -> Option<JobDetail> {
    state.monitor.job_detail(&id)
}

#[tauri::command]
pub fn read_log(
    state: State<'_, AppState>,
    id: String,
    from: Option<u64>,
) -> Result<Option<LogChunk>, String> {
    state
        .monitor
        .read_log(&id, from)
        .map_err(|e| format!("ログを読めませんでした: {e}"))
}

#[tauri::command]
pub fn watch_log(state: State<'_, AppState>, id: Option<String>) -> Result<(), String> {
    state.watch_log(id)
}

#[tauri::command]
pub fn mark_seen(app: AppHandle, id: String) {
    let state = app.state::<AppState>();
    let unseen = {
        let mut snap = state.inner.lock().expect("app state");
        snap.mark_seen(&id);
        snap.unseen.iter().cloned().collect::<Vec<_>>()
    };
    tray::refresh(&app);
    let _ = app.emit_to("main", "unseen-changed", unseen);
}

fn known_path(
    kind: &str,
    id: &str,
    jobs: &[JobSummary],
    roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let job = jobs
        .iter()
        .find(|job| job.id == id)
        .ok_or("ジョブが見つかりません")?;
    match kind {
        "workspace" => job
            .workspace_root
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_dir())
            .ok_or_else(|| "ワークスペースのフォルダが見つかりません".into()),
        "log" => {
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err("ジョブIDが不正です".into());
            }
            for root in roots {
                let Ok(entries) = std::fs::read_dir(root) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let directory = entry.path();
                    if !directory.is_dir() {
                        continue;
                    }
                    let json = directory.join("jobs").join(format!("{id}.json"));
                    if let Ok(parsed) = monitor_core::read_job(&json)
                        && parsed.id == id
                        && parsed.workspace_root == job.workspace_root
                    {
                        return Ok(directory.join("jobs").join(format!("{id}.log")));
                    }
                }
            }
            Err("ログファイルが見つかりません".into())
        }
        _ => Err("開く対象が不正です".into()),
    }
}

#[tauri::command]
pub fn open_path(app: AppHandle, kind: String, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let snap = state.inner.lock().expect("app state").clone();
    let mut roots = monitor_core::default_state_roots();
    roots.extend(snap.settings.extra_state_roots);
    let path = known_path(&kind, &id, &snap.jobs, &roots)?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| format!("開けませんでした: {e}"))
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    settings.validate()?;
    let manager = app.autolaunch();
    let enabled = manager
        .is_enabled()
        .map_err(|e| format!("自動起動を確認できませんでした: {e}"))?;
    if settings.autostart != enabled {
        (if settings.autostart {
            manager.enable()
        } else {
            manager.disable()
        })
        .map_err(|e| format!("自動起動を変更できませんでした: {e}"))?;
    }
    app.state::<AppState>()
        .update_settings(settings.clone())
        .map_err(|e| format!("設定を保存できませんでした: {e}"))?;
    tray::refresh(&app);
    let _ = app.emit_to("main", "settings-changed", &settings);
    Ok(settings)
}

#[tauri::command]
pub fn take_pending_focus(state: State<'_, AppState>) -> Option<String> {
    state.inner.lock().expect("app state").take_pending_focus()
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::EffectiveState;
    fn job_with_workspace(root: &std::path::Path) -> JobSummary {
        JobSummary {
            id: "known".into(),
            workspace_root: Some(root.to_string_lossy().into_owned()),
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
        }
    }
    #[test]
    fn unknown_id_and_invalid_kind_cannot_open() {
        assert!(
            known_path(
                "log",
                "missing",
                &[],
                &[std::path::Path::new("unused").to_path_buf()]
            )
            .is_err()
        );
        assert!(known_path("arbitrary", "missing", &[], &[]).is_err());
        assert!(known_path("workspace", "missing", &[], &[]).is_err());
    }
    #[test]
    fn workspace_path_must_be_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("program.exe");
        std::fs::write(&file, b"fixture").unwrap();
        assert!(known_path("workspace", "known", &[job_with_workspace(&file)], &[]).is_err());
        let missing = dir.path().join("missing");
        assert!(known_path("workspace", "known", &[job_with_workspace(&missing)], &[]).is_err());
        assert_eq!(
            known_path("workspace", "known", &[job_with_workspace(dir.path())], &[]).unwrap(),
            dir.path()
        );
    }
}
