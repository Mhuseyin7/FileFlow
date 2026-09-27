use fileflow_core::model::{CollisionPolicy, OpKind, PlannedOp};
use fileflow_core::safety::{validate, Sandbox};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn op(src: Option<PathBuf>, dst: Option<PathBuf>, policy: CollisionPolicy, kind: OpKind) -> PlannedOp {
    PlannedOp {
        id: "op1".into(),
        rule_id: "r".into(),
        rule_name: "r".into(),
        kind,
        source: src,
        destination: dst,
        size: 0,
        collision: policy,
        reasons: vec![],
        warnings: vec![],
    }
}

#[test]
fn destination_outside_sandbox_is_blocked() -> anyhow::Result<()> {
    let inside = TempDir::new()?;
    let outside = TempDir::new()?;
    let sandbox = Sandbox {
        allowed_roots: vec![inside.path().to_path_buf()],
        forbidden_roots: vec![],
    };
    let src = inside.path().join("a.txt");
    fs::write(&src, b"x")?;
    let dst = outside.path().join("a.txt");
    let (ok, errs) = validate(&sandbox, vec![op(Some(src), Some(dst), CollisionPolicy::Rename, OpKind::Move)]);
    assert!(ok.is_empty());
    assert_eq!(errs.len(), 1);
    Ok(())
}

#[test]
fn forbidden_root_blocks_source_and_destination() -> anyhow::Result<()> {
    let a = TempDir::new()?;
    let b = TempDir::new()?;
    let sandbox = Sandbox {
        allowed_roots: vec![a.path().to_path_buf(), b.path().to_path_buf()],
        forbidden_roots: vec![b.path().to_path_buf()],
    };
    let src = a.path().join("a.txt");
    fs::write(&src, b"x")?;
    let dst = b.path().join("a.txt");
    let (ok, errs) = validate(&sandbox, vec![op(Some(src), Some(dst), CollisionPolicy::Rename, OpKind::Move)]);
    assert!(ok.is_empty());
    assert_eq!(errs.len(), 1);
    Ok(())
}

#[test]
fn source_equals_destination_is_blocked() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let p = dir.path().join("a.txt");
    fs::write(&p, b"x")?;
    let sandbox = Sandbox {
        allowed_roots: vec![dir.path().to_path_buf()],
        forbidden_roots: vec![],
    };
    let (ok, errs) = validate(
        &sandbox,
        vec![op(Some(p.clone()), Some(p), CollisionPolicy::Rename, OpKind::Move)],
    );
    assert!(ok.is_empty());
    assert_eq!(errs.len(), 1);
    Ok(())
}

#[test]
fn replace_policy_does_not_error_on_existing_dest() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"x")?;
    fs::write(&dst, b"y")?;
    let sandbox = Sandbox {
        allowed_roots: vec![dir.path().to_path_buf()],
        forbidden_roots: vec![],
    };
    let (ok, _errs) = validate(
        &sandbox,
        vec![op(Some(src), Some(dst), CollisionPolicy::Replace, OpKind::Move)],
    );
    assert_eq!(ok.len(), 1);
    Ok(())
}

#[test]
fn invalid_destination_filename_is_blocked() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"x")?;
    let sandbox = Sandbox {
        allowed_roots: vec![dir.path().to_path_buf()],
        forbidden_roots: vec![],
    };
    let dst = dir.path().join("CON"); // reserved on Windows
    let (ok, errs) = validate(&sandbox, vec![op(Some(src), Some(dst), CollisionPolicy::Rename, OpKind::Move)]);
    assert!(ok.is_empty());
    assert_eq!(errs.len(), 1);
    Ok(())
}
