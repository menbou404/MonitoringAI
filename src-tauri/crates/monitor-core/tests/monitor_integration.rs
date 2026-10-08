use std::{
    fs,
    path::Path,
    sync::{Arc, mpsc::Receiver},
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

struct FixedClock(chrono::DateTime<Utc>);
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        self.0
    }
}

struct FixedProcess(chrono::DateTime<Utc>);
impl ProcessProbe for FixedProcess {
    fn probe(&self, _: u32) -> Option<ProcessInfo> {
        Some(ProcessInfo {
            created_at: Some(self.0),
            executable_name: Some("node.exe".into()),
        })
    }
}

fn wait_transition(
    events: &Receiver<MonitorEvent>,
    id: &str,
    from: Option<EffectiveState>,
    to: EffectiveState,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "expected state transition did not arrive"
        );
        let event = events.recv_timeout(remaining).expect("monitor event");
        if let MonitorEvent::StateTransition {
            id: actual,
            from: previous,
            to: next,
        } = event
            && actual == id
            && previous == from
            && next == to
        {
            return;
        }
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
            .read_log("task-log", Some(0))
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
    config.rescan_interval = Duration::from_secs(30);
    let (monitor, _events) = Monitor::start_with(config.clone(), Arc::new(Alive), Arc::new(Now));
    wait_until(|| monitor.health().last_error.as_deref() == Some("No state roots found"));
    for n in 0..8 {
        fs::write(dir.path().join(format!("unrelated-{n}")), "fiction").unwrap();
    }
    let first_workspace = first.join("fiction-1");
    fs::create_dir_all(&first_workspace).unwrap();
    write_state(
        &first_workspace.join("state.json"),
        json!([{"id":"task-first","status":"completed"}]),
    );
    wait_until(|| monitor.snapshot().len() == 1);
    thread::sleep(Duration::from_millis(200));
    write_state(
        &first_workspace.join("state.json"),
        json!([{"id":"task-first","status":"failed"}]),
    );
    wait_until(|| monitor.snapshot()[0].effective_state == EffectiveState::Failed);
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

#[test]
fn background_flow_and_new_job_transition_after_initial_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let workspace = root.join("fiction-1");
    fs::create_dir_all(&workspace).unwrap();
    let state = workspace.join("state.json");
    let existing = json!({"id":"task-existing","status":"completed"});
    write_state(&state, json!([existing.clone()]));
    let base = Utc::now() - chrono::Duration::seconds(10);
    let worker_created = base + chrono::Duration::seconds(1);
    let queued_updated = base + chrono::Duration::seconds(2);
    let started = base + chrono::Duration::seconds(3);
    let mut config = MonitorConfig {
        state_roots: vec![root],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("version.json"),
        ..MonitorConfig::default()
    };
    config.rescan_interval = Duration::from_millis(100);
    config.liveness_interval = Duration::from_millis(100);
    let (monitor, events) = Monitor::start_with(
        config,
        Arc::new(FixedProcess(worker_created)),
        Arc::new(FixedClock(base + chrono::Duration::seconds(4))),
    );
    wait_until(|| monitor.snapshot().len() == 1);
    let first_event = events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(first_event, MonitorEvent::JobsChanged { .. }));
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, MonitorEvent::StateTransition { .. }))
    );

    let queued = json!({"id":"task-background","status":"queued","phase":"queued","pid":42,
        "createdAt":base.to_rfc3339(),"updatedAt":queued_updated.to_rfc3339(),
        "request":{"prompt":"fictional background request"}});
    write_state(&state, json!([existing.clone(), queued]));
    wait_until(|| {
        monitor
            .snapshot()
            .iter()
            .any(|job| job.id == "task-background" && job.effective_state == EffectiveState::Queued)
    });
    wait_transition(&events, "task-background", None, EffectiveState::Queued);

    let running = json!({"id":"task-background","status":"running","phase":"investigating","pid":42,
        "createdAt":base.to_rfc3339(),"updatedAt":queued_updated.to_rfc3339(),
        "startedAt":started.to_rfc3339(),"request":{"prompt":"fictional background request"}});
    write_state(&state, json!([existing.clone(), running]));
    wait_until(|| {
        monitor.snapshot().iter().any(|job| {
            job.id == "task-background"
                && job.effective_state == EffectiveState::Running
                && job.phase.as_deref() == Some("investigating")
        })
    });
    wait_transition(
        &events,
        "task-background",
        Some(EffectiveState::Queued),
        EffectiveState::Running,
    );

    let editing = json!({"id":"task-background","status":"running","phase":"editing","pid":42,
        "createdAt":base.to_rfc3339(),"updatedAt":queued_updated.to_rfc3339(),
        "startedAt":started.to_rfc3339()});
    write_state(&state, json!([existing.clone(), editing]));
    wait_until(|| {
        monitor
            .snapshot()
            .iter()
            .any(|job| job.id == "task-background" && job.phase.as_deref() == Some("editing"))
    });

    write_state(
        &state,
        json!([existing, {"id":"task-background","status":"completed",
        "phase":"done","completedAt":(started + chrono::Duration::seconds(2)).to_rfc3339()}]),
    );
    wait_until(|| {
        monitor.snapshot().iter().any(|job| {
            job.id == "task-background" && job.effective_state == EffectiveState::Completed
        })
    });
    wait_transition(
        &events,
        "task-background",
        Some(EffectiveState::Running),
        EffectiveState::Completed,
    );
}

#[test]
fn read_log_defaults_to_configured_tail_and_reports_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let workspace = root.join("fiction-1");
    fs::create_dir_all(workspace.join("jobs")).unwrap();
    write_state(
        &workspace.join("state.json"),
        json!([{"id":"task-tail","status":"completed"}]),
    );
    fs::write(workspace.join("jobs/task-tail.log"), "0123456789abcdef").unwrap();
    let config = MonitorConfig {
        state_roots: vec![root],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("version.json"),
        initial_log_bytes: 4,
        ..MonitorConfig::default()
    };
    let (monitor, _events) = Monitor::start_with(config, Arc::new(Alive), Arc::new(Now));
    wait_until(|| monitor.snapshot().len() == 1);
    let tail = monitor.read_log("task-tail", None).unwrap().unwrap();
    assert_eq!(
        (
            tail.text.as_str(),
            tail.start,
            tail.end,
            tail.truncated_before
        ),
        ("cdef", 12, 16, true)
    );
    let full = monitor.read_log("task-tail", Some(0)).unwrap().unwrap();
    assert_eq!(full.text, "0123456789abcdef");
    assert!(!full.truncated_before);
}

#[test]
fn unrelated_notifications_do_not_reread_unchanged_state_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let workspace = root.join("fiction-1");
    fs::create_dir_all(&workspace).unwrap();
    let state = workspace.join("state.json");
    write_state(&state, json!([{"id":"task-stable","status":"completed"}]));
    let mut config = MonitorConfig {
        state_roots: vec![root],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("version.json"),
        ..MonitorConfig::default()
    };
    config.rescan_interval = Duration::from_secs(30);
    let (monitor, _events) = Monitor::start_with(config, Arc::new(Alive), Arc::new(Now));
    wait_until(|| monitor.snapshot().len() == 1);
    let before = fs::metadata(&state).unwrap();
    let mut bytes = fs::read(&state).unwrap();
    bytes[0] = b'!';
    fs::write(&state, bytes).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&state)
        .unwrap()
        .set_modified(before.modified().unwrap())
        .unwrap();
    let after = fs::metadata(&state).unwrap();
    assert_eq!(
        (after.modified().unwrap(), after.len()),
        (before.modified().unwrap(), before.len())
    );
    fs::write(workspace.join("unrelated.txt"), "fiction").unwrap();
    thread::sleep(Duration::from_millis(700));
    assert_eq!(monitor.snapshot()[0].id, "task-stable");
    assert_eq!(monitor.health().last_error, None);
}

#[test]
fn snapshot_remains_responsive_during_json_retry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let workspace = root.join("fiction-1");
    fs::create_dir_all(&workspace).unwrap();
    let state = workspace.join("state.json");
    write_state(&state, json!([{"id":"task-stable","status":"completed"}]));
    let mut config = MonitorConfig {
        state_roots: vec![root],
        extra_state_roots: Vec::new(),
        installed_plugins_path: dir.path().join("version.json"),
        ..MonitorConfig::default()
    };
    config.rescan_interval = Duration::from_millis(1);
    let (monitor, _events) = Monitor::start_with(config, Arc::new(Alive), Arc::new(Now));
    wait_until(|| monitor.snapshot().len() == 1);
    fs::write(&state, "{").unwrap();
    thread::sleep(Duration::from_millis(150));
    for _ in 0..3 {
        let started = Instant::now();
        assert_eq!(monitor.snapshot()[0].id, "task-stable");
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "snapshot blocked on JSON retries"
        );
        thread::sleep(Duration::from_millis(30));
    }
}
