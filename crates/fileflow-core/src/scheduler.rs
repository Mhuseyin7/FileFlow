//! Scheduled trigger runner. Handles [`crate::model::Trigger::Daily`] and
//! [`crate::model::Trigger::Weekly`] rules.
//!
//! v1 is intentionally simple: a background thread ticks once a minute, and
//! for each scheduled rule it consults the persisted "last run" timestamp
//! to decide whether the cadence is due. No cron parser, no seconds
//! precision — the tradeoff is that Daily and Weekly rules run within a
//! minute of their nominal fire time, and never twice on the same day/week.
//!
//! The scheduler itself doesn't touch files — it emits a
//! [`ScheduleTick { rule_id, now }`] and the caller drives the planner.

use crate::model::{Rule, Trigger};
use crossbeam_channel::{unbounded, Receiver};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct ScheduleTick {
    pub rule_id: String,
    pub at: OffsetDateTime,
}

/// Persistent last-run tracker. In production this is backed by a table in
/// the FileFlow SQLite DB; the type here is generic so tests can pass an
/// in-memory HashMap.
pub trait LastRunStore: Send + Sync {
    fn last_run(&self, rule_id: &str) -> Option<OffsetDateTime>;
    fn record_run(&self, rule_id: &str, at: OffsetDateTime);
}

/// In-memory `LastRunStore` for tests.
#[derive(Default)]
pub struct MemoryLastRunStore(Mutex<HashMap<String, OffsetDateTime>>);
impl LastRunStore for MemoryLastRunStore {
    fn last_run(&self, id: &str) -> Option<OffsetDateTime> {
        self.0.lock().unwrap().get(id).copied()
    }
    fn record_run(&self, id: &str, at: OffsetDateTime) {
        self.0.lock().unwrap().insert(id.to_string(), at);
    }
}

pub struct Scheduler {
    stop: Arc<Mutex<bool>>,
    _thread: thread::JoinHandle<()>,
    pub ticks: Receiver<ScheduleTick>,
}

impl Scheduler {
    pub fn start<F, S>(
        get_rules: F,
        store: S,
        tick_every: Duration,
    ) -> Self
    where
        F: Fn() -> Vec<Rule> + Send + 'static,
        S: LastRunStore + 'static,
    {
        let (tx, rx) = unbounded();
        let stop = Arc::new(Mutex::new(false));
        let stop_c = stop.clone();
        let handle = thread::spawn(move || {
            let store = store;
            loop {
                if *stop_c.lock().unwrap() {
                    break;
                }
                let now = OffsetDateTime::now_utc();
                for rule in get_rules() {
                    if !rule.enabled {
                        continue;
                    }
                    if !is_due(&rule, &store, now) {
                        continue;
                    }
                    let _ = tx.send(ScheduleTick { rule_id: rule.id.clone(), at: now });
                    store.record_run(&rule.id, now);
                }
                thread::sleep(tick_every);
            }
        });
        Scheduler {
            stop,
            _thread: handle,
            ticks: rx,
        }
    }

    pub fn shutdown(&self) {
        *self.stop.lock().unwrap() = true;
    }
}

/// Pure decision function — factored out so it can be tested with fake time
/// and a fake store, without spinning up a thread.
pub fn is_due(rule: &Rule, store: &dyn LastRunStore, now: OffsetDateTime) -> bool {
    match rule.trigger {
        Trigger::Daily => match store.last_run(&rule.id) {
            None => true,
            Some(last) => (now - last).whole_hours() >= 24,
        },
        Trigger::Weekly => match store.last_run(&rule.id) {
            None => true,
            Some(last) => (now - last).whole_days() >= 7,
        },
        Trigger::RealTime | Trigger::Manual => false,
    }
}
