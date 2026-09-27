use fileflow_core::model::{Action, CollisionPolicy, ConditionNode, Rule, RuleContinuation, Trigger};
use fileflow_core::scheduler::{is_due, MemoryLastRunStore};
use std::path::PathBuf;
use time::OffsetDateTime;

fn rule(trigger: Trigger, enabled: bool) -> Rule {
    Rule {
        id: "r".into(),
        name: "r".into(),
        description: String::new(),
        priority: 100,
        enabled,
        profile: None,
        condition: ConditionNode::All { children: vec![] },
        action: Action::Move { dest_dir: PathBuf::from("/x"), rename: None },
        on_conflict: CollisionPolicy::Rename,
        then: RuleContinuation::StopAfterAction,
        sources: vec![],
        trigger,
    }
}

#[test]
fn daily_due_when_never_run() {
    let r = rule(Trigger::Daily, true);
    let store = MemoryLastRunStore::default();
    assert!(is_due(&r, &store, OffsetDateTime::now_utc()));
}

#[test]
fn daily_not_due_within_24h() {
    let r = rule(Trigger::Daily, true);
    let store = MemoryLastRunStore::default();
    use fileflow_core::scheduler::LastRunStore;
    let now = OffsetDateTime::now_utc();
    store.record_run("r", now - time::Duration::hours(5));
    assert!(!is_due(&r, &store, now));
}

#[test]
fn daily_due_after_24h() {
    let r = rule(Trigger::Daily, true);
    let store = MemoryLastRunStore::default();
    use fileflow_core::scheduler::LastRunStore;
    let now = OffsetDateTime::now_utc();
    store.record_run("r", now - time::Duration::hours(25));
    assert!(is_due(&r, &store, now));
}

#[test]
fn weekly_needs_seven_days() {
    let r = rule(Trigger::Weekly, true);
    let store = MemoryLastRunStore::default();
    use fileflow_core::scheduler::LastRunStore;
    let now = OffsetDateTime::now_utc();
    store.record_run("r", now - time::Duration::days(6));
    assert!(!is_due(&r, &store, now));
    store.record_run("r", now - time::Duration::days(8));
    assert!(is_due(&r, &store, now));
}

#[test]
fn realtime_and_manual_never_due() {
    let store = MemoryLastRunStore::default();
    let now = OffsetDateTime::now_utc();
    assert!(!is_due(&rule(Trigger::RealTime, true), &store, now));
    assert!(!is_due(&rule(Trigger::Manual, true), &store, now));
}
