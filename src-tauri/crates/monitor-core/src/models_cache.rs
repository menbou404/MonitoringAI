use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
pub struct ReasoningLevel {
    pub effort: String,
    pub description: String,
}

// Deliberately deserialize only the fields needed by the model page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../../src/bindings/")
)]
pub struct ModelInfo {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub default_reasoning_level: Option<String>,
    pub supported_reasoning_levels: Vec<ReasoningLevel>,
    pub visibility: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub priority: i64,
}

#[derive(Deserialize)]
struct Cache {
    models: Vec<ModelInfo>,
}

#[derive(Default)]
pub struct ModelsReader {
    stamp: Option<(PathBuf, SystemTime)>,
    models: Option<Vec<ModelInfo>>,
}

impl ModelsReader {
    pub fn read(&mut self, home: &Path) -> Option<Vec<ModelInfo>> {
        let path = home.join("models_cache.json");
        let stamp = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .map(|mtime| (path.clone(), mtime));
        if stamp.is_none() {
            self.stamp = None;
            self.models = None;
        } else if stamp != self.stamp || self.models.is_none() {
            self.stamp = stamp;
            self.models = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
                .map(|cache| {
                    let mut models: Vec<_> = cache
                        .models
                        .into_iter()
                        .filter(|m| m.visibility == "list")
                        .collect();
                    models.sort_by_key(|m| m.priority);
                    models
                });
        }
        self.models.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(slug: &str, visibility: &str, priority: i64) -> serde_json::Value {
        serde_json::json!({"slug":slug,"display_name":slug,"description":"Synthetic model", "default_reasoning_level":"low",
            "supported_reasoning_levels":[{"effort":"low","description":"Fast","private":"ignored"}],
            "visibility":visibility,"priority":priority,"private":"never exported"})
    }

    #[test]
    fn filters_sorts_and_exports_only_allowed_fields() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("models_cache.json"),
            serde_json::json!({"private":"ignored", "models":[
                model("later", "list", 2), model("hidden", "hide", 0), model("first", "list", 1)
            ]})
            .to_string(),
        )
        .unwrap();
        let models = ModelsReader::default().read(dir.path()).unwrap();
        assert_eq!(
            models.iter().map(|m| m.slug.as_str()).collect::<Vec<_>>(),
            ["first", "later"]
        );
        let exported = serde_json::to_string(&models).unwrap();
        assert!(!exported.contains("private"));
        assert!(!exported.contains("never exported"));
        assert_eq!(models[0].supported_reasoning_levels[0].effort, "low");
    }

    #[test]
    fn missing_broken_and_changed_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models_cache.json");
        let mut reader = ModelsReader::default();
        assert!(reader.read(dir.path()).is_none());
        fs::write(&path, "{").unwrap();
        assert!(reader.read(dir.path()).is_none());
        fs::write(
            &path,
            serde_json::json!({"models":[model("first", "list", 0)]}).to_string(),
        )
        .unwrap();
        assert_eq!(reader.read(dir.path()).unwrap()[0].slug, "first");
        let previous = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(
            &path,
            serde_json::json!({"models":[model("new", "list", 0)]}).to_string(),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(previous + std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(reader.read(dir.path()).unwrap()[0].slug, "new");
        // Same mtime: keep the cached value.
        fs::write(&path, "{").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(previous + std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(reader.read(dir.path()).unwrap()[0].slug, "new");
        fs::remove_file(&path).unwrap();
        assert!(reader.read(dir.path()).is_none());
    }
}
