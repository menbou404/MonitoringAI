use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn default_state_roots() -> Vec<PathBuf> {
    state_roots_from(
        std::env::var_os("USERPROFILE").map(PathBuf::from),
        std::env::var_os("TEMP").map(PathBuf::from),
        std::env::var_os("CLAUDE_PLUGIN_DATA").map(PathBuf::from),
    )
}

fn state_roots_from(
    home: Option<PathBuf>,
    temp: Option<PathBuf>,
    data: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home {
        roots.push(home.join(".claude/plugins/data/codex-openai-codex/state"));
    }
    if let Some(temp) = temp {
        roots.push(temp.join("codex-companion"));
    }
    if let Some(data) = data {
        roots.push(data.join("state"));
    }
    roots
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Clone, Debug)]
pub struct MonitorConfig {
    pub state_roots: Vec<PathBuf>,
    pub extra_state_roots: Vec<PathBuf>,
    pub installed_plugins_path: PathBuf,
    pub supported_plugin_versions: Vec<String>,
    pub quiet_after: Duration,
    pub rescan_interval: Duration,
    pub liveness_interval: Duration,
    pub debounce: Duration,
    pub initial_log_bytes: u64,
    pub claude_projects_dir: Option<PathBuf>,
    pub history_dir: Option<PathBuf>,
    pub history_days: u32,
    pub codex_home: Option<PathBuf>,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            state_roots: default_state_roots(),
            extra_state_roots: Vec::new(),
            installed_plugins_path: std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".claude/plugins/installed_plugins.json"),
            supported_plugin_versions: vec!["1.0.".into()],
            quiet_after: Duration::from_secs(600),
            rescan_interval: Duration::from_secs(5),
            liveness_interval: Duration::from_secs(5),
            debounce: Duration::from_millis(200),
            initial_log_bytes: 1024 * 1024,
            claude_projects_dir: std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|home| home.join(".claude/projects")),
            history_dir: None,
            history_days: 30,
            codex_home: None,
        }
    }
}

impl MonitorConfig {
    pub(crate) fn roots(&self) -> Vec<PathBuf> {
        self.state_roots
            .iter()
            .chain(&self.extra_state_roots)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

pub(crate) fn state_files(roots: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let state = path.join("state.json");
                if state.is_file() {
                    files.insert(state);
                }
            }
        }
    }
    files
}

pub(crate) fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discover_new_and_removed_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        assert!(state_files(std::slice::from_ref(&root)).is_empty());
        let workspace = root.join("fiction-123");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("state.json"), "{}").unwrap();
        assert_eq!(state_files(std::slice::from_ref(&root)).len(), 1);
        std::fs::remove_dir_all(&workspace).unwrap();
        assert!(state_files(&[root]).is_empty());
    }

    #[test]
    fn default_roots_include_home_temp_and_plugin_override() {
        let home = PathBuf::from("fiction/home");
        let temp = PathBuf::from("fiction/temp");
        let default_data = home.join(".claude/plugins/data/codex-openai-codex");
        let roots = state_roots_from(Some(home.clone()), Some(temp.clone()), Some(default_data));
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&home.join(".claude/plugins/data/codex-openai-codex/state")));
        assert!(roots.contains(&temp.join("codex-companion")));
        let roots = state_roots_from(
            Some(home),
            Some(temp),
            Some(PathBuf::from("fiction/override")),
        );
        assert_eq!(roots.len(), 3);
        assert!(roots.contains(&PathBuf::from("fiction/override/state")));
    }
}
