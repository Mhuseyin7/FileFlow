//! Integration test for the real-time watcher. Uses very short quiet /
//! debounce windows so the test runs in under a second, but the pipeline
//! goes end-to-end through the real `notify` crate.

use fileflow_core::watcher::{FileWatcher, WatcherConfig, WatcherEvent};
use std::fs;
use std::io::Write;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[test]
fn watcher_emits_ready_after_stability() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let cfg = WatcherConfig {
        min_quiet: Duration::from_millis(150),
        debounce: Duration::from_millis(30),
        poll_interval: Duration::from_millis(60),
    };
    let w = FileWatcher::start(&[dir.path().to_path_buf()], cfg)?;

    let target = dir.path().join("hello.txt");
    let mut f = fs::File::create(&target)?;
    f.write_all(b"hi")?;
    drop(f);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_ready = false;
    while Instant::now() < deadline {
        match w.events.recv_timeout(Duration::from_millis(100)) {
            Ok(WatcherEvent::Ready(p)) if p == target => {
                saw_ready = true;
                break;
            }
            _ => {}
        }
    }
    w.shutdown();
    assert!(saw_ready, "watcher did not emit Ready for the created file");
    Ok(())
}

#[test]
fn watcher_ignores_partial_downloads() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let cfg = WatcherConfig {
        min_quiet: Duration::from_millis(80),
        debounce: Duration::from_millis(20),
        poll_interval: Duration::from_millis(40),
    };
    let w = FileWatcher::start(&[dir.path().to_path_buf()], cfg)?;

    let partial = dir.path().join("archive.zip.crdownload");
    fs::write(&partial, b"x")?;

    // Give the pipeline plenty of time; the partial suffix must be filtered.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if let Ok(WatcherEvent::Ready(p)) = w.events.recv_timeout(Duration::from_millis(50)) {
            if p == partial {
                panic!("partial download should not have produced a Ready event");
            }
        }
    }
    w.shutdown();
    Ok(())
}
