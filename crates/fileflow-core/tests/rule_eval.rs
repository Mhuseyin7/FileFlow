mod common;

use common::synth;
use fileflow_core::model::{Condition, ConditionNode, NumOp};
use fileflow_core::rule::evaluate;
use std::path::PathBuf;
use time::OffsetDateTime;

fn leaf(c: Condition) -> ConditionNode { ConditionNode::Leaf { condition: c } }
fn all(cs: Vec<ConditionNode>) -> ConditionNode { ConditionNode::All { children: cs } }
fn any(cs: Vec<ConditionNode>) -> ConditionNode { ConditionNode::Any { children: cs } }

#[test]
fn extension_match_is_case_insensitive() {
    let f = synth("Invoice.PDF", "/home/u/Downloads", 1_000, 1);
    // synth lowercases extension; ensure evaluate does too.
    let node = leaf(Condition::Extension { any_of: vec![".pdf".into(), "PDF".into()] });
    let out = evaluate(&node, &f, OffsetDateTime::now_utc());
    assert!(out.matched, "reasons: {:?}", out.reasons);
}

#[test]
fn all_short_circuits_only_on_report_not_evaluation() {
    // ALL should evaluate every leaf so the reasons vector is complete for the
    // UI simulator, but still return false if any leaf fails.
    let f = synth("notes.pdf", "/downloads", 5, 10);
    let node = all(vec![
        leaf(Condition::Extension { any_of: vec!["pdf".into()] }),
        leaf(Condition::AgeDays { op: NumOp::Gt, value: 365 }),
    ]);
    let out = evaluate(&node, &f, OffsetDateTime::now_utc());
    assert!(!out.matched);
    assert_eq!(out.reasons.len(), 2);
    assert!(out.reasons[0].starts_with('✓'));
    assert!(out.reasons[1].starts_with('✗'));
}

#[test]
fn any_true_when_at_least_one_matches() {
    let f = synth("setup.exe", "/downloads", 1_000_000, 100);
    let node = any(vec![
        leaf(Condition::Extension { any_of: vec!["exe".into()] }),
        leaf(Condition::Extension { any_of: vec!["msi".into()] }),
    ]);
    assert!(evaluate(&node, &f, OffsetDateTime::now_utc()).matched);
}

#[test]
fn any_empty_is_false() {
    let f = synth("x.txt", "/tmp", 1, 0);
    let node = any(vec![]);
    assert!(!evaluate(&node, &f, OffsetDateTime::now_utc()).matched);
}

#[test]
fn size_operators_boundary() {
    let f = synth("big.bin", "/downloads", 5 * 1024 * 1024 * 1024, 0);
    let gt = leaf(Condition::SizeBytes { op: NumOp::Gt, value: 5 * 1024 * 1024 * 1024 });
    let ge = leaf(Condition::SizeBytes { op: NumOp::Ge, value: 5 * 1024 * 1024 * 1024 });
    assert!(!evaluate(&gt, &f, OffsetDateTime::now_utc()).matched);
    assert!(evaluate(&ge, &f, OffsetDateTime::now_utc()).matched);
}

#[test]
fn age_uses_modified_time() {
    let f = synth("old.png", "/screenshots", 100, 91);
    let node = leaf(Condition::AgeDays { op: NumOp::Gt, value: 90 });
    assert!(evaluate(&node, &f, OffsetDateTime::now_utc()).matched);
}

#[test]
fn source_folder_recursive_vs_exact() {
    let f = synth("a.pdf", "/home/u/Downloads/Invoices", 10, 1);
    let exact = leaf(Condition::SourceFolder {
        path: PathBuf::from("/home/u/Downloads"),
        recursive: false,
    });
    let recursive = leaf(Condition::SourceFolder {
        path: PathBuf::from("/home/u/Downloads"),
        recursive: true,
    });
    assert!(!evaluate(&exact, &f, OffsetDateTime::now_utc()).matched);
    assert!(evaluate(&recursive, &f, OffsetDateTime::now_utc()).matched);
}

#[test]
fn filename_regex_bad_pattern_does_not_panic() {
    let f = synth("x.txt", "/tmp", 1, 0);
    let node = leaf(Condition::FilenameRegex { pattern: "[".into() });
    let out = evaluate(&node, &f, OffsetDateTime::now_utc());
    assert!(!out.matched);
    assert!(out.reasons[0].contains("invalid"));
}

#[test]
fn deterministic_same_inputs_same_output() {
    let f = synth("invoice-2026.pdf", "/downloads", 20_000, 3);
    let node = all(vec![
        leaf(Condition::Extension { any_of: vec!["pdf".into()] }),
        leaf(Condition::FilenameContains { needle: "invoice".into() }),
    ]);
    let now = OffsetDateTime::now_utc();
    let a = evaluate(&node, &f, now);
    let b = evaluate(&node, &f, now);
    assert_eq!(a.matched, b.matched);
    assert_eq!(a.reasons, b.reasons);
}
