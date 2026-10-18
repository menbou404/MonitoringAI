use std::{fs, path::Path};

use monitor_core::Usage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelAgent {
    Codex,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
#[serde(rename_all = "kebab-case")]
pub enum SwitchTrigger {
    AutoSaver,
    AutoRevert,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
pub struct ModelDefault {
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../src/bindings/")
)]
pub struct ModelSwitchEntry {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub time: i64,
    pub agent: ModelAgent,
    pub trigger: SwitchTrigger,
    pub before: ModelDefault,
    pub after: ModelDefault,
    #[cfg_attr(feature = "ts", ts(type = "import('./Usage').Usage | null"))]
    pub usage: Option<Usage>,
}

pub fn load(path: &Path) -> Vec<ModelSwitchEntry> {
    match fs::read(path).and_then(|bytes| {
        serde_json::from_slice::<Vec<ModelSwitchEntry>>(&bytes).map_err(std::io::Error::other)
    }) {
        Ok(mut entries) => {
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.time));
            entries.truncate(100);
            entries
        }
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "model switch history could not be read");
            }
            Vec::new()
        }
    }
}

pub fn record(path: &Path, entries: &mut Vec<ModelSwitchEntry>, entry: ModelSwitchEntry) {
    entries.insert(0, entry);
    entries.truncate(100);
    let temp = path.with_extension("json.tmp");
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(entries).map_err(std::io::Error::other)?;
        fs::write(&temp, bytes)?;
        fs::rename(&temp, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        tracing::warn!(%error, "model switch history could not be written");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_newest_first_round_trip_and_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        assert!(load(&path).is_empty());
        fs::write(&path, "{").unwrap();
        assert!(load(&path).is_empty());
        let mut entries = Vec::new();
        for time in 0..105 {
            record(
                &path,
                &mut entries,
                ModelSwitchEntry {
                    time,
                    agent: ModelAgent::Codex,
                    trigger: SwitchTrigger::Manual,
                    before: ModelDefault {
                        model: None,
                        effort: None,
                    },
                    after: ModelDefault {
                        model: Some("synthetic".into()),
                        effort: Some("low".into()),
                    },
                    usage: None,
                },
            );
        }
        assert_eq!(entries.len(), 100);
        assert_eq!(entries[0].time, 104);
        assert_eq!(entries[99].time, 5);
        assert_eq!(load(&path), entries);
        // Failure keeps the in-memory entry and does not propagate to the switch.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let entry = entries[0].clone();
        record(&path, &mut entries, entry.clone());
        assert_eq!(entries[0], entry);
        assert!(!path.with_extension("json.tmp").exists());
    }
}
