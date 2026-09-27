//! Shared runtime state: rules cache, sandbox, journal, DB connection.
//! Everything the Tauri command layer needs lives on `AppState`.

use fileflow_core::journal::Journal;
use fileflow_core::model::Rule;
use fileflow_core::safety::Sandbox;
use fileflow_core::watcher::FileWatcher;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub journal: Mutex<Journal>,
    pub rules: Mutex<Vec<Rule>>,
    pub sandbox: Mutex<Sandbox>,
    pub data_dir: PathBuf,
    /// Currently running real-time watcher, if any. Restarting the watcher
    /// (e.g. after a folder is granted) drops the previous handle here.
    pub watcher: Mutex<Option<FileWatcher>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let journal = Journal::open(&data_dir.join("fileflow.sqlite"))?;
        Ok(Self {
            journal: Mutex::new(journal),
            rules: Mutex::new(Vec::new()),
            sandbox: Mutex::new(Sandbox::default()),
            data_dir,
            watcher: Mutex::new(None),
        })
    }

    pub fn in_memory_for_test() -> anyhow::Result<Self> {
        Ok(Self {
            journal: Mutex::new(Journal::open_in_memory()?),
            rules: Mutex::new(Vec::new()),
            sandbox: Mutex::new(Sandbox::default()),
            data_dir: std::env::temp_dir().join("fileflow-test"),
            watcher: Mutex::new(None),
        })
    }
}
