use std::{collections::BTreeMap, path::PathBuf, sync::mpsc::Sender, time::Duration};

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::discovery::nearest_existing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WatchTarget {
    pub path: PathBuf,
    pub recursive: bool,
}

pub(crate) fn watch_targets(roots: &[PathBuf]) -> Vec<WatchTarget> {
    let mut targets = BTreeMap::<PathBuf, bool>::new();
    for root in roots {
        if let Some(existing) = nearest_existing(root) {
            let recursive = existing == *root;
            targets
                .entry(existing)
                .and_modify(|mode| *mode |= recursive)
                .or_insert(recursive);
        }
    }
    targets
        .into_iter()
        .map(|(path, recursive)| WatchTarget { path, recursive })
        .collect()
}

pub(crate) struct RootWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl RootWatcher {
    pub(crate) fn start(
        roots: &[PathBuf],
        debounce: Duration,
        wake: Sender<()>,
    ) -> notify::Result<Self> {
        let watched: Vec<_> = roots
            .iter()
            .filter_map(|root| nearest_existing(root).map(|parent| (root.clone(), parent)))
            .collect();
        let mut debouncer =
            new_debouncer(
                debounce,
                None,
                move |result: DebounceEventResult| match result {
                    Ok(events) => {
                        let relevant = events.iter().flat_map(|event| &event.paths).any(|path| {
                            watched.iter().any(|(root, parent)| {
                                if root == parent {
                                    path.starts_with(root)
                                } else {
                                    path != parent
                                        && path.starts_with(parent)
                                        && root.starts_with(path)
                                }
                            })
                        });
                        if relevant {
                            let _ = wake.send(());
                        }
                    }
                    Err(errors) => {
                        tracing::warn!(count = errors.len(), "file watcher error");
                        let _ = wake.send(());
                    }
                },
            )?;
        for target in watch_targets(roots) {
            let mode = if target.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            debouncer.watch(&target.path, mode)?;
        }
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_root_ignores_unrelated_parent_writes_then_detects_creation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        let plan = watch_targets(std::slice::from_ref(&root));
        assert_eq!(
            plan,
            vec![WatchTarget {
                path: dir.path().to_path_buf(),
                recursive: false
            }]
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher =
            RootWatcher::start(std::slice::from_ref(&root), Duration::from_millis(50), tx).unwrap();
        for n in 0..8 {
            std::fs::write(dir.path().join(format!("unrelated-{n}")), "fiction").unwrap();
        }
        assert!(rx.recv_timeout(Duration::from_millis(250)).is_err());
        std::fs::create_dir(&root).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
        drop(watcher);
        assert_eq!(
            watch_targets(std::slice::from_ref(&root)),
            vec![WatchTarget {
                path: root,
                recursive: true
            }]
        );
    }
}
