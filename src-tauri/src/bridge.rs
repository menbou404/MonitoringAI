use crate::{notify, state::AppState, tray};
use monitor_core::{EffectiveState, MonitorEvent};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

pub fn start(app: AppHandle, events: Receiver<MonitorEvent>) {
    thread::spawn(move || {
        loop {
            match events.recv_timeout(Duration::from_secs(60)) {
                Ok(event) => handle(&app, event),
                Err(RecvTimeoutError::Timeout) => tray::refresh(&app),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn handle(app: &AppHandle, event: MonitorEvent) {
    let state = app.state::<AppState>();
    let jobs = state.monitor.snapshot();
    let (settings, target, unseen, old_watch) = {
        let mut snap = state.inner.lock().expect("app state");
        let before = snap.unseen.clone();
        let old_watch = snap.watched_log.clone();
        snap.apply(&event, jobs);
        let target = if let MonitorEvent::StateTransition { id, .. } = &event {
            snap.jobs.iter().find(|j| &j.id == id).cloned()
        } else {
            None
        };
        let unseen =
            (before != snap.unseen).then(|| snap.unseen.iter().cloned().collect::<Vec<_>>());
        (
            snap.settings.clone(),
            target,
            unseen,
            old_watch.filter(|id| snap.watched_log.as_ref() != Some(id)),
        )
    };
    if let Some(id) = old_watch {
        state.monitor.unsubscribe_log(&id);
    }
    if let MonitorEvent::StateTransition { id, from, to } = &event
        && let Some(job) = target
    {
        let detail = if *to == EffectiveState::Failed || *to == EffectiveState::Running {
            state.monitor.job_detail(id).and_then(|d| {
                if *to == EffectiveState::Failed {
                    d.error_message
                } else {
                    d.prompt
                }
            })
        } else {
            None
        };
        notify::transition(app, from.as_ref(), to, &job, detail.as_deref(), &settings);
    }
    if app.get_webview_window("main").is_some()
        && let Err(error) = app.emit_to("main", "monitor-event", &event)
    {
        tracing::warn!(%error,"window event failed");
    }
    if let Some(unseen) = unseen {
        let _ = app.emit_to("main", "unseen-changed", unseen);
    }
    if !matches!(event, MonitorEvent::LogAppended { .. }) {
        tray::refresh(app);
    }
}
