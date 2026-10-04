//! Read-only monitoring of codex-plugin-cc state files.

mod discovery;
mod liveness;
mod log_tail;
mod model;
mod monitor;
mod parser;
mod store;
mod watcher;

pub use discovery::{MonitorConfig, default_state_roots};
pub use log_tail::read_log_chunk;
pub use monitor::Monitor;

pub use liveness::{Clock, ProcessInfo, ProcessProbe, SystemClock, SystemProcessProbe};
pub use model::*;
pub use parser::{ParseError, read_broker, read_installed_version, read_job, read_state};
