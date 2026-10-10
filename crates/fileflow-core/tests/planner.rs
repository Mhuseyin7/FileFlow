mod common;

use common::synth;
use fileflow_core::model::{
    Action, CollisionPolicy, Condition, ConditionNode, OpKind, Rule, RuleContinuation, Trigger,
};
use fileflow_core::planner::{active_rules, detect_overlaps, plan, summarize};
use std::path::PathBuf;
use time::OffsetDateTime;

fn ext_rule(name: &str, ext: &str, dest: &str, prio: i32, then: RuleContinuation) -> Rule {
    Rule {
        id: format!("r-{name}"),
        name: name.into(),
        description: String::new(),
        priority: prio,
        enabled: true,
        profile: None,
        condition: ConditionNode::Leaf {
            condition: Condition::Extension {
                any_of: vec![ext.into()],
            },
        },
        action: Action::Move {
            dest_dir: PathBuf::from(dest),
            rename: None,
        },
        on_conflict: CollisionPolicy::Rename,
        then,
        sources: vec![],
        trigger: Trigger::RealTime,
    }
}

fn stop(name: &str, ext: &str, dest: &str, prio: i32) -> Rule {
    ext_rule(name, ext, dest, prio, RuleContinuation::StopAfterAction)
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

#[test]
fn plan_returns_move_op_for_matching_file() {
    let rules = vec![stop("pdfs", "pdf", "/docs/PDF", 10)];
    let files = vec![synth("a.pdf", "/downloads", 100, 1)];
    let ops = plan(&rules, &files, now()).ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, OpKind::Move);
    assert_eq!(
        ops[0].destination.as_ref().unwrap(),
        &PathBuf::from("/docs/PDF/a.pdf")
    );
    assert_eq!(ops[0].rule_name, "pdfs");
}

#[test]
fn disabled_rules_can_still_be_previewed() {
    let mut r = stop("pdfs", "pdf", "/docs/PDF", 10);
    r.enabled = false;
    let files = vec![synth("a.pdf", "/downloads", 100, 1)];
    assert_eq!(plan(&[r], &files, now()).ops.len(), 1);
}

#[test]
fn active_rules_filters_on_enabled_and_trigger() {
    let on = stop("on", "pdf", "/a", 1);
    let mut off = stop("off", "pdf", "/b", 2);
    off.enabled = false;
    let mut weekly = stop("weekly", "pdf", "/c", 3);
    weekly.trigger = Trigger::Weekly;
    let all = [on, off, weekly];
    let real_time = active_rules(&all, Trigger::RealTime);
    assert_eq!(real_time.len(), 1);
    assert_eq!(real_time[0].name, "on");
    assert_eq!(active_rules(&all, Trigger::Weekly).len(), 1);
}

#[test]
fn rules_run_in_priority_order_regardless_of_slice_order() {
    let rules = vec![
        stop("late", "pdf", "/late", 20),
        stop("early", "pdf", "/early", 10),
    ];
    let files = vec![synth("x.pdf", "/downloads", 1, 1)];
    let ops = plan(&rules, &files, now()).ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].rule_name, "early");
}

#[test]
fn continue_after_copy_lets_the_next_rule_act() {
    let mut copy = ext_rule("copy", "pdf", "/backup", 10, RuleContinuation::Continue);
    copy.action = Action::Copy {
        dest_dir: PathBuf::from("/backup"),
        rename: None,
    };
    let mv = stop("move", "pdf", "/docs", 20);
    let files = vec![synth("x.pdf", "/downloads", 1, 1)];
    let ops = plan(&[copy, mv], &files, now()).ops;
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].kind, OpKind::Copy);
    assert_eq!(ops[1].kind, OpKind::Move);
}

#[test]
fn a_move_ends_the_chain_even_with_continue() {
    // The second op would be handed a path that no longer exists.
    let first = ext_rule("A", "pdf", "/A", 10, RuleContinuation::Continue);
    let second = stop("B", "pdf", "/B", 20);
    let files = vec![synth("x.pdf", "/downloads", 1, 1)];
    let ops = plan(&[first, second], &files, now()).ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].rule_name, "A");
}

#[test]
fn stop_after_action_prevents_second_rule() {
    let rules = vec![stop("A", "pdf", "/A", 10), stop("B", "pdf", "/B", 20)];
    let files = vec![synth("x.pdf", "/downloads", 1, 1)];
    let ops = plan(&rules, &files, now()).ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].rule_name, "A");
}

#[test]
fn sources_filter_restricts_rule() {
    let mut r = stop("pdfs", "pdf", "/docs/PDF", 10);
    r.sources = vec![PathBuf::from("/downloads")];
    let outside = synth("a.pdf", "/other", 1, 1);
    let inside = synth("a.pdf", "/downloads", 1, 1);
    let ops = plan(&[r], &[outside, inside], now()).ops;
    assert_eq!(ops.len(), 1);
    assert!(ops[0].source.as_ref().unwrap().starts_with("/downloads"));
}

#[test]
fn template_errors_are_reported_not_swallowed() {
    let mut r = stop("bad", "pdf", "/docs", 10);
    r.action = Action::Move {
        dest_dir: PathBuf::from("/docs"),
        rename: Some("{nope}".into()),
    };
    let files = vec![synth("a.pdf", "/downloads", 1, 1)];
    let result = plan(&[r], &files, now());
    assert!(result.ops.is_empty());
    assert_eq!(result.issues.len(), 1);
    assert!(result.issues[0].message.contains("unknown token"));
}

#[test]
fn destination_folder_tokens_are_expanded() {
    let mut r = stop("by-year", "pdf", "/archive/{year}", 10);
    r.action = Action::Move {
        dest_dir: PathBuf::from("/archive/{year}"),
        rename: None,
    };
    let files = vec![synth("a.pdf", "/downloads", 1, 0)];
    let ops = plan(&[r], &files, now()).ops;
    let dest = ops[0].destination.as_ref().unwrap();
    assert!(
        dest.to_string_lossy()
            .contains(&format!("{}", now().year())),
        "{dest:?}"
    );
}

#[test]
fn create_folder_is_planned_once_per_destination() {
    let mut r = ext_rule("mk", "pdf", "/x", 10, RuleContinuation::StopAfterAction);
    r.action = Action::CreateFolder {
        path: PathBuf::from("/archive"),
    };
    let files = vec![
        synth("a.pdf", "/downloads", 1, 1),
        synth("b.pdf", "/downloads", 1, 1),
    ];
    assert_eq!(plan(&[r], &files, now()).ops.len(), 1);
}

#[test]
fn tag_action_carries_its_tags() {
    let mut r = stop("tag", "pdf", "/x", 10);
    r.action = Action::Tag {
        tags: vec!["invoice".into()],
    };
    let files = vec![synth("a.pdf", "/downloads", 1, 1)];
    let ops = plan(&[r], &files, now()).ops;
    assert_eq!(ops[0].kind, OpKind::Tag);
    assert_eq!(ops[0].tags, vec!["invoice".to_string()]);
}

#[test]
fn overlap_detected_between_pdf_all_and_pdf_invoice() {
    let a = stop("all-pdf", "pdf", "/A", 10);
    let mut b = stop("invoice-pdf", "pdf", "/B", 20);
    b.condition = ConditionNode::All {
        children: vec![
            ConditionNode::Leaf {
                condition: Condition::Extension {
                    any_of: vec!["pdf".into()],
                },
            },
            ConditionNode::Leaf {
                condition: Condition::FilenameContains {
                    needle: "invoice".into(),
                },
            },
        ],
    };
    assert_eq!(detect_overlaps(&[a, b]).len(), 1);
}

#[test]
fn summary_totals_bytes() {
    let rules = vec![stop("pdfs", "pdf", "/docs", 10)];
    let files = vec![
        synth("a.pdf", "/downloads", 1_000, 1),
        synth("b.pdf", "/downloads", 2_000, 1),
    ];
    let ops = plan(&rules, &files, now()).ops;
    let s = summarize(&ops);
    assert_eq!(s.count, 2);
    assert_eq!(s.total_bytes, 3_000);
}

#[test]
fn a_file_already_at_its_destination_is_left_alone() {
    // Real-time watchers see the file a rule just moved; it must not be
    // moved again into PDF/PDF/.
    let rules = vec![stop("pdfs", "pdf", "/docs/PDF", 10)];
    let files = vec![synth("a.pdf", "/docs/PDF", 1, 1)];
    let result = plan(&rules, &files, now());
    assert!(result.ops.is_empty());
    assert!(result.issues.is_empty());
}
