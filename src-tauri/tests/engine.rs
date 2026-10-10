use fileflow_app_lib::commands::*;
use fileflow_app_lib::engine::{process_file, process_schedule};
use fileflow_app_lib::state::AppState;
use fileflow_core::model::{
    Action, CollisionPolicy, Condition, ConditionNode, Rule, RuleContinuation, Trigger,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn rule(name: &str, ext: &str, action: Action, trigger: Trigger) -> Rule {
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
        action,
        on_conflict: CollisionPolicy::Rename,
        then: RuleContinuation::StopAfterAction,
        sources: vec![],
        trigger,
    }
}

fn mv(dest: &Path) -> Action {
    Action::Move {
        dest_dir: dest.to_path_buf(),
        rename: None,
    }
}

fn app() -> (TempDir, AppState, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let inbox = tmp.path().join("inbox");
    fs::create_dir_all(&inbox).unwrap();
    let state = AppState::new(tmp.path().join("data")).unwrap();
    ff_set_sandbox_roots(&state, vec![inbox.clone()]).unwrap();
    (tmp, state, inbox)
}

#[test]
fn a_new_file_is_organized_by_the_real_time_rules() {
    let (_t, state, inbox) = app();
    ff_save_rule(
        &state,
        rule("pdfs", "pdf", mv(&inbox.join("PDF")), Trigger::RealTime),
    )
    .unwrap();
    let f = inbox.join("a.pdf");
    fs::write(&f, b"x").unwrap();
    assert_eq!(process_file(&state, &f), 1);
    assert!(inbox.join("PDF").join("a.pdf").exists());
    assert!(!f.exists());
}

#[test]
fn the_file_a_rule_just_moved_is_not_processed_again() {
    let (_t, state, inbox) = app();
    ff_save_rule(
        &state,
        rule("pdfs", "pdf", mv(&inbox.join("PDF")), Trigger::RealTime),
    )
    .unwrap();
    let f = inbox.join("a.pdf");
    fs::write(&f, b"x").unwrap();
    process_file(&state, &f);
    // The watcher reports the moved file next.
    let moved = inbox.join("PDF").join("a.pdf");
    assert_eq!(process_file(&state, &moved), 0);
    assert!(moved.exists());
    assert!(!inbox.join("PDF").join("PDF").exists());
}

#[test]
fn a_rename_rule_does_not_rename_its_own_output_forever() {
    let (_t, state, inbox) = app();
    let r = rule(
        "stamp",
        "txt",
        Action::Rename {
            pattern: "x-{name}".into(),
        },
        Trigger::RealTime,
    );
    ff_save_rule(&state, r).unwrap();
    let f = inbox.join("a.txt");
    fs::write(&f, b"x").unwrap();
    assert_eq!(process_file(&state, &f), 1);
    let renamed = inbox.join("x-a.txt");
    assert!(renamed.exists());
    assert_eq!(process_file(&state, &renamed), 0);
    assert!(renamed.exists() && !inbox.join("x-x-a.txt").exists());
}

#[test]
fn files_outside_the_granted_folders_are_ignored() {
    let (tmp, state, inbox) = app();
    ff_save_rule(
        &state,
        rule("pdfs", "pdf", mv(&inbox.join("PDF")), Trigger::RealTime),
    )
    .unwrap();
    let outside = tmp.path().join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    let f = outside.join("a.pdf");
    fs::write(&f, b"x").unwrap();
    assert_eq!(process_file(&state, &f), 0);
    assert!(f.exists());
}

#[test]
fn disabled_and_scheduled_rules_do_not_run_in_real_time() {
    let (_t, state, inbox) = app();
    let mut off = rule("off", "pdf", mv(&inbox.join("A")), Trigger::RealTime);
    off.enabled = false;
    ff_save_rule(&state, off).unwrap();
    ff_save_rule(
        &state,
        rule("daily", "pdf", mv(&inbox.join("B")), Trigger::Daily),
    )
    .unwrap();
    let f = inbox.join("a.pdf");
    fs::write(&f, b"x").unwrap();
    assert_eq!(process_file(&state, &f), 0);
    assert!(f.exists());
}

#[test]
fn a_scheduled_rule_sweeps_the_granted_folders() {
    let (_t, state, inbox) = app();
    let saved = ff_save_rule(
        &state,
        rule("daily", "pdf", mv(&inbox.join("B")), Trigger::Daily),
    )
    .unwrap();
    fs::write(inbox.join("a.pdf"), b"1").unwrap();
    fs::create_dir_all(inbox.join("sub")).unwrap();
    fs::write(inbox.join("sub").join("b.pdf"), b"2").unwrap();
    fs::write(inbox.join("c.txt"), b"3").unwrap();

    assert_eq!(process_schedule(&state, &saved.id), 2);
    assert!(inbox.join("B").join("a.pdf").exists());
    assert!(inbox.join("B").join("b.pdf").exists());
    assert!(inbox.join("c.txt").exists());
    // Running it again finds nothing new (the moved files are its output).
    assert_eq!(process_schedule(&state, &saved.id), 0);
}

#[test]
fn a_sweep_skips_downloads_that_are_still_in_progress() {
    let (_t, state, inbox) = app();
    let saved = ff_save_rule(
        &state,
        rule("daily", "part", mv(&inbox.join("B")), Trigger::Daily),
    )
    .unwrap();
    fs::write(inbox.join("movie.part"), b"1").unwrap();
    assert_eq!(process_schedule(&state, &saved.id), 0);
    assert!(inbox.join("movie.part").exists());
}
