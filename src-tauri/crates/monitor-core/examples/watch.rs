use monitor_core::{Monitor, MonitorConfig, MonitorEvent};

fn main() {
    let (_monitor, events) = Monitor::start(MonitorConfig::default());
    println!("Watching Codex jobs. Press Ctrl+C to stop.");
    for event in events {
        match event {
            MonitorEvent::JobsChanged { changed, removed } => {
                for job in changed {
                    println!(
                        "id={} workspace={} status={} phase={} effective={:?} latest={}",
                        job.id,
                        job.workspace_name.as_deref().unwrap_or(""),
                        job.status,
                        job.phase.as_deref().unwrap_or(""),
                        job.effective_state,
                        job.latest_message.as_deref().unwrap_or("")
                    );
                }
                for id in removed {
                    println!("removed id={id}");
                }
            }
            MonitorEvent::StateTransition { id, from, to } => {
                println!("id={id} {from:?} -> {to:?}")
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
