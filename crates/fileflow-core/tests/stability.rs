use fileflow_core::stability::{looks_partial, StabilityWatcher};
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn detects_common_partial_suffixes() {
    assert!(looks_partial("big.iso.crdownload"));
    assert!(looks_partial("archive.zip.part"));
    assert!(looks_partial("~$doc.docx"));
    assert!(!looks_partial("archive.zip"));
}

#[test]
fn first_observation_is_never_stable() {
    let mut w = StabilityWatcher::new(Duration::from_secs(1));
    let t0 = Instant::now();
    assert!(!w.observe(Path::new("/downloads/a.zip"), 1024, t0));
}

#[test]
fn stable_after_quiet_period_with_unchanged_size() {
    let mut w = StabilityWatcher::new(Duration::from_millis(50));
    let path = Path::new("/downloads/a.zip");
    let t0 = Instant::now();
    assert!(!w.observe(path, 1024, t0));
    let t1 = t0 + Duration::from_millis(60);
    assert!(w.observe(path, 1024, t1));
}

#[test]
fn size_change_resets_timer() {
    let mut w = StabilityWatcher::new(Duration::from_millis(50));
    let path = Path::new("/downloads/a.zip");
    let t0 = Instant::now();
    w.observe(path, 1024, t0);
    let t1 = t0 + Duration::from_millis(60);
    // Size changed → not stable
    assert!(!w.observe(path, 2048, t1));
    // Still not stable immediately
    assert!(!w.observe(path, 2048, t1 + Duration::from_millis(10)));
    // Stable once the new size holds
    assert!(w.observe(path, 2048, t1 + Duration::from_millis(60)));
}
