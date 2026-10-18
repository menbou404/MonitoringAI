use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct JobContext {
    pub session_id: String,
    pub session_title: Option<String>,
    pub requester: Option<Requester>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct Requester {
    pub kind: RequesterKind,
    pub description: Option<String>,
    pub agent_type: Option<String>,
    pub depth: u32,
    pub parent_description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub enum RequesterKind {
    Subagent,
    Main,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub status: String,
    pub phase: Option<String>,
    pub kind: Option<String>,
    pub kind_label: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub workspace_root: Option<String>,
    pub write: Option<bool>,
    pub session_id: Option<String>,
    pub pid: Option<u32>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub log_file: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub request: Option<JobRequest>,
    pub rendered: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct Broker {
    pub pid: Option<u32>,
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveState {
    Queued,
    Running,
    Quiet,
    Orphaned,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub workspace_root: Option<String>,
    pub workspace_name: Option<String>,
    pub kind: Option<String>,
    pub kind_label: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub status: String,
    pub phase: Option<String>,
    pub effective_state: EffectiveState,
    pub created_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub pid: Option<u32>,
    pub thread_id: Option<String>,
    pub write: Option<bool>,
    pub session_id: Option<String>,
    pub latest_message: Option<String>,
    pub log_modified_at: Option<DateTime<Utc>>,
    pub context: Option<JobContext>,
    pub previous_job_id: Option<String>,
    pub archived: bool,
    pub archived_note: Option<String>,
    pub prompt_head: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct JobDetail {
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub summary: JobSummary,
    pub prompt: Option<String>,
    pub turn_id: Option<String>,
    pub rendered: Option<String>,
    pub error_message: Option<String>,
    pub log_file: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub text: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end: u64,
    pub truncated_before: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub plugin_version: Option<String>,
    pub version_supported: bool,
    pub watched_roots: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MonitorEvent {
    JobsChanged {
        changed: Vec<JobSummary>,
        removed: Vec<String>,
    },
    StateTransition {
        id: String,
        from: Option<EffectiveState>,
        to: EffectiveState,
    },
    LogAppended {
        id: String,
        text: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        start: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        end: u64,
    },
    HealthChanged(Health),
}
