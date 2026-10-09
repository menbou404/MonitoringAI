use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::{Job, JobContext, Requester, RequesterKind};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Title {
    custom_title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    description: Option<String>,
    agent_type: Option<String>,
    spawn_depth: Option<u32>,
    tool_use_id: Option<String>,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<DateTime<Utc>>,
    message: Option<Message>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
}
#[derive(Deserialize)]
struct Message {
    content: Option<Vec<Content>>,
}
#[derive(Deserialize)]
struct Content {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
    input: Option<Input>,
    id: Option<String>,
}
#[derive(Deserialize)]
struct Input {
    command: Option<String>,
}

struct Candidate {
    path: Option<PathBuf>,
    time: DateTime<Utc>,
    prompt: bool,
}

fn tail(path: &Path, limit: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(limit);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    if start > 0 {
        let cut = bytes.iter().position(|b| *b == b'\n')? + 1;
        bytes.drain(..cut);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn session_file(projects: &Path, session: &str) -> Option<PathBuf> {
    if !session
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    for entry in fs::read_dir(projects).ok()?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let path = entry.path().join(format!("{session}.jsonl"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn title(main: &Path, session: &str) -> Option<String> {
    let custom = main.parent()?.join(session).join("custom-title.json");
    if let Ok(bytes) = fs::read(custom)
        && let Ok(value) = serde_json::from_slice::<Title>(&bytes)
        && value.custom_title.is_some()
    {
        return value.custom_title;
    }
    tail(main, 1024 * 1024)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Line>(line).ok())
        .filter(|line| line.kind.as_deref() == Some("custom-title"))
        .filter_map(|line| line.custom_title)
        .next_back()
}

type Stamp = Option<(SystemTime, u64)>;
fn stamp(path: &Path) -> Stamp {
    fs::metadata(path)
        .ok()
        .and_then(|m| Some((m.modified().ok()?, m.len())))
}

#[derive(Default)]
pub(crate) struct Resolver {
    locations: BTreeMap<(PathBuf, String), PathBuf>,
    titles: BTreeMap<PathBuf, (Stamp, Stamp, Option<String>)>,
}

impl Resolver {
    fn locate(&mut self, projects: &Path, session: &str) -> Option<PathBuf> {
        let key = (projects.to_path_buf(), session.to_owned());
        if let Some(path) = self.locations.get(&key)
            && path.is_file()
        {
            return Some(path.clone());
        }
        let path = session_file(projects, session)?;
        self.locations.insert(key, path.clone());
        Some(path)
    }
    fn cached_title(&mut self, main: &Path, session: &str) -> Option<String> {
        let main_stamp = stamp(main);
        let custom_stamp = main
            .parent()
            .map(|p| p.join(session).join("custom-title.json"))
            .and_then(|p| stamp(&p));
        if let Some((old_main, old_custom, value)) = self.titles.get(main)
            && *old_main == main_stamp
            && *old_custom == custom_stamp
        {
            return value.clone();
        }
        let value = title(main, session);
        self.titles.insert(
            main.to_path_buf(),
            (main_stamp, custom_stamp, value.clone()),
        );
        value
    }
    pub(crate) fn refresh_title(&mut self, projects: &Path, session: &str) -> Option<String> {
        let main = self.locate(projects, session)?;
        self.cached_title(&main, session)
    }
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn candidates(
    text: &str,
    created: DateTime<Utc>,
    head: Option<&str>,
    path: Option<&Path>,
) -> Vec<Candidate> {
    let mut found = Vec::new();
    for raw in text.lines() {
        let Ok(line) = serde_json::from_str::<Line>(raw) else {
            continue;
        };
        if line.kind.as_deref() != Some("assistant") {
            continue;
        }
        let Some(at) = line.timestamp else {
            continue;
        };
        if at < created - Duration::seconds(60) || at > created + Duration::seconds(5) {
            continue;
        }
        for content in line.message.and_then(|m| m.content).unwrap_or_default() {
            if content.kind.as_deref() != Some("tool_use")
                || content.name.as_deref() != Some("Bash")
            {
                continue;
            }
            let Some(command) = content.input.and_then(|i| i.command) else {
                continue;
            };
            if !command.contains("codex-companion.mjs") {
                continue;
            }
            let prompt = head.is_some_and(|h| normalized(&command).contains(h));
            found.push(Candidate {
                path: path.map(Path::to_path_buf),
                time: at,
                prompt,
            });
        }
    }
    found
}

fn choose(mut found: Vec<Candidate>, created: DateTime<Utc>) -> Option<Candidate> {
    if found.iter().any(|c| c.prompt) {
        found.retain(|c| c.prompt);
    } else if found.len() != 1 {
        return None;
    }
    found.sort_by_key(|c| (created - c.time).num_milliseconds().abs());
    found.into_iter().next()
}

fn meta(path: &Path) -> Option<Meta> {
    let name = path.file_name()?.to_str()?.strip_suffix(".jsonl")?;
    let path = path.with_file_name(format!("{name}.meta.json"));
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn requester(path: &Path, agents: &[PathBuf]) -> Requester {
    let info = meta(path);
    let parent_description = info
        .as_ref()
        .filter(|m| m.spawn_depth.unwrap_or(1) >= 2)
        .and_then(|m| m.tool_use_id.as_ref())
        .and_then(|tool_id| {
            agents
                .iter()
                .filter(|other| other.as_path() != path)
                .find(|other| {
                    tail(other, u64::MAX).is_some_and(|text| {
                        text.lines().any(|raw| {
                            serde_json::from_str::<Line>(raw)
                                .ok()
                                .and_then(|line| line.message)
                                .and_then(|message| message.content)
                                .is_some_and(|content| {
                                    content.iter().any(|c| {
                                        c.kind.as_deref() == Some("tool_use")
                                            && c.id.as_deref() == Some(tool_id)
                                    })
                                })
                        })
                    })
                })
                .and_then(|other| meta(other))
                .and_then(|m| m.description)
        });
    Requester {
        kind: RequesterKind::Subagent,
        description: info.as_ref().and_then(|m| m.description.clone()),
        agent_type: info.as_ref().and_then(|m| m.agent_type.clone()),
        depth: info.as_ref().and_then(|m| m.spawn_depth).unwrap_or(1),
        parent_description,
    }
}

impl Resolver {
    pub(crate) fn resolve(
        &mut self,
        projects: &Path,
        job: &Job,
        prompt: Option<&str>,
    ) -> Option<JobContext> {
        let session = job.session_id.as_ref()?;
        let mut context = JobContext {
            session_id: session.clone(),
            session_title: None,
            requester: None,
        };
        let Some(main) = self.locate(projects, session) else {
            return Some(context);
        };
        context.session_title = self.cached_title(&main, session);
        let Some(created) = job.created_at else {
            return Some(context);
        };
        let agent_dir = main.parent()?.join(session).join("subagents");
        let agents: Vec<PathBuf> = fs::read_dir(agent_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .filter(|p| {
                fs::metadata(p)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .is_some_and(|at| SystemTime::from(created - Duration::minutes(5)) <= at)
            })
            .collect();
        let mut exact = agents
            .iter()
            .filter(|p| tail(p, u64::MAX).is_some_and(|s| s.contains(&job.id)));
        if let Some(path) = exact.next()
            && exact.next().is_none()
        {
            context.requester = Some(requester(path, &agents));
            return Some(context);
        }
        let head = prompt
            .map(normalized)
            .map(|s| s.chars().take(40).collect::<String>())
            .filter(|s| !s.is_empty());
        let found: Vec<_> = agents
            .iter()
            .flat_map(|path| {
                tail(path, u64::MAX)
                    .map(|s| candidates(&s, created, head.as_deref(), Some(path)))
                    .unwrap_or_default()
            })
            .collect();
        if let Some(candidate) = choose(found, created) {
            context.requester = candidate.path.as_deref().map(|p| requester(p, &agents));
            return Some(context);
        }
        if let Some(candidate) = tail(&main, 4 * 1024 * 1024)
            .and_then(|s| choose(candidates(&s, created, head.as_deref(), None), created))
        {
            let _ = candidate;
            context.requester = Some(Requester {
                kind: RequesterKind::Main,
                description: None,
                agent_type: None,
                depth: 0,
                parent_description: None,
            });
        }
        Some(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resolve(projects: &Path, job: &Job, prompt: Option<&str>) -> Option<JobContext> {
        Resolver::default().resolve(projects, job, prompt)
    }
    fn refresh_title(projects: &Path, session: &str) -> Option<String> {
        Resolver::default().refresh_title(projects, session)
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/fake-workspace");
        let agents = project.join("session-fiction/subagents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(project.join("session-fiction.jsonl"), "").unwrap();
        (dir, project, agents)
    }
    fn job(id: &str, seconds: i64) -> Job {
        Job {
            id: id.into(),
            status: "running".into(),
            phase: None,
            kind: None,
            kind_label: None,
            title: None,
            summary: None,
            workspace_root: None,
            write: None,
            session_id: Some("session-fiction".into()),
            pid: None,
            thread_id: None,
            turn_id: None,
            log_file: None,
            created_at: Some(
                DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
                    + Duration::seconds(seconds),
            ),
            updated_at: None,
            started_at: None,
            completed_at: None,
            request: None,
            rendered: None,
            error_message: None,
        }
    }
    fn use_line(command: &str, at: &str, id: &str) -> String {
        json!({"type":"assistant","timestamp":at,"message":{"content":[{"type":"tool_use","name":"Bash","id":id,"input":{"command":command}}]}}).to_string()
    }
    fn agent(
        agents: &Path,
        name: &str,
        description: &str,
        depth: u32,
        tool_id: &str,
        lines: &[String],
    ) {
        fs::write(agents.join(format!("agent-{name}.jsonl")), lines.join("\n")).unwrap();
        fs::write(agents.join(format!("agent-{name}.meta.json")), json!({"description":description,"agentType":"codex:codex-rescue","spawnDepth":depth,"toolUseId":tool_id}).to_string()).unwrap();
    }

    #[test]
    fn background_id_and_nested_parent_description() {
        let (dir, project, agents) = fixture();
        agent(
            &agents,
            "parent",
            "親の調査",
            1,
            "root",
            &[use_line(
                "echo child",
                "2026-10-09T11:59:59Z",
                "spawn-child",
            )],
        );
        agent(&agents, "child", "画面を実装", 2, "spawn-child", &[json!({"type":"user","message":{"content":[{"type":"tool_result","content":"Codex Task started as task-fiction-one"}]}}).to_string()]);
        let context = resolve(
            &dir.path().join("projects"),
            &job("task-fiction-one", 0),
            None,
        )
        .unwrap();
        let requester = context.requester.unwrap();
        assert_eq!(requester.kind, RequesterKind::Subagent);
        assert_eq!(requester.description.as_deref(), Some("画面を実装"));
        assert_eq!(requester.parent_description.as_deref(), Some("親の調査"));
        assert_eq!(requester.depth, 2);
        assert_eq!(requester.agent_type.as_deref(), Some("codex:codex-rescue"));
        let _ = project;
    }

    #[test]
    fn foreground_and_parallel_agents_match_prompts() {
        let (dir, _project, agents) = fixture();
        agent(
            &agents,
            "one",
            "第一依頼",
            1,
            "one",
            &[use_line(
                "node codex-companion.mjs task 'first task distinct request'",
                "2026-10-09T12:00:00Z",
                "call-one",
            )],
        );
        agent(
            &agents,
            "two",
            "第二依頼",
            1,
            "two",
            &[use_line(
                "node codex-companion.mjs task 'second task distinct request'",
                "2026-10-09T12:00:01Z",
                "call-two",
            )],
        );
        let projects = dir.path().join("projects");
        assert_eq!(
            resolve(
                &projects,
                &job("task-a", 2),
                Some("first task distinct request")
            )
            .unwrap()
            .requester
            .unwrap()
            .description
            .as_deref(),
            Some("第一依頼")
        );
        assert_eq!(
            resolve(
                &projects,
                &job("task-b", 2),
                Some("second task distinct request")
            )
            .unwrap()
            .requester
            .unwrap()
            .description
            .as_deref(),
            Some("第二依頼")
        );
        assert!(
            resolve(&projects, &job("task-c", 2), None)
                .unwrap()
                .requester
                .is_none()
        );
    }

    #[test]
    fn main_titles_changes_missing_and_malformed() {
        let (dir, project, _agents) = fixture();
        let main = project.join("session-fiction.jsonl");
        fs::write(
            &main,
            format!(
                "{}\n{}\n{}",
                json!({"type":"custom-title","customTitle":"最初"}),
                use_line(
                    "node codex-companion.mjs task 'main direct request'",
                    "2026-10-09T12:00:00Z",
                    "call-main"
                ),
                json!({"type":"custom-title","customTitle":"変更後"})
            ),
        )
        .unwrap();
        let projects = dir.path().join("projects");
        let context =
            resolve(&projects, &job("task-main", 1), Some("main direct request")).unwrap();
        assert_eq!(context.session_title.as_deref(), Some("変更後"));
        assert_eq!(context.requester.unwrap().kind, RequesterKind::Main);
        fs::write(
            project.join("session-fiction/custom-title.json"),
            r#"{"customTitle":"専用タイトル"}"#,
        )
        .unwrap();
        assert_eq!(
            refresh_title(&projects, "session-fiction").as_deref(),
            Some("専用タイトル")
        );
        fs::write(project.join("session-fiction/custom-title.json"), "{").unwrap();
        fs::write(&main, "garbage\n").unwrap();
        let context = resolve(&projects, &job("task-other", 1), None).unwrap();
        assert_eq!(context.session_title, None);
        assert_eq!(context.requester, None);
        fs::remove_file(main).unwrap();
        assert_eq!(
            resolve(&projects, &job("task-missing", 1), None)
                .unwrap()
                .requester,
            None
        );
    }

    #[test]
    fn title_cache_reloads_when_file_changes() {
        let (dir, project, _agents) = fixture();
        let projects = dir.path().join("projects");
        let path = project.join("session-fiction/custom-title.json");
        let mut resolver = Resolver::default();
        assert_eq!(resolver.refresh_title(&projects, "session-fiction"), None);
        fs::write(&path, r#"{"customTitle":"短い題"}"#).unwrap();
        assert_eq!(
            resolver
                .refresh_title(&projects, "session-fiction")
                .as_deref(),
            Some("短い題")
        );
        fs::write(&path, r#"{"customTitle":"更新後の長い題名"}"#).unwrap();
        assert_eq!(
            resolver
                .refresh_title(&projects, "session-fiction")
                .as_deref(),
            Some("更新後の長い題名")
        );
    }
}
