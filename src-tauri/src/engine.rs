//! Background engine: turns watcher events and schedule ticks into planned,
//! validated and journaled operations, using exactly the same path as a
//! manual run (`commands::ff_run`), so every safety check applies.

use crate::commands::{ff_run, ff_scan_folder, RunStatus};
use crate::state::AppState;
use fileflow_core::model::{FileMeta, Rule, Trigger};
use fileflow_core::planner::{active_rules, plan};
use fileflow_core::safety::validate;
use fileflow_core::scheduler::Scheduler;
use fileflow_core::watcher::{FileWatcher, WatcherConfig, WatcherEvent};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

/// Files FileFlow itself wrote are not fed back into the rules for this
/// long (a rename rule would otherwise rename its own output forever).
pub const OWN_OUTPUT_GRACE: Duration = Duration::from_secs(120);

/// Upper bound for one scheduled sweep.
const SCHEDULE_SCAN_LIMIT: usize = 50_000;

/// Starts everything that runs in the background: the scheduler and the
/// real-time watcher (if folders are granted). Safe to call again; previous
/// instances are replaced.
pub fn start(state: &Arc<AppState>) -> anyhow::Result<()> {
    start_scheduler(state);
    start_watcher(state)
}

pub fn start_watcher(state: &Arc<AppState>) -> anyhow::Result<()> {
    let roots = state.sandbox.lock().unwrap().allowed_roots().to_vec();
    let mut slot = state.watcher.lock().unwrap();
    if let Some(prev) = slot.take() {
        prev.shutdown();
    }
    if roots.is_empty() {
        return Ok(());
    }
    let watcher = FileWatcher::start(&roots, WatcherConfig::default())?;
    let events = watcher.events.clone();
    let st = state.clone();
    // Ends by itself when the watcher is replaced or stopped: its pipeline
    // thread exits, which disconnects the channel.
    std::thread::spawn(move || {
        while let Ok(ev) = events.recv() {
            if let WatcherEvent::Ready(path) = ev {
                process_file(&st, &path);
            }
        }
    });
    *slot = Some(watcher);
    Ok(())
}

pub fn stop_watcher(state: &AppState) {
    if let Some(w) = state.watcher.lock().unwrap().take() {
        w.shutdown();
    }
}

pub fn watcher_running(state: &AppState) -> bool {
    state.watcher.lock().unwrap().is_some()
}

fn start_scheduler(state: &Arc<AppState>) {
    let mut slot = state.scheduler.lock().unwrap();
    if let Some(prev) = slot.take() {
        prev.shutdown();
    }
    let rules_state = state.clone();
    let scheduler = Scheduler::start(
        move || rules_state.rules.lock().unwrap().clone(),
        state.store.clone(),
        Duration::from_secs(60),
    );
    let ticks = scheduler.ticks.clone();
    let st = state.clone();
    std::thread::spawn(move || {
        while let Ok(tick) = ticks.recv() {
            process_schedule(&st, &tick.rule_id);
        }
    });
    *slot = Some(scheduler);
}

/// Applies the real-time rules to one file that has stopped changing.
/// Returns the number of operations that were carried out.
pub fn process_file(state: &AppState, path: &Path) -> usize {
    if state.is_own_output(path) {
        return 0;
    }
    let sandbox = state.sandbox.lock().unwrap().clone();
    if !sandbox.contains(path) {
        return 0;
    }
    let Ok(file) = FileMeta::from_path(path) else {
        return 0;
    };
    if file.is_symlink {
        return 0;
    }
    let rules = active_rules(&state.rules.lock().unwrap(), Trigger::RealTime);
    run_plan(state, &rules, &[file])
}

/// Sweeps the granted folders with one scheduled rule.
pub fn process_schedule(state: &AppState, rule_id: &str) -> usize {
    let rule = state
        .rules
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.id == rule_id && r.enabled)
        .cloned();
    let Some(rule) = rule else { return 0 };
    let roots = state.sandbox.lock().unwrap().allowed_roots().to_vec();
    let mut files = Vec::new();
    for root in roots {
        let room = SCHEDULE_SCAN_LIMIT.saturating_sub(files.len());
        if let Ok(found) = ff_scan_folder(&root, room) {
            files.extend(found);
        }
    }
    // A sweep has no stability gate, so in-flight downloads are skipped by name.
    files.retain(|f| {
        !state.is_own_output(&f.path) && !fileflow_core::stability::looks_partial(&f.name)
    });
    run_plan(state, &[rule], &files)
}

fn run_plan(state: &AppState, rules: &[Rule], files: &[FileMeta]) -> usize {
    if rules.is_empty() || files.is_empty() {
        return 0;
    }
    let sandbox = state.sandbox.lock().unwrap().clone();
    let planned = plan(rules, files, OffsetDateTime::now_utc());
    let (ops, _rejected) = validate(&sandbox, planned.ops);
    if ops.is_empty() {
        return 0;
    }
    let done = ff_run(state, ops, false)
        .iter()
        .filter(|o| o.status == RunStatus::Done)
        .count();
    state.notify_changed();
    done
}
