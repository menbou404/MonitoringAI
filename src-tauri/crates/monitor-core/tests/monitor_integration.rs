use std::{
    fs,
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use monitor_core::{
    Clock, EffectiveState, Monitor, MonitorConfig, MonitorEvent, ProcessInfo, ProcessProbe,
};
use serde_json::json;

struct Alive;
impl ProcessProbe for Alive {
    fn probe(&self, _: u32) -> Option<ProcessInfo> {
        Some(ProcessInfo {
            created_at: Some(Utc::now() - chrono::Duration::hours(1)),
            executable_name: Some("node.exe".into()),
        })
    }
}
struct Now;
impl Clock for Now {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "monitor did not converge");
        thread::sleep(Duration::from_millis(25));
    }
}

fn write_state(path: &Path, jobs: serde_json::Value) {
    fs::write(path, json!({"jobs": jobs}).to_string()).unwrap();
}

#[test]
fn snapshot_keeps_last_good_json_and_removes_pruned_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let work = root.join("fiction-123");
    fs::create_dir_all(work.join("jobs")).unwrap();
    let state = work.join("state.json");
    let created = (Utc::now() - chrono::Duration::minutes(2)).to_rfc3339();
    let queued = json!({"id":"task-fictional-1","status":"queued","phase":"queued","pid":42,
        "createdAt":created,"workspaceRoot":"C:/fiction/work","summary":"fictional request",
        "request":{"prompt":"fictional full request","model":"fictional-model"}});
    write_state(&state, json!([queued]));
    fs::write(
        work.join("jobs/task-fictional-1.json"),
        json!({"id":"task-fictional-1","status":"queued",
        "summary":"fictional request","request":{"prompt":"fictional full request"}})
        .to_string(),
    )
    .unwrap();
    let mut config = MonitorConfig {
        state_roots: vec![root.clone()],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("installed_plugins.json"),
        ..MonitorConfig::default()
    };
    config.rescan_interval = Duration::from_millis(100);
    config.liveness_interval = Duration::from_millis(100);
    fs::write(
        &config.installed_plugins_path,
        r#"{"plugins":{"codex@openai-codex":[{"version":"1.0.6"}]}}"#,
    )
    .unwrap();
    let (monitor, events) = Monitor::start_with(config, Arc::new(Alive), Arc::new(Now));
    wait_until(|| monitor.snapshot().len() == 1);
    assert_eq!(
        monitor.snapshot()[0].effective_state,
        EffectiveState::Queued
    );
    assert_eq!(
        monitor
            .job_detail("task-fictional-1")
            .unwrap()
            .prompt
            .as_deref(),
        Some("fictional full request")
    );
    fs::write(work.join("jobs/task-fictional-1.json"), "{").unwrap();
    assert_eq!(
        monitor
            .job_detail("task-fictional-1")
            .unwrap()
            .prompt
            .as_deref(),
        Some("fictional full request")
    );
    assert!(monitor.health().version_supported);
    fs::write(&state, "{").unwrap();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(monitor.snapshot().len(), 1);
    write_state(
        &state,
        json!([{"id":"task-fictional-1","status":"running","phase":"investigating",
        "pid":42,"createdAt":created,"workspaceRoot":"C:/fiction/work"}]),
    );
    wait_until(|| monitor.snapshot()[0].effective_state == EffectiveState::Running);
    let mut transitioned = false;
    while let Ok(event) = events.try_recv() {
        if matches!(
            event,
            MonitorEvent::StateTransition {
                to: EffectiveState::Running,
                ..
            }
        ) {
            transitioned = true;
        }
    }
    assert!(transitioned);
    let many: Vec<_> = (0..52)
        .map(|n| json!({"id":format!("task-fictional-{n}"),"status":"completed"}))
        .collect();
    write_state(&state, json!(many));
    wait_until(|| monitor.snapshot().len() == 52);
    write_state(&state, json!([]));
    wait_until(|| monitor.snapshot().is_empty());
    fs::remove_dir_all(&work).unwrap();
    let second = root.join("another-456");
    fs::create_dir_all(&second).unwrap();
    write_state(
        &second.join("state.json"),
        json!([{"id":"task-new","status":"failed","errorMessage":"fictional failure"}]),
    );
    wait_until(|| monitor.snapshot().len() == 1 && monitor.snapshot()[0].id == "task-new");
}

#[test]
fn subscribed_log_emits_append() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let work = root.join("fiction-123");
    fs::create_dir_all(work.join("jobs")).unwrap();
    write_state(
        &work.join("state.json"),
        json!([{"id":"task-log","status":"running","pid":42}]),
    );
    let mut config = MonitorConfig {
        state_roots: vec![root],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("version.json"),
        ..MonitorConfig::default()
    };
    config.rescan_interval = Duration::from_millis(100);
    let (monitor, events) = Monitor::start_with(config, Arc::new(Alive), Arc::new(Now));
    wait_until(|| monitor.snapshot().len() == 1);
    monitor.subscribe_log("task-log");
    fs::write(
        work.join("jobs/task-log.log"),
        "[2026-01-01T00:00:00Z] Fictional update\n",
    )
    .unwrap();
    wait_until(|| monitor.snapshot()[0].latest_message.as_deref() == Some("Fictional update"));
    assert!(
        monitor
            .read_log("task-log", 0)
            .unwrap()
            .unwrap()
            .text
            .contains("Fictional update")
    );
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, MonitorEvent::LogAppended { .. }))
    );
    let log = work.join("jobs/task-log.log");
    let prefix =
        "[2026-01-01T00:00:00Z] Fictional update\n\n[2026-01-01T00:00:01Z] Assistant message\n";
    fs::write(&log, prefix).unwrap();
    wait_until(|| monitor.snapshot()[0].latest_message.as_deref() == Some("Assistant message"));
    fs::write(&log, format!("{prefix}Fictional body\n")).unwrap();
    wait_until(|| {
        monitor.snapshot()[0].latest_message.as_deref() == Some("Assistant message: Fictional body")
    });
}

#[test]
fn missing_root_appears_and_config_changes() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("missing-first");
    let second = dir.path().join("second");
    let mut config = MonitorConfig {
        state_roots: vec![first.clone()],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("version.json"),
        ..MonitorConfig::default()
    };
    config.rescan_interval = Duration::from_millis(100);
    let (monitor, _events) = Monitor::start_with(config.clone(), Arc::new(Alive), Arc::new(Now));
    let first_workspace = first.join("fiction-1");
    fs::create_dir_all(&first_workspace).unwrap();
    write_state(
        &first_workspace.join("state.json"),
        json!([{"id":"task-first","status":"completed"}]),
    );
    wait_until(|| monitor.snapshot().len() == 1);
    let second_workspace = second.join("fiction-2");
    fs::create_dir_all(&second_workspace).unwrap();
    write_state(
        &second_workspace.join("state.json"),
        json!([{"id":"task-second","status":"cancelled"}]),
    );
    config.state_roots = vec![second];
    monitor.update_config(config);
    wait_until(|| monitor.snapshot().len() == 1 && monitor.snapshot()[0].id == "task-second");
    drop(monitor);
}
