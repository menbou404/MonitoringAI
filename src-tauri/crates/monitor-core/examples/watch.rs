use monitor_core::{Monitor, MonitorConfig, MonitorEvent, RequesterKind};

fn main() {
    let mut config = MonitorConfig::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--history" {
            config.history_dir = args.next().map(std::path::PathBuf::from);
        }
    }
    let (_monitor, events) = Monitor::start(config);
    println!("Watching Codex jobs. Press Ctrl+C to stop.");
    for event in events {
        match event {
            MonitorEvent::JobsChanged { changed, removed } => {
                for job in changed {
                    println!(
                        "id={} workspace={} status={} phase={} effective={:?} latest={} sessionTitle={} description={} requesterKind={} depth={} previousJobId={} archived={}",
                        job.id,
                        job.workspace_name.as_deref().unwrap_or(""),
                        job.status,
                        job.phase.as_deref().unwrap_or(""),
                        job.effective_state,
                        job.latest_message.as_deref().unwrap_or(""),
                        job.context
                            .as_ref()
                            .and_then(|c| c.session_title.as_deref())
                            .unwrap_or(""),
                        job.context
                            .as_ref()
                            .and_then(|c| c.requester.as_ref())
                            .and_then(|r| r.description.as_deref())
                            .unwrap_or(""),
                        job.context
                            .as_ref()
                            .and_then(|c| c.requester.as_ref())
                            .map_or("", |r| match r.kind {
                                RequesterKind::Subagent => "subagent",
                                RequesterKind::Main => "main",
                            }),
                        job.context
                            .as_ref()
                            .and_then(|c| c.requester.as_ref())
                            .map_or(0, |r| r.depth),
                        job.previous_job_id.as_deref().unwrap_or(""),
                        job.archived,
                    );
                }
                for id in removed {
                    println!("removed id={id}");
                }
            }
            MonitorEvent::StateTransition { id, from, to } => {
                let previous = from.map_or_else(|| "new".to_owned(), |state| format!("{state:?}"));
                println!("id={id} {previous} -> {to:?}")
            }
            MonitorEvent::LogAppended { id, start, end, .. } => {
                println!("id={id} log bytes {start}..{end}")
            }
            MonitorEvent::HealthChanged(health) => println!(
                "health version={} supported={} error={}",
                health.plugin_version.as_deref().unwrap_or(""),
                health.version_supported,
                health.last_error.as_deref().unwrap_or("")
            ),
        }
    }
}
