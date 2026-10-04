use std::{fs::File, io::Read, path::Path, thread, time::Duration};

use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

use crate::{Broker, Job, JobRequest};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing id or status")]
    MissingRequired,
}

fn read_json(path: &Path) -> Result<Value, ParseError> {
    let mut last = None;
    for attempt in 0..=3 {
        if attempt != 0 {
            thread::sleep(Duration::from_millis(100));
        }
        let mut bytes = Vec::new();
        File::open(path)?.read_to_end(&mut bytes)?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => return Ok(value),
            Err(error) => last = Some(error),
        }
    }
    Err(ParseError::Json(last.expect("four attempts")))
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

fn timestamp(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.get(key)?.as_str()?)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn job(value: &Value) -> Result<Job, ParseError> {
    let request = value
        .get("request")
        .filter(|r| r.is_object())
        .map(|r| JobRequest {
            prompt: string(r, "prompt"),
            model: string(r, "model"),
            effort: string(r, "effort"),
        });
    Ok(Job {
        id: string(value, "id").ok_or(ParseError::MissingRequired)?,
        status: string(value, "status").ok_or(ParseError::MissingRequired)?,
        phase: string(value, "phase"),
        kind: string(value, "kind"),
        kind_label: string(value, "kindLabel"),
        title: string(value, "title"),
        summary: string(value, "summary"),
        workspace_root: string(value, "workspaceRoot"),
        write: value.get("write").and_then(Value::as_bool),
        session_id: string(value, "sessionId"),
        pid: value
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| pid.try_into().ok()),
        thread_id: string(value, "threadId"),
        turn_id: string(value, "turnId"),
        log_file: string(value, "logFile"),
        created_at: timestamp(value, "createdAt"),
        updated_at: timestamp(value, "updatedAt"),
        started_at: timestamp(value, "startedAt"),
        completed_at: timestamp(value, "completedAt"),
        request,
        rendered: string(value, "rendered"),
        error_message: string(value, "errorMessage"),
    })
}

pub fn read_state(path: &Path) -> Result<Vec<Job>, ParseError> {
    let value = read_json(path)?;
    value
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or(ParseError::MissingRequired)?
        .iter()
        .map(job)
        .collect()
}

pub fn read_job(path: &Path) -> Result<Job, ParseError> {
    job(&read_json(path)?)
}

pub fn read_broker(path: &Path) -> Result<Broker, ParseError> {
    let value = read_json(path)?;
    Ok(Broker {
        pid: value
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| pid.try_into().ok()),
        endpoint: string(&value, "endpoint"),
    })
}

pub fn read_installed_version(path: &Path) -> Result<Option<String>, ParseError> {
    let value = read_json(path)?;
    Ok(value
        .pointer("/plugins/codex@openai-codex/0/version")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_unknown_and_invalid_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"jobs":[{"id":"made-up","status":"running","phase":"future","createdAt":"bad","newField":7}]}"#).unwrap();
        let jobs = read_state(&path).unwrap();
        assert_eq!(jobs[0].phase.as_deref(), Some("future"));
        assert_eq!(jobs[0].created_at, None);
        assert_eq!(jobs[0].thread_id, None);
        std::fs::write(&path, "{").unwrap();
        assert!(read_state(&path).is_err());
        std::fs::write(&path, "").unwrap();
        assert!(read_state(&path).is_err());
    }

    #[test]
    fn foreground_failed_cancelled_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"jobs":[
            {"id":"foreground-fiction","status":"completed","phase":"done","summary":"fictional foreground request"},
            {"id":"failed-fiction","status":"failed","errorMessage":"fictional failure"},
            {"id":"cancel-fiction","status":"cancelled"},
            {"id":"future-fiction","status":"future","phase":"future-phase","newField":true}
        ]}"#).unwrap();
        let jobs = read_state(&path).unwrap();
        assert!(jobs[0].request.is_none());
        assert_eq!(jobs[1].error_message.as_deref(), Some("fictional failure"));
        assert_eq!(jobs[2].status, "cancelled");
        assert_eq!(jobs[3].phase.as_deref(), Some("future-phase"));
        assert!(jobs[3].session_id.is_none());
    }
}
