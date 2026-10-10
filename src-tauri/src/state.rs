//! Shared runtime state: rules cache, sandbox, journal and store. Everything
//! the Tauri command layer needs lives on `AppState`.

use fileflow_core::db::Db;
use fileflow_core::journal::{Journal, Recovery};
use fileflow_core::model::Rule;
use fileflow_core::safety::{default_forbidden_roots, Sandbox};
use fileflow_core::scheduler::Scheduler;
use fileflow_core::store::Store;
use fileflow_core::watcher::FileWatcher;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AppState {
    pub journal: Journal,
    pub store: Arc<Store>,
    pub rules: Mutex<Vec<Rule>>,
    pub sandbox: Mutex<Sandbox>,
    pub data_dir: PathBuf,
    /// Currently running real-time watcher, if any. Restarting the watcher
    /// (e.g. after a folder is granted) drops the previous handle here.
    pub watcher: Mutex<Option<FileWatcher>>,
    pub scheduler: Mutex<Option<Scheduler>>,
    /// Called after the engine changed something, so the UI can refresh.
    pub on_change: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Operations that were interrupted by a crash and reconciled at start.
    pub recovered: Vec<Recovery>,
    /// Paths FileFlow itself just wrote, so the watcher does not feed them
    /// back into the rules.
    pub recent_outputs: Mutex<HashMap<PathBuf, Instant>>,
    /// Serializes `ff_run` so two batches never interleave.
    pub run_lock: Mutex<()>,
}

impl AppState {
    /// Opens (or creates) the database in `data_dir`, reloads rules and the
    /// granted folders, and reconciles operations interrupted by a crash.
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let db = Arc::new(Db::open(&data_dir.join("fileflow.sqlite"))?);
        Self::from_db(db, data_dir)
    }

    pub fn in_memory_for_test() -> anyhow::Result<Self> {
        let db = Arc::new(Db::open_in_memory()?);
        Self::from_db(db, std::env::temp_dir().join("fileflow-test"))
    }

    fn from_db(db: Arc<Db>, data_dir: PathBuf) -> anyhow::Result<Self> {
        let journal = Journal::new(db.clone());
        let store = Arc::new(Store::new(db));
        let recovered = journal.recover()?;
        let rules = store.list_rules()?;
        let forbidden = default_forbidden_roots(std::slice::from_ref(&data_dir));
        // Folders that were moved or deleted since last time simply drop out.
        let sandbox = Sandbox::new(store.list_folders()?, forbidden);
        Ok(Self {
            journal,
            store,
            rules: Mutex::new(rules),
            sandbox: Mutex::new(sandbox),
            data_dir,
            watcher: Mutex::new(None),
            scheduler: Mutex::new(None),
            on_change: Mutex::new(None),
            recovered,
            recent_outputs: Mutex::new(HashMap::new()),
            run_lock: Mutex::new(()),
        })
    }
}

impl AppState {
    pub fn notify_changed(&self) {
        let cb = self.on_change.lock().unwrap().clone();
        if let Some(cb) = cb {
            cb();
        }
    }
}

impl AppState {
    pub fn remember_output(&self, path: &Path) {
        let mut map = self.recent_outputs.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, at| now.duration_since(*at) < crate::engine::OWN_OUTPUT_GRACE);
        map.insert(path.to_path_buf(), now);
    }

    pub fn is_own_output(&self, path: &Path) -> bool {
        self.recent_outputs
            .lock()
            .unwrap()
            .get(path)
            .is_some_and(|at| at.elapsed() < crate::engine::OWN_OUTPUT_GRACE)
    }
}
