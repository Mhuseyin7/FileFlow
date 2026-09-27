//! Real-time file watcher. Wraps `notify` v6 with:
//!
//! * a **coalescing debouncer** so a burst of rename+write+chmod events for
//!   the same path collapses into a single downstream event;
//! * a **stability gate** so a file must stop changing for `min_quiet` before
//!   FileFlow acts on it (see [`crate::stability::StabilityWatcher`]);
//! * a **partial-download filter** so `.crdownload`/`.part`/etc. never
//!   escape the watcher.
//!
//! The pipeline is: notify raw event → normalize → debounce → stability
//! poll → [`WatcherEvent::Ready`].

use crate::error::{Error, Result};
use crate::stability::{looks_partial, StabilityWatcher};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use notify::event::{CreateKind, EventKind, ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Event that leaves the watcher pipeline. Only ready-to-act events reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherEvent {
    Ready(PathBuf),
    Removed(PathBuf),
}

/// Configuration.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// A file must be unchanged for this long before it is considered stable
    /// and a Ready event is emitted.
    pub min_quiet: Duration,
    /// Debounce window: raw notify events for the same path collapse into
    /// one candidate observation within this window.
    pub debounce: Duration,
    /// How often the background loop polls for stability. Small enough to
    /// keep min_quiet responsive; large enough to be cheap.
    pub poll_interval: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            min_quiet: Duration::from_secs(5),
            debounce: Duration::from_millis(250),
            poll_interval: Duration::from_millis(500),
        }
    }
}

/// A running watcher. Drop to stop.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    _thread: thread::JoinHandle<()>,
    stop: Arc<Mutex<bool>>,
    /// Consumer end for callers; producer side is owned by the pipeline
    /// thread and never leaves this module.
    pub events: Receiver<WatcherEvent>,
}

impl FileWatcher {
    /// Start watching the given roots recursively. Returns immediately.
    pub fn start(roots: &[PathBuf], cfg: WatcherConfig) -> Result<Self> {
        // Raw notify → pipeline channel (bounded so a runaway file storm
        // applies back-pressure instead of exhausting memory).
        let (raw_tx, raw_rx) = bounded::<RawSignal>(4096);
        let (out_tx, out_rx) = unbounded::<WatcherEvent>();

        let handler = {
            let tx = raw_tx.clone();
            move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    for path in ev.paths.iter().cloned() {
                        let sig = classify(&ev.kind, path);
                        if let Some(s) = sig {
                            let _ = tx.try_send(s);
                        }
                    }
                }
            }
        };
        let mut watcher = notify::recommended_watcher(handler)
            .map_err(|e| Error::Other(format!("notify: {e}")))?;
        for r in roots {
            watcher
                .watch(r, RecursiveMode::Recursive)
                .map_err(|e| Error::Other(format!("notify watch {}: {e}", r.display())))?;
        }

        let stop = Arc::new(Mutex::new(false));
        let handle = spawn_pipeline(raw_rx, out_tx, cfg, stop.clone());

        Ok(FileWatcher {
            _watcher: watcher,
            _thread: handle,
            stop,
            events: out_rx,
        })
    }

    /// Ask the pipeline thread to stop. Drop still cleans up; this is here
    /// so callers can join deterministically in tests.
    pub fn shutdown(&self) {
        *self.stop.lock().unwrap() = true;
    }
}

// -------------------------------------------------------------------------
// internals
// -------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum RawSignal {
    Touched(PathBuf),
    Removed(PathBuf),
}

fn classify(kind: &EventKind, path: PathBuf) -> Option<RawSignal> {
    match kind {
        EventKind::Create(CreateKind::File | CreateKind::Any) => Some(RawSignal::Touched(path)),
        EventKind::Modify(ModifyKind::Data(_)) => Some(RawSignal::Touched(path)),
        EventKind::Modify(ModifyKind::Name(RenameMode::To | RenameMode::Any | RenameMode::Both)) => {
            Some(RawSignal::Touched(path))
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => Some(RawSignal::Removed(path)),
        EventKind::Remove(_) => Some(RawSignal::Removed(path)),
        _ => None,
    }
}

struct PendingEntry {
    /// Last time we saw *any* signal for this path.
    last_signal: Instant,
    /// Marker so we can drop Ready emission after the file goes away.
    removed: bool,
}

fn spawn_pipeline(
    raw: Receiver<RawSignal>,
    out: Sender<WatcherEvent>,
    cfg: WatcherConfig,
    stop: Arc<Mutex<bool>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut pending: HashMap<PathBuf, PendingEntry> = HashMap::new();
        let mut stability = StabilityWatcher::new(cfg.min_quiet);
        loop {
            if *stop.lock().unwrap() {
                break;
            }
            // Drain raw events for up to `poll_interval`; then evaluate.
            let deadline = Instant::now() + cfg.poll_interval;
            while let Ok(sig) = raw.recv_deadline(deadline) {
                match sig {
                    RawSignal::Touched(p) => {
                        if is_ignored(&p) {
                            continue;
                        }
                        pending.entry(p).or_insert_with(|| PendingEntry {
                            last_signal: Instant::now(),
                            removed: false,
                        }).last_signal = Instant::now();
                    }
                    RawSignal::Removed(p) => {
                        if let Some(e) = pending.get_mut(&p) {
                            e.removed = true;
                        }
                        stability.forget(&p);
                        let _ = out.send(WatcherEvent::Removed(p));
                    }
                }
            }

            let now = Instant::now();
            let mut to_remove = Vec::new();
            for (path, entry) in pending.iter() {
                if entry.removed {
                    to_remove.push(path.clone());
                    continue;
                }
                if now.duration_since(entry.last_signal) < cfg.debounce {
                    continue;
                }
                match std::fs::metadata(path) {
                    Ok(md) if md.is_file() => {
                        if stability.observe(path, md.len(), now) {
                            let _ = out.send(WatcherEvent::Ready(path.clone()));
                            to_remove.push(path.clone());
                        }
                    }
                    _ => {
                        // File is gone; drop from pending.
                        to_remove.push(path.clone());
                    }
                }
            }
            for p in to_remove {
                pending.remove(&p);
                stability.forget(&p);
            }
        }
    })
}

fn is_ignored(p: &Path) -> bool {
    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
        if looks_partial(name) {
            return true;
        }
        // FileFlow's own local trash — never re-enter the pipeline.
        if name == ".fileflow-trash" {
            return true;
        }
    }
    p.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new(".fileflow-trash"))
}
