use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use crate::{
    Clock, Health, JobDetail, JobSummary, LogChunk, MonitorConfig, MonitorEvent, ProcessProbe,
    SystemClock, SystemProcessProbe,
    discovery::state_files,
    log_tail::{LogTail, latest_message, modified_at},
    parser::{read_installed_version, read_job, read_state},
    store::{Store, safe_id},
    watcher::RootWatcher,
};

enum Command {
    Stop,
    Update(MonitorConfig),
    Subscribe(String),
    Unsubscribe(String),
}

pub struct Monitor {
    store: Arc<Mutex<Store>>,
    health: Arc<Mutex<Health>>,
    details: Arc<Mutex<BTreeMap<String, crate::Job>>>,
    command: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    pub fn start(config: MonitorConfig) -> (Self, Receiver<MonitorEvent>) {
        Self::start_with(config, Arc::new(SystemProcessProbe), Arc::new(SystemClock))
    }

    /// Probe and clock are replaceable for deterministic tests.
    pub fn start_with(
        config: MonitorConfig,
        probe: Arc<dyn ProcessProbe>,
        clock: Arc<dyn Clock>,
    ) -> (Self, Receiver<MonitorEvent>) {
        let store = Arc::new(Mutex::new(Store::new()));
        let health = Arc::new(Mutex::new(Health {
            plugin_version: None,
            version_supported: false,
            watched_roots: Vec::new(),
            last_error: None,
        }));
        let details = Arc::new(Mutex::new(BTreeMap::new()));
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let worker_store = Arc::clone(&store);
        let worker_health = Arc::clone(&health);
        let thread = thread::spawn(move || {
            Worker::new(config, worker_store, worker_health, event_tx, probe, clock)
                .run(command_rx);
        });
        (
            Self {
                store,
                health,
                details,
                command: command_tx,
                thread: Some(thread),
            },
            event_rx,
        )
    }

    pub fn snapshot(&self) -> Vec<JobSummary> {
        self.store.lock().expect("store lock").snapshot()
    }

    pub fn job_detail(&self, id: &str) -> Option<JobDetail> {
        if !safe_id(id) {
            return None;
        }
        let (summary, directory) = {
            let store = self.store.lock().expect("store lock");
            (
                store.summary(id)?.clone(),
                store.path_for(id)?.to_path_buf(),
            )
        };
        let path = directory.join("jobs").join(format!("{id}.json"));
        let job = match read_job(&path) {
            Ok(job) if job.id == id => {
                self.details
                    .lock()
                    .expect("detail lock")
                    .insert(id.to_owned(), job.clone());
                Some(job)
            }
            _ => self.details.lock().expect("detail lock").get(id).cloned(),
        };
        Some(JobDetail {
            prompt: job.as_ref().and_then(|j| {
                j.request
                    .as_ref()
                    .and_then(|r| r.prompt.clone())
                    .or_else(|| j.summary.clone())
            }),
            turn_id: job.as_ref().and_then(|j| j.turn_id.clone()),
            model: job
                .as_ref()
                .and_then(|j| j.request.as_ref().and_then(|r| r.model.clone())),
            effort: job
                .as_ref()
                .and_then(|j| j.request.as_ref().and_then(|r| r.effort.clone())),
            rendered: job.as_ref().and_then(|j| j.rendered.clone()),
            error_message: job.as_ref().and_then(|j| j.error_message.clone()),
            log_file: job.as_ref().and_then(|j| j.log_file.clone()),
            summary,
        })
    }

    pub fn read_log(&self, id: &str, from: u64) -> std::io::Result<Option<LogChunk>> {
        if !safe_id(id) {
            return Ok(None);
        }
        let directory = self
            .store
            .lock()
            .expect("store lock")
            .path_for(id)
            .map(PathBuf::from);
        let Some(directory) = directory else {
            return Ok(None);
        };
        let path = directory.join("jobs").join(format!("{id}.log"));
        match crate::read_log_chunk(&path, from) {
            Ok(chunk) => Ok(Some(chunk)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn subscribe_log(&self, id: &str) {
        if safe_id(id) {
            let _ = self.command.send(Command::Subscribe(id.to_owned()));
        }
    }
    pub fn unsubscribe_log(&self, id: &str) {
        let _ = self.command.send(Command::Unsubscribe(id.to_owned()));
    }
    pub fn health(&self) -> Health {
        self.health.lock().expect("health lock").clone()
    }
    pub fn update_config(&self, config: MonitorConfig) {
        let _ = self.command.send(Command::Update(config));
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.command.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    config: MonitorConfig,
    store: Arc<Mutex<Store>>,
    health: Arc<Mutex<Health>>,
    events: Sender<MonitorEvent>,
    probe: Arc<dyn ProcessProbe>,
    clock: Arc<dyn Clock>,
    stamps: BTreeMap<PathBuf, (Option<SystemTime>, u64)>,
    tail: LogTail,
    latest: BTreeMap<String, String>,
    log_context: BTreeMap<String, String>,
    subscribed: BTreeSet<String>,
    watcher_error: Option<String>,
    last_version: Option<String>,
}

impl Worker {
    fn new(
        config: MonitorConfig,
        store: Arc<Mutex<Store>>,
        health: Arc<Mutex<Health>>,
        events: Sender<MonitorEvent>,
        probe: Arc<dyn ProcessProbe>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            store,
            health,
            events,
            probe,
            clock,
            stamps: BTreeMap::new(),
            tail: LogTail::default(),
            latest: BTreeMap::new(),
            log_context: BTreeMap::new(),
            subscribed: BTreeSet::new(),
            watcher_error: None,
            last_version: None,
        }
    }

    fn run(mut self, commands: Receiver<Command>) {
        let (wake_tx, wake_rx) = mpsc::channel();
        let mut watcher = self.watch(wake_tx.clone());
        self.scan(true);
        let mut rescan = Instant::now();
        let mut liveness = Instant::now();
        loop {
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Command::Update(config)) => {
                    self.config = config;
                    self.stamps.clear();
                    watcher = self.watch(wake_tx.clone());
                    self.scan(true);
                    rescan = Instant::now();
                    liveness = Instant::now();
                }
                Ok(Command::Subscribe(id)) => {
                    self.subscribed.insert(id);
                    self.scan(false);
                }
                Ok(Command::Unsubscribe(id)) => {
                    self.subscribed.remove(&id);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let woke = wake_rx.try_recv().is_ok();
            if woke || rescan.elapsed() >= self.config.rescan_interval {
                self.scan(woke);
                rescan = Instant::now();
            } else if liveness.elapsed() >= self.config.liveness_interval {
                self.reconcile();
                liveness = Instant::now();
            }
        }
        drop(watcher);
    }

    fn watch(&mut self, wake: Sender<()>) -> Option<RootWatcher> {
        match RootWatcher::start(&self.config.roots(), self.config.debounce, wake) {
            Ok(watcher) => {
                self.watcher_error = None;
                Some(watcher)
            }
            Err(_) => {
                tracing::warn!("watcher setup failed; periodic scans remain active");
                self.watcher_error = Some("File watcher setup failed".to_owned());
                None
            }
        }
    }

    fn scan(&mut self, force: bool) {
        let roots = self.config.roots();
        let found = state_files(&roots);
        let mut error = None;
        {
            let mut store = self.store.lock().expect("store lock");
            store.retain_sources(&found);
            self.stamps.retain(|path, _| found.contains(path));
            for path in &found {
                let Ok(metadata) = std::fs::metadata(path) else {
                    continue;
                };
                let stamp = (metadata.modified().ok(), metadata.len());
                if force || self.stamps.get(path) != Some(&stamp) {
                    match read_state(path) {
                        Ok(jobs) => {
                            store.replace_source(path.clone(), jobs);
                            self.stamps.insert(path.clone(), stamp);
                        }
                        Err(_) => {
                            error = Some("State file read failed".to_owned());
                            tracing::warn!("state file read failed");
                        }
                    }
                }
            }
            let candidates = store.log_candidates();
            self.latest
                .retain(|id, _| candidates.iter().any(|(candidate, _, _)| candidate == id));
            self.log_context
                .retain(|id, _| candidates.iter().any(|(candidate, _, _)| candidate == id));
            self.tail
                .retain_paths(&candidates.iter().map(|(_, _, path)| path.clone()).collect());
            for (id, status, path) in candidates {
                if status != "running" && !self.subscribed.contains(&id) {
                    continue;
                }
                match self.tail.poll(&path, self.config.initial_log_bytes) {
                    Ok(Some(chunk)) => {
                        if chunk.start == 0 {
                            self.latest.remove(&id);
                            self.log_context.remove(&id);
                        }
                        let context = self.log_context.entry(id.clone()).or_default();
                        context.push_str(&chunk.text);
                        if context.len() > 4096 {
                            let mut trim = context.len() - 4096;
                            while !context.is_char_boundary(trim) {
                                trim += 1;
                            }
                            context.drain(..trim);
                        }
                        if let Some(message) = latest_message(context) {
                            self.latest.insert(id.clone(), message);
                        }
                        if self.subscribed.contains(&id) {
                            let _ = self.events.send(MonitorEvent::LogAppended {
                                id: id.clone(),
                                text: chunk.text,
                                start: chunk.start,
                                end: chunk.end,
                            });
                        }
                    }
                    Ok(None) => {
                        if !path.exists() || std::fs::metadata(&path).is_ok_and(|m| m.len() == 0) {
                            self.latest.remove(&id);
                            self.log_context.remove(&id);
                        }
                    }
                    Err(_) => {
                        error = Some("Log file read failed".to_owned());
                        tracing::warn!("log file read failed");
                    }
                }
                store.set_log(&id, self.latest.get(&id).cloned(), modified_at(&path));
            }
        }
        self.reconcile();
        self.update_health(&roots, error);
    }

    fn reconcile(&self) {
        let events = self.store.lock().expect("store lock").reconcile(
            self.probe.as_ref(),
            self.clock.as_ref(),
            self.config.quiet_after,
        );
        for event in events {
            let _ = self.events.send(event);
        }
    }

    fn update_health(&mut self, roots: &[PathBuf], error: Option<String>) {
        let mut error = error.or_else(|| self.watcher_error.clone());
        let version = match read_installed_version(&self.config.installed_plugins_path) {
            Ok(version) => {
                self.last_version = version.clone();
                version
            }
            Err(crate::ParseError::Io(ref io)) if io.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => {
                tracing::warn!("plugin version read failed");
                error.get_or_insert_with(|| "Plugin version read failed".to_owned());
                self.last_version.clone()
            }
        };
        let next = Health {
            version_supported: version.as_ref().is_some_and(|v| {
                self.config
                    .supported_plugin_versions
                    .iter()
                    .any(|prefix| v.starts_with(prefix))
            }),
            plugin_version: version,
            watched_roots: roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            last_error: error.or_else(|| {
                roots
                    .iter()
                    .all(|root| !root.exists())
                    .then(|| "No state roots found".to_owned())
            }),
        };
        let mut current = self.health.lock().expect("health lock");
        if *current != next {
            *current = next.clone();
            let _ = self.events.send(MonitorEvent::HealthChanged(next));
        }
    }
}
