use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::Job;

const TAIL_BYTES: u64 = 128 * 1024;
const RECENT_FILES: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub used_percent: f64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub resets_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub five_hour: Option<UsageWindow>,
    pub week: Option<UsageWindow>,
}

impl Usage {
    pub fn at(&self, now: i64) -> Self {
        let reset = |window: &Option<UsageWindow>| {
            window.as_ref().map(|w| UsageWindow {
                used_percent: if now >= w.resets_at {
                    0.0
                } else {
                    w.used_percent
                },
                resets_at: w.resets_at,
            })
        };
        Self {
            five_hour: reset(&self.five_hour),
            week: reset(&self.week),
        }
    }
}

#[derive(Default, Deserialize)]
struct Event {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    payload: Payload,
}

#[derive(Default, Deserialize)]
struct Payload {
    #[serde(rename = "type", default)]
    kind: String,
    rate_limits: Option<Limits>,
    turn_id: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Deserialize)]
struct Limits {
    primary: Option<Window>,
    secondary: Option<Window>,
}

#[derive(Deserialize)]
struct Window {
    used_percent: f64,
    window_minutes: u32,
    resets_at: i64,
}

fn usage_tail(path: &Path) -> Option<Usage> {
    let mut file = File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let bytes = if start > 0 {
        &bytes[bytes.iter().position(|b| *b == b'\n')? + 1..]
    } else {
        &bytes
    };
    for line in bytes.rsplit(|b| *b == b'\n') {
        let Ok(event) = serde_json::from_slice::<Event>(line) else {
            continue;
        };
        if event.kind != "event_msg" || event.payload.kind != "token_count" {
            continue;
        }
        let Some(limits) = event.payload.rate_limits else {
            continue;
        };
        let mut usage = Usage::default();
        for w in [limits.primary, limits.secondary].into_iter().flatten() {
            if !w.used_percent.is_finite() || w.used_percent < 0.0 {
                continue;
            }
            let target = match w.window_minutes {
                300 => &mut usage.five_hour,
                10080 => &mut usage.week,
                _ => continue,
            };
            *target = Some(UsageWindow {
                used_percent: w.used_percent,
                resets_at: w.resets_at,
            });
        }
        if usage.five_hour.is_some() || usage.week.is_some() {
            return Some(usage);
        }
    }
    None
}

fn rollout_files(home: &Path) -> Vec<(SystemTime, PathBuf)> {
    let mut dirs = vec![home.join("sessions")];
    for _ in 0..3 {
        dirs = dirs
            .into_iter()
            .flat_map(|dir| fs::read_dir(dir).into_iter().flatten().flatten())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
    }
    let mut files: Vec<_> = dirs
        .into_iter()
        .flat_map(|dir| fs::read_dir(dir).into_iter().flatten().flatten())
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                return None;
            }
            let meta = e.metadata().ok()?;
            meta.is_file()
                .then(|| (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        })
        .collect();
    files.sort_by(|a, b| b.cmp(a));
    files.truncate(RECENT_FILES);
    files
}

#[derive(Default)]
pub struct UsageReader {
    tails: BTreeMap<PathBuf, (SystemTime, u64, Option<Usage>)>,
}

impl UsageReader {
    pub fn read(&mut self, home: &Path, now: i64) -> Option<Usage> {
        let files = rollout_files(home);
        self.tails
            .retain(|path, _| files.iter().any(|(_, p)| p == path));
        for (modified, path) in files {
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let cached =
                self.tails
                    .entry(path.clone())
                    .or_insert((SystemTime::UNIX_EPOCH, 0, None));
            if cached.0 != modified || cached.1 != meta.len() {
                *cached = (modified, meta.len(), usage_tail(&path));
            }
            if let Some(usage) = &cached.2 {
                return Some(usage.at(now));
            }
        }
        None
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobModel {
    pub model: Option<String>,
    pub effort: Option<String>,
}

type ModelKey = (Option<String>, Option<String>, Option<DateTime<Utc>>);

type FileStamp = (PathBuf, Option<SystemTime>, u64);

struct CachedModel {
    key: ModelKey,
    stamp: Option<FileStamp>,
    resolved: Option<JobModel>,
    fallback: JobModel,
}

#[derive(Default)]
pub struct JobModelReader {
    jobs: BTreeMap<String, CachedModel>,
}

impl JobModelReader {
    fn cached(&mut self, job: &Job) -> &mut CachedModel {
        let key = (job.thread_id.clone(), job.turn_id.clone(), job.created_at);
        let cached = self
            .jobs
            .entry(job.id.clone())
            .or_insert_with(|| CachedModel {
                key: key.clone(),
                stamp: None,
                resolved: None,
                fallback: JobModel::default(),
            });
        if cached.key != key {
            cached.key = key;
            cached.stamp = None;
            cached.resolved = None;
        }
        if let Some(request) = &job.request {
            cached.fallback = JobModel {
                model: request.model.clone(),
                effort: request.effort.clone(),
            };
        }
        cached
    }

    pub(crate) fn request_model(&mut self, job: &Job) -> JobModel {
        self.cached(job).fallback.clone()
    }

    pub fn read(&mut self, home: &Path, job: &Job) -> JobModel {
        let cached = self.cached(job);
        if cached.resolved.is_none()
            && let Some(path) = find_rollout(home, job)
            && let Ok(meta) = fs::metadata(&path)
        {
            let stamp = (path.clone(), meta.modified().ok(), meta.len());
            if cached.stamp.as_ref() != Some(&stamp) {
                cached.resolved = read_model(&path, job.turn_id.as_deref());
                cached.stamp = Some(stamp);
            }
        }
        let found = cached.resolved.as_ref();
        JobModel {
            model: found
                .and_then(|m| m.model.clone())
                .or_else(|| cached.fallback.model.clone()),
            effort: found
                .and_then(|m| m.effort.clone())
                .or_else(|| cached.fallback.effort.clone()),
        }
    }

    pub fn retain(&mut self, ids: &std::collections::BTreeSet<String>) {
        self.jobs.retain(|id, _| ids.contains(id));
    }
}

fn find_rollout(home: &Path, job: &Job) -> Option<PathBuf> {
    let thread = job
        .thread_id
        .as_deref()
        .filter(|s| crate::store::safe_id(s))?;
    let date = job.created_at?.date_naive();
    for offset in [0, -1, 1] {
        let date = date.checked_add_signed(Duration::days(offset))?;
        let dir = home
            .join("sessions")
            .join(date.format("%Y/%m/%d").to_string());
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("rollout-") && name.ends_with(&format!("-{thread}.jsonl")) {
                return Some(entry.path());
            }
        }
    }
    None
}

fn read_model(path: &Path, turn_id: Option<&str>) -> Option<JobModel> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file).split(b'\n') {
        let Ok(line) = line else { break };
        let Ok(event) = serde_json::from_slice::<Event>(&line) else {
            continue;
        };
        if event.kind == "turn_context"
            && turn_id.is_none_or(|id| event.payload.turn_id.as_deref() == Some(id))
        {
            return Some(JobModel {
                model: event.payload.model,
                effort: event.payload.effort,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rollout(home: &Path, day: &str, thread: &str) -> PathBuf {
        let dir = home.join("sessions").join(day);
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("rollout-fiction-{thread}.jsonl"))
    }

    fn token(five: f64, week: f64, reset: i64) -> String {
        json!({"type":"event_msg","payload":{"type":"token_count","ignored":"private fiction",
            "rate_limits":{"secondary":{"used_percent":five,"window_minutes":300,"resets_at":reset},
            "primary":{"used_percent":week,"window_minutes":10080,"resets_at":reset + 100}}}})
        .to_string()
    }

    fn job(home: &Path) -> Job {
        let path = home.join("job.json");
        fs::write(&path, json!({"id":"fictional-job","status":"running","threadId":"thread-one","turnId":"second",
            "createdAt":"2026-10-16T00:01:00Z","request":{"model":"fallback","effort":"low"}}).to_string()).unwrap();
        crate::read_job(&path).unwrap()
    }

    #[test]
    fn tail_uses_latest_valid_token_and_window_lengths_and_resets_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = rollout(dir.path(), "2026/10/16", "one");
        fs::write(
            &path,
            format!(
                "{}\n{}\nbroken\n{{\"type\":",
                token(2.0, 3.0, 100),
                token(92.5, 18.0, 100)
            ),
        )
        .unwrap();
        let mut reader = UsageReader::default();
        let usage = reader.read(dir.path(), 99).unwrap();
        assert_eq!(usage.five_hour.unwrap().used_percent, 92.5);
        assert_eq!(usage.week.unwrap().used_percent, 18.0);
        let usage = reader.read(dir.path(), 100).unwrap();
        assert_eq!(usage.five_hour.unwrap().used_percent, 0.0);
        assert_eq!(usage.week.unwrap().used_percent, 18.0);
        assert_eq!(
            reader
                .read(dir.path(), 201)
                .unwrap()
                .week
                .unwrap()
                .used_percent,
            0.0
        );
    }

    #[test]
    fn reads_only_a_bounded_tail_and_skips_its_cut_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = rollout(dir.path(), "2026/10/16", "one");
        fs::write(
            &path,
            format!(
                "{}\n{}",
                token(99.0, 99.0, 100),
                "x".repeat(TAIL_BYTES as usize + 20)
            ),
        )
        .unwrap();
        assert!(usage_tail(&path).is_none());
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        writeln!(file, "\n{}", token(1.0, 2.0, 100)).unwrap();
        assert_eq!(
            usage_tail(&path).unwrap().five_hour.unwrap().used_percent,
            1.0
        );
    }

    #[test]
    fn newest_files_first_with_fallback_to_an_older_valid_tail() {
        let dir = tempfile::tempdir().unwrap();
        let old = rollout(dir.path(), "2026/10/15", "old");
        let new = rollout(dir.path(), "2026/10/16", "new");
        fs::write(&old, token(10.0, 20.0, 100)).unwrap();
        fs::write(&new, token(30.0, 40.0, 100)).unwrap();
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap();
        File::options()
            .write(true)
            .open(&new)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2))
            .unwrap();
        let mut reader = UsageReader::default();
        assert_eq!(
            reader
                .read(dir.path(), 0)
                .unwrap()
                .five_hour
                .unwrap()
                .used_percent,
            30.0
        );
        fs::write(&new, "{broken\n").unwrap();
        assert_eq!(
            reader
                .read(dir.path(), 0)
                .unwrap()
                .five_hour
                .unwrap()
                .used_percent,
            10.0
        );
    }

    #[test]
    fn model_matches_turn_and_adjacent_date_caches_and_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut job = job(dir.path());
        let path = rollout(dir.path(), "2026/10/15", "thread-one");
        fs::write(&path, concat!("{broken\n", "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"first\",\"model\":\"first-model\",\"effort\":\"low\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"second\",\"model\":\"second-model\",\"effort\":\"high\",\"ignored\":\"private fiction\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"second\",\"model\":\"wrong\"}}\n")).unwrap();
        let mut reader = JobModelReader::default();
        let model = reader.read(dir.path(), &job);
        assert_eq!(model.model.as_deref(), Some("second-model"));
        assert_eq!(model.effort.as_deref(), Some("high"));
        fs::remove_file(&path).unwrap();
        assert_eq!(reader.read(dir.path(), &job), model);
        job.turn_id = None;
        assert_eq!(
            reader.read(dir.path(), &job).model.as_deref(),
            Some("fallback")
        );
        fs::write(&path, "{\"type\":\"turn_context\",\"payload\":{\"model\":\"first-model\",\"effort\":\"medium\"}}\n").unwrap();
        assert_eq!(
            reader.read(dir.path(), &job).model.as_deref(),
            Some("first-model")
        );
        job.thread_id = Some("missing".into());
        job.request = None;
        job.id = "without-request".into();
        assert_eq!(reader.read(dir.path(), &job), JobModel::default());
    }

    #[test]
    fn incomplete_context_is_retried_only_when_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path());
        let path = rollout(dir.path(), "2026/10/17", "thread-one");
        fs::write(&path, "{\"type\":\"turn_context\"").unwrap();
        let mut reader = JobModelReader::default();
        assert_eq!(
            reader.read(dir.path(), &job).model.as_deref(),
            Some("fallback")
        );
        let before = reader.jobs[&job.id].stamp.clone();
        reader.read(dir.path(), &job);
        assert_eq!(reader.jobs[&job.id].stamp, before);
        fs::write(&path, "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"second\",\"model\":\"arrived\"}}\n").unwrap();
        assert_eq!(
            reader.read(dir.path(), &job).model.as_deref(),
            Some("arrived")
        );
    }

    #[test]
    fn late_turn_id_drops_the_first_context_and_preserves_only_request_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let mut job = job(dir.path());
        job.turn_id = None;
        let path = rollout(dir.path(), "2026/10/16", "thread-one");
        fs::write(&path, "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"first\",\"model\":\"first-model\"}}\n").unwrap();
        let mut reader = JobModelReader::default();
        assert_eq!(
            reader.read(dir.path(), &job).model.as_deref(),
            Some("first-model")
        );
        job.turn_id = Some("late-turn".into());
        job.request = None;
        assert_eq!(
            reader.read(dir.path(), &job).model.as_deref(),
            Some("fallback")
        );
        let mut without_request = job.clone();
        without_request.id = "without-request".into();
        assert!(reader.read(dir.path(), &without_request).model.is_none());
    }
}
