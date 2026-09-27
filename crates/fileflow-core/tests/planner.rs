mod common;

use common::synth;
use fileflow_core::model::{
    Action, CollisionPolicy, Condition, ConditionNode, OpKind, Rule, RuleContinuation, Trigger,
};
use fileflow_core::planner::{detect_overlaps, plan, summarize};
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
            condition: Condition::Extension { any_of: vec![ext.into()] },
        },
        action: Action::Move { dest_dir: PathBuf::from(dest), rename: None },
        on_conflict: CollisionPolicy::Rename,
        then,
        sources: vec![],
        trigger: Trigger::RealTime,
    }
}

#[test]
fn plan_returns_move_op_for_matching_file() {
    let rules = vec![ext_rule("pdfs", "pdf", "/docs/PDF", 10, RuleContinuation::StopAfterAction)];
    let files = vec![synth("a.pdf", "/downloads", 100, 1)];
    let ops = plan(&rules, &files, OffsetDateTime::now_utc());
    assert_eq!(ops.len(), 1);
    assert!(matches!(ops[0].kind, OpKind::Move));
    assert_eq!(ops[0].destination.as_ref().unwrap(), &PathBuf::from("/docs/PDF/a.pdf"));
    assert_eq!(ops[0].rule_name, "pdfs");
}

#[test]
fn disabled_rules_are_skipped() {
    let mut r = ext_rule("pdfs", "pdf", "/docs/PDF", 10, RuleContinuation::StopAfterAction);
    r.enabled = false;
    let files = vec![synth("a.pdf", "/downloads", 100, 1)];
    assert!(plan(&[r], &files, OffsetDateTime::now_utc()).is_empty());
}

#[test]
fn stop_after_action_prevents_second_rule() {
    // Rule A matches all PDFs and stops. Rule B would also match. Only A fires.
    let rules = vec![
        ext_rule("A", "pdf", "/A", 10, RuleContinuation::StopAfterAction),
        ext_rule("B", "pdf", "/B", 20, RuleContinuation::StopAfterAction),
    ];
    let files = vec![synth("x.pdf", "/downloads", 1, 1)];
    let ops = plan(&rules, &files, OffsetDateTime::now_utc());
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].rule_name, "A");
}

#[test]
fn continue_yields_both_rules() {
    let rules = vec![
        ext_rule("A", "pdf", "/A", 10, RuleContinuation::Continue),
        ext_rule("B", "pdf", "/B", 20, RuleContinuation::StopAfterAction),
    ];
    let files = vec![synth("x.pdf", "/downloads", 1, 1)];
    let ops = plan(&rules, &files, OffsetDateTime::now_utc());
    assert_eq!(ops.len(), 2);
}

#[test]
fn sources_filter_restricts_rule() {
    let mut r = ext_rule("pdfs", "pdf", "/docs/PDF", 10, RuleContinuation::StopAfterAction);
    r.sources = vec![PathBuf::from("/downloads")];
    let outside = synth("a.pdf", "/other", 1, 1);
    let inside = synth("a.pdf", "/downloads", 1, 1);
    let ops = plan(&[r], &[outside, inside], OffsetDateTime::now_utc());
    assert_eq!(ops.len(), 1);
    assert!(ops[0].source.as_ref().unwrap().starts_with("/downloads"));
}

#[test]
fn overlap_detected_between_pdf_all_and_pdf_invoice() {
    let a = ext_rule("all-pdf", "pdf", "/A", 10, RuleContinuation::StopAfterAction);
    let mut b = ext_rule("invoice-pdf", "pdf", "/B", 20, RuleContinuation::StopAfterAction);
    b.condition = ConditionNode::All {
        children: vec![
            ConditionNode::Leaf { condition: Condition::Extension { any_of: vec!["pdf".into()] } },
            ConditionNode::Leaf {
                condition: Condition::FilenameContains { needle: "invoice".into() },
            },
        ],
    };
    let hits = detect_overlaps(&[a, b]);
    assert_eq!(hits.len(), 1);
}

#[test]
fn summary_totals_bytes() {
    let rules = vec![ext_rule("pdfs", "pdf", "/docs", 10, RuleContinuation::StopAfterAction)];
    let files = vec![
        synth("a.pdf", "/downloads", 1_000, 1),
        synth("b.pdf", "/downloads", 2_000, 1),
    ];
    let ops = plan(&rules, &files, OffsetDateTime::now_utc());
    let s = summarize(&ops);
    assert_eq!(s.count, 2);
    assert_eq!(s.total_bytes, 3_000);
}
