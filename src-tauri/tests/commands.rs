use fileflow_app_lib::commands::*;
use fileflow_app_lib::state::AppState;
use fileflow_core::journal::OpState;
use fileflow_core::model::{
    Action, CollisionPolicy, Condition, ConditionNode, OpKind, PlannedOp, Rule, RuleContinuation,
    Trigger,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn rule(name: &str, ext: &str, dest: &Path) -> Rule {
    Rule {
        id: String::new(),
        name: name.into(),
        description: String::new(),
        priority: 100,
        enabled: true,
        profile: None,
        condition: ConditionNode::All {
            children: vec![ConditionNode::Leaf {
                condition: Condition::Extension {
                    any_of: vec![ext.into()],
                },
            }],
        },
        action: Action::Move {
            dest_dir: dest.to_path_buf(),
            rename: None,
        },
        on_conflict: CollisionPolicy::Rename,
        then: RuleContinuation::StopAfterAction,
        sources: vec![],
        trigger: Trigger::RealTime,
    }
}

/// An app whose data dir and granted folder live in a fresh temp dir.
fn app() -> (TempDir, AppState, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    let inbox = tmp.path().join("inbox");
    fs::create_dir_all(&inbox).unwrap();
    let state = AppState::new(data).unwrap();
    ff_set_sandbox_roots(&state, vec![inbox.clone()]).unwrap();
    (tmp, state, inbox)
}

#[test]
fn a_fresh_app_has_no_granted_folders_and_previews_nothing() {
    let tmp = TempDir::new().unwrap();
    let state = AppState::new(tmp.path().join("data")).unwrap();
    assert!(ff_get_sandbox_roots(&state).is_empty());
    assert!(ff_preview(&state, tmp.path(), None).is_err());
}

#[test]
fn rules_and_folders_survive_a_restart() {
    let (tmp, state, inbox) = app();
    let saved = ff_save_rule(&state, rule("pdfs", "pdf", &inbox.join("PDF"))).unwrap();
    assert!(!saved.id.is_empty());
    drop(state);

    let again = AppState::new(tmp.path().join("data")).unwrap();
    assert_eq!(ff_list_rules(&again).len(), 1);
    assert_eq!(ff_list_rules(&again)[0].name, "pdfs");
    assert_eq!(ff_get_sandbox_roots(&again).len(), 1);

    ff_delete_rule(&again, &saved.id).unwrap();
    drop(again);
    let third = AppState::new(tmp.path().join("data")).unwrap();
    assert!(ff_list_rules(&third).is_empty());
}

#[test]
fn invalid_rules_are_rejected_with_a_message() {
    let (_t, state, inbox) = app();
    let mut r = rule("all", "pdf", &inbox);
    r.condition = ConditionNode::All { children: vec![] };
    let err = ff_save_rule(&state, r).unwrap_err().to_string();
    assert!(err.contains("every file"), "{err}");
    assert!(ff_list_rules(&state).is_empty());
}

#[test]
fn dangerous_folders_cannot_be_granted() {
    let (_t, state, _inbox) = app();
    let root = std::env::temp_dir()
        .ancestors()
        .last()
        .unwrap()
        .to_path_buf();
    assert!(ff_set_sandbox_roots(&state, vec![root]).is_err());
    assert_eq!(ff_get_sandbox_roots(&state).len(), 1, "previous grant kept");
}

#[test]
fn preview_then_run_moves_the_file_and_undo_brings_it_back() {
    let (_t, state, inbox) = app();
    ff_save_rule(&state, rule("pdfs", "pdf", &inbox.join("PDF"))).unwrap();
    fs::write(inbox.join("a.pdf"), b"hello").unwrap();

    let preview = ff_preview(&state, &inbox, None).unwrap();
    assert_eq!(preview.ops.len(), 1);
    assert!(preview.errors.is_empty());
    assert!(inbox.join("a.pdf").exists(), "preview changes nothing");

    let outcomes = ff_run(&state, preview.ops.clone(), false);
    assert_eq!(outcomes[0].status, RunStatus::Done);
    assert!(inbox.join("PDF").join("a.pdf").exists());
    assert!(!inbox.join("a.pdf").exists());

    ff_undo(&state, &outcomes[0].op_id).unwrap();
    assert!(inbox.join("a.pdf").exists());
}

#[test]
fn dry_run_writes_nothing_and_journals_nothing() {
    let (_t, state, inbox) = app();
    ff_save_rule(&state, rule("pdfs", "pdf", &inbox.join("PDF"))).unwrap();
    fs::write(inbox.join("a.pdf"), b"x").unwrap();
    let preview = ff_preview(&state, &inbox, None).unwrap();
    let out = ff_run(&state, preview.ops, true);
    assert_eq!(out[0].status, RunStatus::Done);
    assert!(inbox.join("a.pdf").exists());
    assert!(ff_journal(&state, 10).unwrap().is_empty());
}

fn forged_move(src: &Path, dst: &Path) -> PlannedOp {
    PlannedOp {
        id: PlannedOp::new_id(),
        rule_id: "forged".into(),
        rule_name: "forged".into(),
        kind: OpKind::Move,
        source: Some(src.to_path_buf()),
        destination: Some(dst.to_path_buf()),
        size: 0,
        collision: CollisionPolicy::Rename,
        reasons: vec![],
        warnings: vec![],
        tags: vec![],
    }
}

#[test]
fn run_revalidates_ops_the_frontend_sends() {
    let (tmp, state, inbox) = app();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"s").unwrap();
    fs::write(inbox.join("a.txt"), b"a").unwrap();

    // Source outside the grant, and destination outside the grant.
    let a = forged_move(&outside.join("secret.txt"), &inbox.join("secret.txt"));
    let b = forged_move(&inbox.join("a.txt"), &outside.join("a.txt"));
    let out = ff_run(&state, vec![a, b], false);
    assert!(out.iter().all(|o| o.status == RunStatus::Failed), "{out:?}");
    assert!(outside.join("secret.txt").exists());
    assert!(inbox.join("a.txt").exists());
    assert!(!outside.join("a.txt").exists());
}

#[test]
fn ask_policy_sends_the_file_to_the_review_queue_once() {
    let (_t, state, inbox) = app();
    let mut r = rule("pdfs", "pdf", &inbox.join("PDF"));
    r.on_conflict = CollisionPolicy::Ask;
    ff_save_rule(&state, r).unwrap();
    fs::create_dir_all(inbox.join("PDF")).unwrap();
    fs::write(inbox.join("PDF").join("a.pdf"), b"old").unwrap();
    fs::write(inbox.join("a.pdf"), b"new").unwrap();

    for _ in 0..2 {
        let preview = ff_preview(&state, &inbox, None).unwrap();
        let ops: Vec<_> = preview
            .ops
            .into_iter()
            .filter(|o| o.source.as_deref() == Some(inbox.join("a.pdf").as_path()))
            .collect();
        let out = ff_run(&state, ops, false);
        assert_eq!(out[0].status, RunStatus::NeedsReview);
    }
    assert!(inbox.join("a.pdf").exists(), "nothing was moved");
    let queue = ff_review_queue(&state).unwrap();
    assert_eq!(queue.len(), 1, "no duplicates");
    ff_resolve_review(&state, &queue[0].id, "dismissed").unwrap();
    assert!(ff_review_queue(&state).unwrap().is_empty());
}

#[test]
fn skip_policy_is_reported_as_skipped() {
    let (_t, state, inbox) = app();
    let mut r = rule("pdfs", "pdf", &inbox.join("PDF"));
    r.on_conflict = CollisionPolicy::Skip;
    ff_save_rule(&state, r).unwrap();
    fs::create_dir_all(inbox.join("PDF")).unwrap();
    fs::write(inbox.join("PDF").join("a.pdf"), b"old").unwrap();
    fs::write(inbox.join("a.pdf"), b"new").unwrap();
    let preview = ff_preview(&state, &inbox, None).unwrap();
    let ops: Vec<_> = preview
        .ops
        .into_iter()
        .filter(|o| o.source.as_deref() == Some(inbox.join("a.pdf").as_path()))
        .collect();
    let out = ff_run(&state, ops, false);
    assert_eq!(out[0].status, RunStatus::Skipped);
    let entry = &ff_journal(&state, 10).unwrap()[0];
    assert_eq!(entry.state, OpState::Skipped);
}

#[test]
fn tag_action_is_recorded_without_touching_the_file() {
    let (_t, state, inbox) = app();
    let mut r = rule("tag", "pdf", &inbox);
    r.action = Action::Tag {
        tags: vec!["invoice".into()],
    };
    ff_save_rule(&state, r).unwrap();
    fs::write(inbox.join("a.pdf"), b"x").unwrap();
    let preview = ff_preview(&state, &inbox, None).unwrap();
    let out = ff_run(&state, preview.ops, false);
    assert_eq!(out[0].status, RunStatus::Done, "{out:?}");
    assert!(inbox.join("a.pdf").exists());
    assert_eq!(
        state.store.tags_for(&inbox.join("a.pdf")).unwrap(),
        ["invoice"]
    );
}

#[test]
fn an_interrupted_operation_is_reconciled_at_startup() {
    let (tmp, state, inbox) = app();
    let src = inbox.join("a.txt");
    fs::write(&src, b"x").unwrap();
    // Journal entry written, then "crash" before the move.
    state
        .journal
        .record_start(&forged_move(&src, &inbox.join("b.txt")))
        .unwrap();
    drop(state);

    let again = AppState::new(tmp.path().join("data")).unwrap();
    assert_eq!(ff_recovered(&again).len(), 1);
    assert!(src.exists());
    assert!(ff_recovered(&AppState::new(tmp.path().join("data")).unwrap()).is_empty());
}

#[test]
fn a_rule_can_be_previewed_alone_even_when_disabled_or_outranked() {
    let (_t, state, inbox) = app();
    let mut first = rule("first", "pdf", &inbox.join("A"));
    first.priority = 1;
    ff_save_rule(&state, first).unwrap();
    let mut second = rule("second", "pdf", &inbox.join("B"));
    second.priority = 2;
    second.enabled = false;
    let second = ff_save_rule(&state, second).unwrap();
    fs::write(inbox.join("a.pdf"), b"x").unwrap();

    let all = ff_preview(&state, &inbox, None).unwrap();
    assert_eq!(all.ops[0].rule_name, "first", "first outranks second");
    let only = ff_preview(&state, &inbox, Some(&second.id)).unwrap();
    assert_eq!(only.ops.len(), 1);
    assert_eq!(only.ops[0].rule_name, "second");
}

#[test]
fn preview_expands_a_leading_tilde() {
    let (_t, state, _inbox) = app();
    // "~" resolves to the home directory, which is not granted here.
    let err = ff_preview(&state, Path::new("~"), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("outside the folders you granted"), "{err}");
}
