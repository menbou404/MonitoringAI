use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use crate::{
    Clock, Health, JobDetail, JobSummary, LogChunk, MonitorConfig, MonitorEvent, ProcessProbe,
    SystemClock, SystemProcessProbe,
    discovery::state_files,
    history::History,
    log_tail::{LogTail, latest_message, modified_at},
    parser::{read_installed_version, read_job, read_state},
    store::{Store, safe_id},
    watcher::{RootWatcher, watch_targets},
};

struct ContextRequest {
    job: crate::Job,
    directory: PathBuf,
    projects: PathBuf,
    previous: Option<crate::JobContext>,
}

enum Command {
    Stop,
    Update(Box<MonitorConfig>),
    Subscribe(String),
    Unsubscribe(String),
}

pub struct Monitor {
    store: Arc<Mutex<Store>>,
    health: Arc<Mutex<Health>>,
    details: Arc<Mutex<BTreeMap<String, crate::Job>>>,
    history: Arc<Mutex<Option<History>>>,
    initial_log_bytes: AtomicU64,
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
        let initial_log_bytes = AtomicU64::new(config.initial_log_bytes);
        let store = Arc::new(Mutex::new(Store::new()));
        let health = Arc::new(Mutex::new(Health {
            plugin_version: None,
            version_supported: false,
            watched_roots: Vec::new(),
            last_error: None,
        }));
        let details = Arc::new(Mutex::new(BTreeMap::new()));
        let history = Arc::new(Mutex::new(
            config
                .history_dir
                .clone()
                .map(|dir| History::open(dir, config.history_days, clock.now())),
        ));
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let worker_store = Arc::clone(&store);
        let worker_health = Arc::clone(&health);
        let worker_history = Arc::clone(&history);
        let thread = thread::spawn(move || {
            Worker::new(
                config,
                worker_store,
                worker_health,
                worker_history,
                event_tx,
                probe,
                clock,
            )
            .run(command_rx);
        });
        (
            Self {
                store,
                health,
                details,
                history,
                initial_log_bytes,
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
                store.path_for(id).map(PathBuf::from),
            )
        };
        let Some(directory) = directory else {
            return self
                .history
                .lock()
                .expect("history lock")
                .as_ref()?
                .get(id)
                .map(|r| {
                    let mut detail = r.detail();
                    detail.summary = summary;
                    detail
                });
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

    pub fn read_log(&self, id: &str, from: Option<u64>) -> std::io::Result<Option<LogChunk>> {
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
        let start = match from {
            Some(from) => from,
            None => match std::fs::metadata(&path) {
                Ok(metadata) => metadata
                    .len()
                    .saturating_sub(self.initial_log_bytes.load(Ordering::Relaxed)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            },
        };
        match crate::read_log_chunk(&path, start) {
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
        self.initial_log_bytes
            .store(config.initial_log_bytes, Ordering::Relaxed);
        let _ = self.command.send(Command::Update(Box::new(config)));
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
    history: Arc<Mutex<Option<History>>>,
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
        history: Arc<Mutex<Option<History>>>,
        events: Sender<MonitorEvent>,
        probe: Arc<dyn ProcessProbe>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            store,
            health,
            history,
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
        let (context_tx, context_rx) = mpsc::channel::<ContextRequest>();
        let (result_tx, result_rx) = mpsc::channel();
        let context_thread = thread::spawn(move || {
            let mut resolver = crate::context::Resolver::default();
            while let Ok(request) = context_rx.recv() {
                let id = request.job.id.clone();
                let result = if let Some(mut previous) =
                    request.previous.filter(|c| c.requester.is_some())
                {
                    previous.session_title = resolver
                        .refresh_title(&request.projects, &previous.session_id)
                        .or(previous.session_title);
                    Some(previous)
                } else {
                    let prompt = if safe_id(&id) {
                        read_job(&request.directory.join("jobs").join(format!("{id}.json")))
                            .ok()
                            .and_then(|j| j.request.and_then(|r| r.prompt).or(j.summary))
                    } else {
                        None
                    };
                    let prompt = prompt.or_else(|| {
                        request
                            .job
                            .request
                            .as_ref()
                            .and_then(|r| r.prompt.clone())
                            .or(request.job.summary.clone())
                    });
                    resolver.resolve(&request.projects, &request.job, prompt.as_deref())
                };
                if let Some(context) = result {
                    let _ = result_tx.send((id, context));
                }
            }
        });
        let mut pending = BTreeSet::new();
        let mut attempted = BTreeSet::new();
        let (wake_tx, wake_rx) = mpsc::channel();
        let mut targets = watch_targets(&self.config.roots());
        let mut watcher = self.watch(wake_tx.clone());
        self.scan();
        self.queue_context(&context_tx, &mut pending, &mut attempted);
        let mut rescan = Instant::now();
        let mut liveness = Instant::now();
        loop {
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Command::Update(config)) => {
                    if self.config.history_dir != config.history_dir
                        || self.config.history_days != config.history_days
                    {
                        *self.history.lock().expect("history lock") = config
                            .history_dir
                            .clone()
                            .map(|dir| History::open(dir, config.history_days, self.clock.now()));
                    }
                    self.config = *config;
                    self.stamps.clear();
                    targets = watch_targets(&self.config.roots());
                    watcher = self.watch(wake_tx.clone());
                    self.scan();
                    self.queue_context(&context_tx, &mut pending, &mut attempted);
                    rescan = Instant::now();
                    liveness = Instant::now();
                }
                Ok(Command::Subscribe(id)) => {
                    self.subscribed.insert(id);
                    self.scan();
                    self.queue_context(&context_tx, &mut pending, &mut attempted);
                }
                Ok(Command::Unsubscribe(id)) => {
                    self.subscribed.remove(&id);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let mut woke = false;
            let mut context_changed = false;
            while let Ok((id, context)) = result_rx.try_recv() {
                pending.remove(&id);
                let mut store = self.store.lock().expect("store lock");
                if store
                    .summary(&id)
                    .is_some_and(|s| s.context.as_ref() != Some(&context))
                {
                    if store.summary(&id).is_some_and(|s| s.archived) {
                        drop(store);
                        if let Some(history) = self.history.lock().expect("history lock").as_mut() {
                            history.set_context(&id, context);
                            history.flush();
                        }
                    } else {
                        store.set_context(&id, context);
                    }
                    context_changed = true;
                }
            }
            if context_changed {
                self.reconcile();
            }
            while wake_rx.try_recv().is_ok() {
                woke = true;
            }
            if woke || rescan.elapsed() >= self.config.rescan_interval {
                self.scan();
                self.queue_context(&context_tx, &mut pending, &mut attempted);
                rescan = Instant::now();
                let current_targets = watch_targets(&self.config.roots());
                if current_targets != targets || watcher.is_none() {
                    watcher = self.watch(wake_tx.clone());
                    targets = current_targets;
                }
            } else if liveness.elapsed() >= self.config.liveness_interval {
                self.reconcile();
                liveness = Instant::now();
            }
        }
        drop(watcher);
        drop(context_tx);
        let _ = context_thread.join();
    }

    fn queue_context(
        &self,
        tx: &Sender<ContextRequest>,
        pending: &mut BTreeSet<String>,
        attempted: &mut BTreeSet<String>,
    ) {
        let Some(projects) = &self.config.claude_projects_dir else {
            return;
        };
        let jobs = self.store.lock().expect("store lock").context_jobs();
        let live: BTreeSet<_> = jobs.iter().map(|(job, _, _)| job.id.clone()).collect();
        attempted.retain(|id| live.contains(id));
        for (job, directory, previous) in jobs {
            if pending.contains(&job.id) {
                continue;
            }
            if attempted.contains(&job.id)
                && previous.as_ref().is_none_or(|c| c.requester.is_none())
                && matches!(job.status.as_str(), "completed" | "failed" | "cancelled")
                && job
                    .completed_at
                    .or(job.updated_at)
                    .or(job.created_at)
                    .is_some_and(|at| self.clock.now() - at > chrono::Duration::minutes(10))
            {
                continue;
            }
            attempted.insert(job.id.clone());
            pending.insert(job.id.clone());
            let _ = tx.send(ContextRequest {
                job,
                directory,
                projects: projects.clone(),
                previous,
            });
        }
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

    fn scan(&mut self) {
        let roots = self.config.roots();
        let found = state_files(&roots);
        let mut error = None;
        let mut updates = Vec::new();
        self.stamps.retain(|path, _| found.contains(path));
        for path in &found {
            let Ok(metadata) = std::fs::metadata(path) else {
                continue;
            };
            let stamp = (metadata.modified().ok(), metadata.len());
            if self.stamps.get(path) != Some(&stamp) {
                match read_state(path) {
                    Ok(jobs) => {
                        updates.push((path.clone(), jobs));
                        self.stamps.insert(path.clone(), stamp);
                    }
                    Err(_) => {
                        error = Some("State file read failed".to_owned());
                        tracing::warn!("state file read failed");
                    }
                }
            }
        }
        let candidates = {
            let mut store = self.store.lock().expect("store lock");
            store.retain_sources(&found);
            for (path, jobs) in updates {
                store.replace_source(path, jobs);
            }
            store.log_candidates()
        };
        self.latest
            .retain(|id, _| candidates.iter().any(|(candidate, _, _)| candidate == id));
        self.log_context
            .retain(|id, _| candidates.iter().any(|(candidate, _, _)| candidate == id));
        self.tail
            .retain_paths(&candidates.iter().map(|(_, _, path)| path.clone()).collect());
        let mut log_updates = Vec::new();
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
            log_updates.push((
                id.clone(),
                self.latest.get(&id).cloned(),
                modified_at(&path),
            ));
        }
        {
            let mut store = self.store.lock().expect("store lock");
            for (id, latest, modified) in log_updates {
                store.set_log(&id, latest, modified);
            }
        }
        self.reconcile();
        self.update_health(&roots, error);
    }

    fn reconcile(&self) {
        let candidates = self.store.lock().expect("store lock").detail_candidates();
        let details: BTreeMap<_, _> = candidates
            .into_iter()
            .filter_map(|(id, path)| {
                read_job(&path)
                    .ok()
                    .filter(|job| job.id == id)
                    .map(|job| (id, job))
            })
            .collect();
        let mut history = self.history.lock().expect("history lock");
        let events = self.store.lock().expect("store lock").reconcile(
            self.probe.as_ref(),
            self.clock.as_ref(),
            self.config.quiet_after,
            history.as_mut(),
            &details,
        );
        if let Some(history) = history.as_mut() {
            history.flush();
        }
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
