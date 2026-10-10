use fileflow_core::model::{CollisionPolicy, OpKind, PlannedOp};
use fileflow_core::safety::{default_forbidden_roots, validate, vet_root, Sandbox};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn op(src: Option<PathBuf>, dst: Option<PathBuf>, policy: CollisionPolicy) -> PlannedOp {
    PlannedOp {
        id: "op1".into(),
        rule_id: "r".into(),
        rule_name: "r".into(),
        kind: OpKind::Move,
        source: src,
        destination: dst,
        size: 0,
        collision: policy,
        reasons: vec![],
        warnings: vec![],
        tags: vec![],
    }
}

fn sandbox(allowed: &[&Path], forbidden: &[&Path]) -> Sandbox {
    Sandbox::new(
        allowed.iter().map(|p| p.to_path_buf()),
        forbidden.iter().map(|p| p.to_path_buf()),
    )
}

fn run_one(sb: &Sandbox, o: PlannedOp) -> (usize, usize) {
    let (ok, errs) = validate(sb, vec![o]);
    (ok.len(), errs.len())
}

#[test]
fn empty_sandbox_denies_everything() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"x")?;
    let dst = dir.path().join("b.txt");
    let sb = Sandbox::default();
    assert_eq!(
        run_one(&sb, op(Some(src), Some(dst), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn destination_outside_sandbox_is_blocked() -> anyhow::Result<()> {
    let inside = TempDir::new()?;
    let outside = TempDir::new()?;
    let sb = sandbox(&[inside.path()], &[]);
    let src = inside.path().join("a.txt");
    fs::write(&src, b"x")?;
    let dst = outside.path().join("a.txt");
    assert_eq!(
        run_one(&sb, op(Some(src), Some(dst), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn source_outside_sandbox_is_blocked() -> anyhow::Result<()> {
    let inside = TempDir::new()?;
    let outside = TempDir::new()?;
    let sb = sandbox(&[inside.path()], &[]);
    let src = outside.path().join("a.txt");
    fs::write(&src, b"x")?;
    let dst = inside.path().join("a.txt");
    assert_eq!(
        run_one(&sb, op(Some(src), Some(dst), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn dot_dot_traversal_is_blocked() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let granted = root.path().join("granted");
    fs::create_dir_all(&granted)?;
    let sb = sandbox(&[&granted], &[]);
    let src = granted.join("a.txt");
    fs::write(&src, b"x")?;
    // Lexically starts with the granted root, but climbs out of it.
    let dst = granted.join("..").join("escaped.txt");
    assert_eq!(
        run_one(&sb, op(Some(src), Some(dst), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn relative_paths_are_blocked() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"x")?;
    let sb = sandbox(&[dir.path()], &[]);
    let rel = PathBuf::from("a.txt");
    assert_eq!(
        run_one(&sb, op(Some(src), Some(rel), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn forbidden_root_wins_over_allowed_root() -> anyhow::Result<()> {
    let a = TempDir::new()?;
    let b = TempDir::new()?;
    let sb = sandbox(&[a.path(), b.path()], &[b.path()]);
    let src = a.path().join("a.txt");
    fs::write(&src, b"x")?;
    let dst = b.path().join("a.txt");
    assert_eq!(
        run_one(&sb, op(Some(src), Some(dst), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn source_equals_destination_is_blocked() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let p = dir.path().join("a.txt");
    fs::write(&p, b"x")?;
    let sb = sandbox(&[dir.path()], &[]);
    assert_eq!(
        run_one(&sb, op(Some(p.clone()), Some(p), CollisionPolicy::Rename)),
        (0, 1)
    );
    Ok(())
}

#[test]
fn invalid_destination_names_are_blocked() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"x")?;
    let sb = sandbox(&[dir.path()], &[]);
    for bad in ["CON", "what?.txt", "trailing."] {
        let dst = dir.path().join(bad);
        assert_eq!(
            run_one(
                &sb,
                op(Some(src.clone()), Some(dst), CollisionPolicy::Rename)
            ),
            (0, 1),
            "{bad} should be rejected"
        );
    }
    Ok(())
}

#[test]
fn existing_destination_is_a_warning_not_an_error() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"x")?;
    fs::write(&dst, b"y")?;
    let sb = sandbox(&[dir.path()], &[]);
    for policy in [
        CollisionPolicy::Rename,
        CollisionPolicy::Skip,
        CollisionPolicy::Ask,
        CollisionPolicy::Replace,
    ] {
        let (ok, errs) = validate(&sb, vec![op(Some(src.clone()), Some(dst.clone()), policy)]);
        assert!(errs.is_empty());
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].warnings.len(), 1, "{policy:?}");
    }
    Ok(())
}

#[test]
fn duplicate_destinations_in_one_batch_are_flagged() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, b"1")?;
    fs::write(&b, b"2")?;
    let dst = dir.path().join("out").join("same.txt");
    let sb = sandbox(&[dir.path()], &[]);
    let mut second = op(Some(b), Some(dst.clone()), CollisionPolicy::Rename);
    second.id = "op2".into();
    let (ok, errs) = validate(
        &sb,
        vec![op(Some(a), Some(dst), CollisionPolicy::Rename), second],
    );
    assert!(errs.is_empty());
    assert!(ok.iter().all(|o| !o.warnings.is_empty()));
    Ok(())
}

#[test]
fn default_forbidden_roots_are_never_empty() {
    assert!(!default_forbidden_roots(&[]).is_empty());
    let extra = PathBuf::from("/some/data/dir");
    assert!(default_forbidden_roots(std::slice::from_ref(&extra)).contains(&extra));
}

#[test]
fn vet_root_rejects_unsafe_candidates() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let file = dir.path().join("f.txt");
    fs::write(&file, b"x")?;
    let forbidden = vec![dir.path().join("protected")];
    fs::create_dir_all(&forbidden[0])?;

    assert!(vet_root(dir.path(), &[]).is_ok());
    assert!(vet_root(&file, &[]).is_err(), "files are not folders");
    assert!(vet_root(&dir.path().join("missing"), &[]).is_err());
    assert!(vet_root(Path::new("relative/dir"), &[]).is_err());
    assert!(vet_root(&forbidden[0], &forbidden).is_err());
    assert!(
        vet_root(dir.path(), &forbidden).is_err(),
        "a folder containing a protected location is rejected"
    );

    let fs_root = dir.path().ancestors().last().unwrap().to_path_buf();
    assert!(
        vet_root(&fs_root, &[]).is_err(),
        "filesystem roots are rejected"
    );
    Ok(())
}

#[cfg(unix)]
mod unix_only {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinked_directory_cannot_be_used_to_escape() -> anyhow::Result<()> {
        let inside = TempDir::new()?;
        let outside = TempDir::new()?;
        symlink(outside.path(), inside.path().join("link"))?;
        let src = inside.path().join("a.txt");
        fs::write(&src, b"x")?;
        let dst = inside.path().join("link").join("a.txt");
        let sb = sandbox(&[inside.path()], &[]);
        assert_eq!(
            run_one(&sb, op(Some(src), Some(dst), CollisionPolicy::Rename)),
            (0, 1)
        );
        Ok(())
    }

    #[test]
    fn copying_a_symlink_is_refused() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let target = dir.path().join("target.txt");
        fs::write(&target, b"x")?;
        let link = dir.path().join("link.txt");
        symlink(&target, &link)?;
        let sb = sandbox(&[dir.path()], &[]);
        let mut o = op(
            Some(link),
            Some(dir.path().join("copy.txt")),
            CollisionPolicy::Rename,
        );
        o.kind = OpKind::Copy;
        assert_eq!(run_one(&sb, o), (0, 1));
        Ok(())
    }
}
