use fileflow_core::model::{
    Action, CollisionPolicy, Condition, ConditionNode, NumOp, Rule, RuleContinuation, Trigger,
};
use std::path::{Path, PathBuf};

fn abs(p: &str) -> PathBuf {
    std::env::temp_dir().join(p)
}

fn leaf(c: Condition) -> ConditionNode {
    ConditionNode::Leaf { condition: c }
}

fn rule(condition: ConditionNode, action: Action) -> Rule {
    Rule {
        id: "r".into(),
        name: "Invoices".into(),
        description: String::new(),
        priority: 100,
        enabled: true,
        profile: None,
        condition,
        action,
        on_conflict: CollisionPolicy::Rename,
        then: RuleContinuation::StopAfterAction,
        sources: vec![],
        trigger: Trigger::RealTime,
    }
}

fn good_action() -> Action {
    Action::Move {
        dest_dir: abs("out"),
        rename: None,
    }
}

fn good_condition() -> ConditionNode {
    ConditionNode::All {
        children: vec![leaf(Condition::Extension {
            any_of: vec!["pdf".into()],
        })],
    }
}

fn is_invalid(r: Rule) -> bool {
    r.normalized().validate().is_err()
}

#[test]
fn a_sensible_rule_validates() {
    assert!(rule(good_condition(), good_action())
        .normalized()
        .validate()
        .is_ok());
}

#[test]
fn rules_that_would_match_every_file_are_rejected() {
    assert!(is_invalid(rule(
        ConditionNode::All { children: vec![] },
        good_action()
    )));
    // An empty nested group is just as bad.
    assert!(is_invalid(rule(
        ConditionNode::All {
            children: vec![ConditionNode::All { children: vec![] }]
        },
        good_action()
    )));
    // The editor's default "contains" text is empty, which matches everything.
    assert!(is_invalid(rule(
        leaf(Condition::FilenameContains {
            needle: "  ".into()
        }),
        good_action()
    )));
    assert!(is_invalid(rule(
        leaf(Condition::FilenameRegex { pattern: "".into() }),
        good_action()
    )));
}

#[test]
fn bad_leaf_conditions_are_rejected() {
    for c in [
        Condition::FilenameRegex {
            pattern: "[".into(),
        },
        Condition::Extension { any_of: vec![] },
        Condition::Extension {
            any_of: vec!["a/b".into()],
        },
        Condition::Category { any_of: vec![] },
        Condition::AgeDays {
            op: NumOp::Gt,
            value: -1,
        },
        Condition::ModifiedBetween {
            from: None,
            to: None,
        },
        Condition::SourceFolder {
            path: PathBuf::from("relative"),
            recursive: true,
        },
    ] {
        assert!(is_invalid(rule(leaf(c.clone()), good_action())), "{c:?}");
    }
}

#[test]
fn bad_actions_are_rejected() {
    for a in [
        Action::Move {
            dest_dir: PathBuf::from("relative/dir"),
            rename: None,
        },
        Action::Move {
            dest_dir: PathBuf::new(),
            rename: None,
        },
        Action::Move {
            dest_dir: abs("a").join("..").join("b"),
            rename: None,
        },
        Action::Move {
            dest_dir: abs("out"),
            rename: Some("{nope}".into()),
        },
        Action::Rename { pattern: "".into() },
        Action::CreateFolder {
            path: PathBuf::from("x"),
        },
        Action::Tag { tags: vec![] },
        Action::Tag {
            tags: vec!["".into()],
        },
    ] {
        assert!(is_invalid(rule(good_condition(), a.clone())), "{a:?}");
    }
}

#[test]
fn a_name_is_required() {
    let mut r = rule(good_condition(), good_action());
    r.name = "   ".into();
    assert!(is_invalid(r));
}

#[test]
fn normalization_expands_tilde_and_cleans_extensions() {
    let mut r = rule(
        leaf(Condition::Extension {
            any_of: vec![" .PDF ".into()],
        }),
        Action::Move {
            dest_dir: PathBuf::from("~").join("Documents"),
            rename: None,
        },
    );
    r.sources = vec![PathBuf::from("~").join("Downloads")];
    let n = r.normalized();
    match &n.action {
        Action::Move { dest_dir, .. } => {
            assert!(dest_dir.is_absolute(), "{dest_dir:?}");
            assert!(!dest_dir.starts_with("~"));
        }
        _ => unreachable!(),
    }
    assert!(n.sources[0].is_absolute());
    assert_eq!(
        n.condition,
        leaf(Condition::Extension {
            any_of: vec!["pdf".into()]
        })
    );
    assert!(n.validate().is_ok());
}

#[test]
fn folder_tokens_are_allowed_in_destinations() {
    let a = Action::Move {
        dest_dir: Path::new(&abs("out")).join("{year}"),
        rename: Some("{date}-{name}".into()),
    };
    assert!(rule(good_condition(), a).normalized().validate().is_ok());
}
