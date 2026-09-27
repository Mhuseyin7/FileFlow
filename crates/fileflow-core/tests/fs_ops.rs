//! Filesystem operation tests. All I/O happens inside a `tempfile::TempDir`;
//! we never touch the user's real folders.

use fileflow_core::fs_ops::{execute, next_available, ExecuteOpts};
use fileflow_core::journal::{Journal, OpState, UndoStatus};
use fileflow_core::model::{CollisionPolicy, OpKind, PlannedOp};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn planned(kind: OpKind, src: Option<PathBuf>, dst: Option<PathBuf>, policy: CollisionPolicy) -> PlannedOp {
    PlannedOp {
        id: PlannedOp::new_id(),
        rule_id: "rule".into(),
        rule_name: "rule".into(),
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
fn move_atomic_same_dir() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"hello")?;
    let dst = dir.path().join("moved").join("a.txt");
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    let e = execute(&op, ExecuteOpts { verify_hash: false, dry_run: false, use_os_trash: false })?;
    assert!(dst.exists());
    assert!(!src.exists());
    assert_eq!(e.bytes, 5);
    Ok(())
}

#[test]
fn move_collision_rename_gets_suffix() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("a.txt.dest");
    fs::write(&src, b"x")?;
    // Pre-existing destination.
    fs::write(&dst, b"y")?;
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    let e = execute(&op, ExecuteOpts { verify_hash: false, dry_run: false, use_os_trash: false })?;
    // Rename policy should have written to `a.txt.dest (1)` or similar.
    assert!(e.destination.unwrap().to_string_lossy().contains("(1)"));
    Ok(())
}

#[test]
fn move_collision_skip_errors() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("a.txt.dest");
    fs::write(&src, b"x")?;
    fs::write(&dst, b"y")?;
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Skip);
    let r = execute(&op, ExecuteOpts { verify_hash: false, dry_run: false, use_os_trash: false });
    assert!(r.is_err());
    // Source untouched
    assert!(src.exists());
    Ok(())
}

#[test]
fn dry_run_does_not_touch_filesystem() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"hello")?;
    let dst = dir.path().join("moved").join("a.txt");
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    execute(&op, ExecuteOpts { verify_hash: false, dry_run: true, use_os_trash: false })?;
    assert!(src.exists());
    assert!(!dst.exists());
    Ok(())
}

#[test]
fn copy_verify_produces_identical_file() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("copy").join("dst.bin");
    // ~10 MB of pseudo-random data to exercise the streaming path.
    let payload: Vec<u8> = (0..(10 * 1024 * 1024)).map(|i| (i * 31) as u8).collect();
    fs::write(&src, &payload)?;
    let op = planned(OpKind::Copy, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    let e = execute(&op, ExecuteOpts { verify_hash: true, dry_run: false, use_os_trash: false })?;
    assert_eq!(e.bytes as usize, payload.len());
    assert_eq!(fs::read(&dst)?, payload);
    assert!(src.exists(), "copy must not delete source");
    Ok(())
}

#[test]
fn trash_moves_to_local_trash_folder_when_os_trash_disabled() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"bye")?;
    let op = planned(OpKind::Trash, Some(src.clone()), None, CollisionPolicy::Rename);
    let e = execute(
        &op,
        ExecuteOpts { verify_hash: false, dry_run: false, use_os_trash: false },
    )?;
    assert!(!src.exists());
    let dst = e.destination.unwrap();
    assert!(dst.exists());
    assert!(dst.to_string_lossy().contains(".fileflow-trash"));
    Ok(())
}

#[cfg(feature = "os-trash")]
#[test]
fn trash_routes_to_os_trash_when_enabled() -> anyhow::Result<()> {
    // The OS trash test is opt-in per-CI (some CI runners lack a shell trash
    // implementation, e.g. headless Linux without `gio`); skip when the env
    // var says so.
    if std::env::var_os("FILEFLOW_SKIP_OS_TRASH").is_some() {
        return Ok(());
    }
    let dir = TempDir::new()?;
    let src = dir.path().join("os-trash-target.txt");
    fs::write(&src, b"bye")?;
    let op = planned(OpKind::Trash, Some(src.clone()), None, CollisionPolicy::Rename);
    let e = execute(
        &op,
        ExecuteOpts { verify_hash: false, dry_run: false, use_os_trash: true },
    )?;
    // Either the OS trash accepted it (destination is None, source gone) or
    // the executor fell back to the local trash (destination set); both are
    // acceptable per fs_ops.rs contract.
    assert!(!src.exists(), "source must be gone either way");
    if let Some(dst) = e.destination {
        assert!(dst.to_string_lossy().contains(".fileflow-trash"));
    }
    Ok(())
}

#[test]
fn next_available_appends_counter() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("x.txt");
    fs::write(&base, b"a").unwrap();
    let next = next_available(&base);
    assert_eq!(next.file_name().unwrap().to_string_lossy(), "x (1).txt");
}

#[test]
fn journal_records_and_undoes_move() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"hello world")?;
    let journal = Journal::open_in_memory()?;
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    journal.record_start(&op)?;
    let e = execute(&op, ExecuteOpts { verify_hash: true, dry_run: false, use_os_trash: false })?;
    journal.record_success(&e)?;

    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.state, OpState::Completed);
    assert_eq!(entry.undo_status, UndoStatus::Available);

    journal.undo(&op.id)?;
    // File back at original path
    assert!(src.exists());
    assert!(!dst.exists());
    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.state, OpState::Undone);
    Ok(())
}

#[test]
fn undo_refuses_when_size_differs() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"one")?;
    let journal = Journal::open_in_memory()?;
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    journal.record_start(&op)?;
    let e = execute(&op, ExecuteOpts { verify_hash: true, dry_run: false, use_os_trash: false })?;
    journal.record_success(&e)?;
    // Tamper with the destination: user edited it.
    fs::write(&dst, b"much larger content now")?;
    let r = journal.undo(&op.id);
    assert!(r.is_err(), "must refuse undo when destination changed");
    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.undo_status, UndoStatus::Unsafe);
    Ok(())
}

#[test]
fn undo_refuses_when_original_path_occupied() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"one")?;
    let journal = Journal::open_in_memory()?;
    let op = planned(OpKind::Move, Some(src.clone()), Some(dst.clone()), CollisionPolicy::Rename);
    journal.record_start(&op)?;
    let e = execute(&op, ExecuteOpts { verify_hash: true, dry_run: false, use_os_trash: false })?;
    journal.record_success(&e)?;
    // A new file appears at the original path — undo would overwrite it.
    fs::write(&src, b"user re-created")?;
    let r = journal.undo(&op.id);
    assert!(r.is_err());
    Ok(())
}
