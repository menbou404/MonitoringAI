use std::{collections::BTreeSet, path::PathBuf, sync::mpsc::Sender, time::Duration};

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::discovery::nearest_existing;

pub(crate) struct RootWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl RootWatcher {
    pub(crate) fn start(
        roots: &[PathBuf],
        debounce: Duration,
        wake: Sender<()>,
    ) -> notify::Result<Self> {
        let mut debouncer =
            new_debouncer(
                debounce,
                None,
                move |result: DebounceEventResult| match result {
                    Ok(events) if !events.is_empty() => {
                        let _ = wake.send(());
                    }
                    Err(errors) => {
                        tracing::warn!(count = errors.len(), "file watcher error");
                        let _ = wake.send(());
                    }
                    _ => {}
                },
            )?;
        let ancestors = roots
            .iter()
            .filter_map(|root| nearest_existing(root))
            .collect::<BTreeSet<_>>();
        for root in ancestors {
            debouncer.watch(&root, RecursiveMode::Recursive)?;
        }
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}
