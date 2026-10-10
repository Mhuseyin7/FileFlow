//! Filesystem operation tests. All I/O happens inside a `tempfile::TempDir`;
//! we never touch the user's real folders.

use fileflow_core::fs_ops::{
    execute, hash_file, move_by_copy, next_available, ExecuteOpts, LOCAL_TRASH_DIR,
};
use fileflow_core::model::{CollisionPolicy, OpKind, PlannedOp};
use fileflow_core::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

fn planned(
    kind: OpKind,
    src: Option<PathBuf>,
    dst: Option<PathBuf>,
    policy: CollisionPolicy,
) -> PlannedOp {
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
        tags: vec![],
    }
}

fn opts() -> ExecuteOpts {
    ExecuteOpts {
        use_os_trash: false,
        ..ExecuteOpts::default()
    }
}

#[test]
fn move_atomic_same_dir() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"hello")?;
    let dst = dir.path().join("moved").join("a.txt");
    let op = planned(
        OpKind::Move,
        Some(src.clone()),
        Some(dst.clone()),
        CollisionPolicy::Rename,
    );
    let e = execute(&op, &opts())?;
    assert!(dst.exists());
    assert!(!src.exists());
    assert_eq!(e.bytes, 5);
    assert!(e.dest_modified_ns.is_some());
    Ok(())
}

#[test]
fn move_collision_rename_never_overwrites() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"new")?;
    fs::write(&dst, b"existing")?;
    let op = planned(
        OpKind::Move,
        Some(src.clone()),
        Some(dst.clone()),
        CollisionPolicy::Rename,
    );
    let e = execute(&op, &opts())?;
    assert_eq!(fs::read(&dst)?, b"existing", "existing file untouched");
    let landed = e.destination.unwrap();
    assert!(landed.to_string_lossy().contains("(1)"));
    assert_eq!(fs::read(landed)?, b"new");
    assert!(!src.exists());
    Ok(())
}

#[test]
fn move_collision_skip_is_a_typed_skip_and_changes_nothing() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"x")?;
    fs::write(&dst, b"y")?;
    let op = planned(
        OpKind::Move,
        Some(src.clone()),
        Some(dst),
        CollisionPolicy::Skip,
    );
    assert!(matches!(execute(&op, &opts()), Err(Error::Skipped(_))));
    assert!(src.exists());
    Ok(())
}

#[test]
fn move_collision_ask_needs_input() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"x")?;
    fs::write(&dst, b"y")?;
    let op = planned(
        OpKind::Move,
        Some(src.clone()),
        Some(dst),
        CollisionPolicy::Ask,
    );
    assert!(matches!(execute(&op, &opts()), Err(Error::NeedsInput(_))));
    assert!(src.exists());
    Ok(())
}

#[test]
fn replace_moves_the_existing_file_to_trash_instead_of_destroying_it() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"new")?;
    fs::write(&dst, b"precious")?;
    let op = planned(
        OpKind::Move,
        Some(src),
        Some(dst.clone()),
        CollisionPolicy::Replace,
    );
    let e = execute(&op, &opts())?;
    assert_eq!(fs::read(&dst)?, b"new");
    let trashed = e.replaced.expect("replaced file is tracked");
    assert!(trashed.to_string_lossy().contains(LOCAL_TRASH_DIR));
    assert_eq!(fs::read(trashed)?, b"precious");
    Ok(())
}

#[test]
fn dry_run_does_not_touch_filesystem() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"hello")?;
    let dst = dir.path().join("moved").join("a.txt");
    let op = planned(
        OpKind::Move,
        Some(src.clone()),
        Some(dst.clone()),
        CollisionPolicy::Rename,
    );
    execute(&op, &ExecuteOpts::dry_run())?;
    assert!(src.exists());
    assert!(!dst.exists());
    assert!(!dir.path().join("moved").exists(), "no folders created");
    Ok(())
}

#[test]
fn folders_cannot_be_moved_by_a_rule() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let folder = dir.path().join("folder");
    fs::create_dir(&folder)?;
    let op = planned(
        OpKind::Move,
        Some(folder),
        Some(dir.path().join("elsewhere")),
        CollisionPolicy::Rename,
    );
    assert!(execute(&op, &opts()).is_err());
    Ok(())
}

#[test]
fn copy_verify_produces_identical_file_and_keeps_mtime() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("copy").join("dst.bin");
    // ~5 MiB of pseudo-random data spans several copy buffers.
    let payload: Vec<u8> = (0..(5 * 1024 * 1024)).map(|i| (i * 31) as u8).collect();
    fs::write(&src, &payload)?;
    let op = planned(
        OpKind::Copy,
        Some(src.clone()),
        Some(dst.clone()),
        CollisionPolicy::Rename,
    );
    let e = execute(&op, &opts())?;
    assert_eq!(e.bytes as usize, payload.len());
    assert_eq!(fs::read(&dst)?, payload);
    assert!(src.exists(), "copy must not delete source");
    assert_eq!(
        fs::metadata(&src)?.modified()?,
        fs::metadata(&dst)?.modified()?,
        "modification time is preserved so age rules and undo identity keep working"
    );
    assert_eq!(e.source_hash, Some(hash_file(&src)?));
    Ok(())
}

#[test]
fn cancelled_copy_leaves_no_partial_destination() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    fs::write(&src, vec![7u8; 3 * 1024 * 1024])?;
    let cancel = Arc::new(AtomicBool::new(true));
    let op = planned(
        OpKind::Copy,
        Some(src.clone()),
        Some(dst.clone()),
        CollisionPolicy::Rename,
    );
    let r = execute(
        &op,
        &ExecuteOpts {
            cancel: Some(cancel),
            ..opts()
        },
    );
    assert!(matches!(r, Err(Error::Cancelled)));
    assert!(!dst.exists());
    assert!(src.exists());
    Ok(())
}

#[test]
fn progress_reports_bytes_done_against_total() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    fs::write(&src, vec![1u8; 2_500_000])?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let op = planned(OpKind::Copy, Some(src), Some(dst), CollisionPolicy::Rename);
    execute(
        &op,
        &ExecuteOpts {
            progress: Some(Arc::new(move |done, total| {
                sink.lock().unwrap().push((done, total))
            })),
            ..opts()
        },
    )?;
    let seen = seen.lock().unwrap();
    assert!(seen.len() >= 2);
    assert_eq!(*seen.last().unwrap(), (2_500_000, 2_500_000));
    Ok(())
}

#[test]
fn cross_volume_move_verifies_then_removes_the_source() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.bin");
    let dst = dir.path().join("b.bin");
    fs::write(&src, vec![9u8; 1_234_567])?;
    let before = hash_file(&src)?;
    let (bytes, hash) = move_by_copy(&src, &dst, &opts())?;
    assert_eq!(bytes, 1_234_567);
    assert_eq!(hash, before);
    assert_eq!(hash_file(&dst)?, before);
    assert!(!src.exists());
    Ok(())
}

#[test]
fn cross_volume_move_never_clobbers_an_existing_destination() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.bin");
    let dst = dir.path().join("b.bin");
    fs::write(&src, b"new")?;
    fs::write(&dst, b"existing")?;
    assert!(move_by_copy(&src, &dst, &opts()).is_err());
    assert_eq!(fs::read(&dst)?, b"existing");
    assert!(src.exists(), "source is kept when the copy cannot start");
    Ok(())
}

#[test]
fn trash_falls_back_to_the_local_folder_when_os_trash_is_off() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"bye")?;
    let op = planned(
        OpKind::Trash,
        Some(src.clone()),
        None,
        CollisionPolicy::Rename,
    );
    let e = execute(&op, &opts())?;
    assert!(!src.exists());
    let dst = e.destination.unwrap();
    assert!(dst.exists());
    assert!(dst.to_string_lossy().contains(LOCAL_TRASH_DIR));
    Ok(())
}

#[test]
fn trashing_two_files_with_the_same_name_keeps_both() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let a = dir.path().join("a.txt");
    fs::write(&a, b"first")?;
    let first = execute(
        &planned(
            OpKind::Trash,
            Some(a.clone()),
            None,
            CollisionPolicy::Rename,
        ),
        &opts(),
    )?;
    fs::write(&a, b"second")?;
    let second = execute(
        &planned(OpKind::Trash, Some(a), None, CollisionPolicy::Rename),
        &opts(),
    )?;
    assert_eq!(fs::read(first.destination.unwrap())?, b"first");
    assert_eq!(fs::read(second.destination.unwrap())?, b"second");
    Ok(())
}

#[cfg(feature = "os-trash")]
#[test]
fn trash_routes_to_os_trash_when_enabled() -> anyhow::Result<()> {
    // Headless CI runners have no desktop trash; opt out with the env var.
    if std::env::var_os("FILEFLOW_SKIP_OS_TRASH").is_some() {
        return Ok(());
    }
    let dir = TempDir::new()?;
    let src = dir.path().join("os-trash-target.txt");
    fs::write(&src, b"bye")?;
    let op = planned(
        OpKind::Trash,
        Some(src.clone()),
        None,
        CollisionPolicy::Rename,
    );
    let e = execute(
        &op,
        &ExecuteOpts {
            use_os_trash: true,
            ..ExecuteOpts::default()
        },
    )?;
    // Either the OS trash took it (no destination) or the local fallback did.
    assert!(!src.exists(), "source must be gone either way");
    if let Some(dst) = e.destination {
        assert!(dst.to_string_lossy().contains(LOCAL_TRASH_DIR));
    }
    Ok(())
}

#[test]
fn next_available_appends_counter_and_keeps_extension_case() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("x.PDF");
    fs::write(&base, b"a").unwrap();
    let next = next_available(&base);
    assert_eq!(next.file_name().unwrap().to_string_lossy(), "x (1).PDF");
}

#[test]
fn tag_and_review_are_not_filesystem_operations() {
    for kind in [OpKind::Tag, OpKind::Review] {
        let op = planned(
            kind,
            Some(PathBuf::from("x")),
            None,
            CollisionPolicy::Rename,
        );
        assert!(execute(&op, &opts()).is_err());
    }
}
