//! File stability detection. Prevents FileFlow from acting on a file that is
//! still being written by the browser or another process.
//!
//! A file is considered *stable* when:
//!
//! 1. Its filename doesn't match a known partial-download pattern, and
//! 2. Its size has been unchanged for at least `min_quiet` since the last
//!    observation.
//!
//! The scanner records `(size, modified)` pairs and asks this module whether
//! it may proceed.

use std::path::Path;
use std::time::{Duration, Instant};

/// Filename suffixes browsers and downloaders use for in-flight files.
const PARTIAL_SUFFIXES: &[&str] = &[
    ".crdownload",
    ".part",
    ".partial",
    ".download",
    ".!ut",
    ".!qb",
    ".opdownload",
];

pub fn looks_partial(name: &str) -> bool {
    let lower = name.to_lowercase();
    PARTIAL_SUFFIXES.iter().any(|s| lower.ends_with(s)) || lower.starts_with("~$")
}

#[derive(Debug, Clone)]
pub struct StabilityWatcher {
    min_quiet: Duration,
    last_seen: std::collections::HashMap<std::path::PathBuf, Observation>,
}

#[derive(Debug, Clone, Copy)]
struct Observation {
    size: u64,
    at: Instant,
}

impl StabilityWatcher {
    pub fn new(min_quiet: Duration) -> Self {
        Self {
            min_quiet,
            last_seen: Default::default(),
        }
    }

    /// Observe a file's current size and return `true` if it appears stable.
    /// First observation is never stable; the watcher needs at least two
    /// samples across `min_quiet`.
    pub fn observe(&mut self, path: &Path, size: u64, now: Instant) -> bool {
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if looks_partial(name) {
                return false;
            }
        }
        let entry = self
            .last_seen
            .entry(path.to_path_buf())
            .or_insert(Observation { size, at: now });
        if entry.size != size {
            entry.size = size;
            entry.at = now;
            return false;
        }
        now.duration_since(entry.at) >= self.min_quiet
    }

    pub fn forget(&mut self, path: &Path) {
        self.last_seen.remove(path);
    }
}
