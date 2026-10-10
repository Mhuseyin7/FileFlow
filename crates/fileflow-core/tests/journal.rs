//! Journal, undo and crash-recovery tests. Everything runs on temp folders.

use fileflow_core::db::Db;
use fileflow_core::fs_ops::{execute, hash_file, hex, ExecuteOpts, Executed, LOCAL_TRASH_DIR};
use fileflow_core::journal::{Journal, OpState, RecoveryOutcome, UndoStatus};
use fileflow_core::model::{CollisionPolicy, OpKind, PlannedOp};
use fileflow_core::Error;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
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

/// Journals and executes an op, the way the runner does.
fn run(journal: &Journal, op: &PlannedOp) -> anyhow::Result<Executed> {
    journal.record_start(op)?;
    let done = execute(op, &opts())?;
    journal.record_success(&done)?;
    Ok(done)
}

fn mv(src: &Path, dst: &Path, policy: CollisionPolicy) -> PlannedOp {
    planned(
        OpKind::Move,
        Some(src.to_path_buf()),
        Some(dst.to_path_buf()),
        policy,
    )
}

#[test]
fn records_and_undoes_a_move() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"hello world")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    run(&journal, &op)?;

    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.state, OpState::Completed);
    assert_eq!(entry.undo_status, UndoStatus::Available);

    journal.undo(&op.id)?;
    assert!(src.exists());
    assert!(!dst.exists());
    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.state, OpState::Undone);
    assert_eq!(entry.undo_status, UndoStatus::Done);
    Ok(())
}

#[test]
fn an_undone_operation_cannot_be_undone_twice() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    fs::write(&src, b"x")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dir.path().join("b.txt"), CollisionPolicy::Rename);
    run(&journal, &op)?;
    journal.undo(&op.id)?;
    assert!(journal.undo(&op.id).is_err());
    Ok(())
}

#[test]
fn undo_refuses_when_the_size_changed() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"one")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    run(&journal, &op)?;
    fs::write(&dst, b"much larger content now")?;
    assert!(journal.undo(&op.id).is_err());
    assert_eq!(
        journal.get(&op.id)?.unwrap().undo_status,
        UndoStatus::Unsafe
    );
    assert!(dst.exists(), "the edited file stays where it is");
    Ok(())
}

#[test]
fn undo_refuses_a_same_size_edit() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"AAAA")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    run(&journal, &op)?;

    // Same length, different content, and a later modification time.
    fs::write(&dst, b"BBBB")?;
    let later = SystemTime::now() + Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&dst)?
        .set_modified(later)?;

    let err = journal.undo(&op.id).unwrap_err();
    assert!(matches!(err, Error::Integrity(_)), "{err}");
    assert!(dst.exists());
    Ok(())
}

#[test]
fn undo_never_overwrites_a_file_that_appeared_at_the_original_path() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"one")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    run(&journal, &op)?;
    fs::write(&src, b"user re-created")?;
    assert!(journal.undo(&op.id).is_err());
    assert_eq!(fs::read(&src)?, b"user re-created");
    assert!(dst.exists());
    Ok(())
}

#[test]
fn a_refused_undo_can_be_retried_once_the_obstacle_is_gone() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dest").join("a.txt");
    fs::write(&src, b"one")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    run(&journal, &op)?;
    fs::write(&src, b"in the way")?;
    assert!(journal.undo(&op.id).is_err());
    fs::remove_file(&src)?;
    journal.undo(&op.id)?;
    assert_eq!(fs::read(&src)?, b"one");
    Ok(())
}

#[test]
fn undoing_a_copy_sets_the_copy_aside_instead_of_deleting_it() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("backup").join("a.txt");
    fs::write(&src, b"data")?;
    let journal = Journal::open_in_memory()?;
    let op = planned(
        OpKind::Copy,
        Some(src.clone()),
        Some(dst.clone()),
        CollisionPolicy::Rename,
    );
    run(&journal, &op)?;
    journal.undo(&op.id)?;
    assert!(src.exists(), "the original is untouched");
    assert!(!dst.exists());
    let trashed = dst.parent().unwrap().join(LOCAL_TRASH_DIR).join("a.txt");
    assert_eq!(fs::read(trashed)?, b"data");
    Ok(())
}

#[test]
fn undoing_a_replace_puts_the_replaced_file_back() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("new.txt");
    let dst = dir.path().join("target.txt");
    fs::write(&src, b"new")?;
    fs::write(&dst, b"precious")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Replace);
    run(&journal, &op)?;
    assert_eq!(fs::read(&dst)?, b"new");

    journal.undo(&op.id)?;
    assert_eq!(fs::read(&src)?, b"new");
    assert_eq!(fs::read(&dst)?, b"precious");
    Ok(())
}

#[test]
fn os_trash_entries_do_not_promise_an_undo() -> anyhow::Result<()> {
    let journal = Journal::open_in_memory()?;
    let op = planned(
        OpKind::Trash,
        Some(PathBuf::from("/somewhere/a.txt")),
        None,
        CollisionPolicy::Rename,
    );
    journal.record_start(&op)?;
    journal.record_success(&Executed {
        op_id: op.id.clone(),
        kind: OpKind::Trash,
        source: op.source.clone(),
        destination: None,
        bytes: 3,
        source_hash: None,
        dest_modified_ns: None,
        replaced: None,
    })?;
    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.undo_status, UndoStatus::NotApplicable);
    assert!(journal.undo(&op.id).is_err());
    Ok(())
}

#[test]
fn skipped_and_failed_operations_are_recorded_with_a_reason() -> anyhow::Result<()> {
    let journal = Journal::open_in_memory()?;
    let a = mv(Path::new("/x/a"), Path::new("/y/a"), CollisionPolicy::Skip);
    let b = mv(
        Path::new("/x/b"),
        Path::new("/y/b"),
        CollisionPolicy::Rename,
    );
    journal.record_start(&a)?;
    journal.record_skipped(&a.id, "destination exists")?;
    journal.record_start(&b)?;
    journal.record_failure(&b.id, &Error::Permission("access is denied".into()))?;

    let a = journal.get(&a.id)?.unwrap();
    assert_eq!(a.state, OpState::Skipped);
    assert_eq!(a.error.as_deref(), Some("destination exists"));
    let b = journal.get(&b.id)?.unwrap();
    assert_eq!(b.state, OpState::Failed);
    assert!(b.error.unwrap().contains("access is denied"));
    Ok(())
}

#[test]
fn the_journal_survives_reopening_the_database() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("fileflow.sqlite");
    let src = dir.path().join("a.txt");
    fs::write(&src, b"x")?;
    let op = mv(&src, &dir.path().join("b.txt"), CollisionPolicy::Rename);
    {
        let journal = Journal::open(&db_path)?;
        run(&journal, &op)?;
    }
    let journal = Journal::open(&db_path)?;
    journal.undo(&op.id)?;
    assert!(src.exists());
    Ok(())
}

// ---- crash recovery -------------------------------------------------------

#[test]
fn recovery_leaves_an_untouched_file_alone_and_does_not_repeat_the_move() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"x")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    journal.record_start(&op)?; // crash before executing

    let report = journal.recover()?;
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].outcome, RecoveryOutcome::NotStarted);
    assert!(
        src.exists() && !dst.exists(),
        "recovery must not re-run the move"
    );
    assert_eq!(journal.get(&op.id)?.unwrap().state, OpState::Failed);
    Ok(())
}

#[test]
fn recovery_completes_the_bookkeeping_of_a_move_that_finished() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"hello")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    journal.record_start(&op)?;
    execute(&op, &opts())?; // crash before record_success

    let report = journal.recover()?;
    assert_eq!(report[0].outcome, RecoveryOutcome::Completed);
    let entry = journal.get(&op.id)?.unwrap();
    assert_eq!(entry.state, OpState::Completed);
    assert_eq!(entry.undo_status, UndoStatus::Available);
    // ... and the recovered entry can be undone like any other.
    journal.undo(&op.id)?;
    assert!(src.exists());
    Ok(())
}

#[test]
fn recovery_flags_but_never_deletes_when_both_files_exist() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"original")?;
    fs::write(&dst, b"partial")?;
    let journal = Journal::open_in_memory()?;
    let op = mv(&src, &dst, CollisionPolicy::Rename);
    journal.record_start(&op)?;

    let report = journal.recover()?;
    assert_eq!(report[0].outcome, RecoveryOutcome::NeedsAttention);
    assert_eq!(fs::read(&src)?, b"original");
    assert_eq!(fs::read(&dst)?, b"partial");
    Ok(())
}

#[test]
fn recovery_checks_a_finished_copy_by_checksum() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let src = dir.path().join("a.bin");
    let good = dir.path().join("good.bin");
    let partial = dir.path().join("partial.bin");
    fs::write(&src, vec![5u8; 4096])?;
    fs::write(&good, vec![5u8; 4096])?;
    fs::write(&partial, vec![5u8; 100])?;
    let journal = Journal::open_in_memory()?;
    let copy_good = planned(
        OpKind::Copy,
        Some(src.clone()),
        Some(good),
        CollisionPolicy::Rename,
    );
    let copy_partial = planned(
        OpKind::Copy,
        Some(src),
        Some(partial.clone()),
        CollisionPolicy::Rename,
    );
    journal.record_start(&copy_good)?;
    journal.record_start(&copy_partial)?;

    let report = journal.recover()?;
    let outcome = |id: &str| report.iter().find(|r| r.op_id == id).unwrap().outcome;
    assert_eq!(outcome(&copy_good.id), RecoveryOutcome::Completed);
    assert_eq!(outcome(&copy_partial.id), RecoveryOutcome::NeedsAttention);
    assert!(
        partial.exists(),
        "a suspicious partial copy is reported, not deleted"
    );
    Ok(())
}

#[test]
fn recovery_finishes_or_reverts_an_interrupted_undo() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let db = Arc::new(Db::open_in_memory()?);
    let journal = Journal::new(db.clone());
    let force_state = |op_id: &str, state: &str| {
        db.lock()
            .execute("update journal set state=?2 where op_id=?1", [op_id, state])
    };

    // Interrupted after the file moved back, before the state update.
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&src, b"x")?;
    let moved_back = mv(&src, &dst, CollisionPolicy::Rename);
    run(&journal, &moved_back)?;
    fs::rename(&dst, &src)?;
    force_state(&moved_back.id, "undoing")?;

    // Interrupted before anything moved back.
    let src2 = dir.path().join("c.txt");
    let dst2 = dir.path().join("d.txt");
    fs::write(&src2, b"y")?;
    let not_started = mv(&src2, &dst2, CollisionPolicy::Rename);
    run(&journal, &not_started)?;
    force_state(&not_started.id, "undoing")?;

    let report = journal.recover()?;
    let outcome = |id: &str| report.iter().find(|r| r.op_id == id).unwrap().outcome;
    assert_eq!(outcome(&moved_back.id), RecoveryOutcome::Completed);
    assert_eq!(journal.get(&moved_back.id)?.unwrap().state, OpState::Undone);
    assert_eq!(outcome(&not_started.id), RecoveryOutcome::NotStarted);
    let back = journal.get(&not_started.id)?.unwrap();
    assert_eq!(back.state, OpState::Completed);
    assert_eq!(back.undo_status, UndoStatus::Available);
    Ok(())
}

#[test]
fn recovery_reports_nothing_when_nothing_was_interrupted() -> anyhow::Result<()> {
    let journal = Journal::open_in_memory()?;
    assert!(journal.recover()?.is_empty());
    Ok(())
}

// ---- schema ----------------------------------------------------------------

#[test]
fn a_v0_1_database_is_migrated_in_place() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let path = dir.path().join("old.sqlite");
    {
        // The schema exactly as v0.1 shipped it, including one row.
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            r#"
            create table meta (key text primary key, value text not null);
            create table journal (
                op_id text primary key, rule_id text not null, rule_name text not null,
                kind text not null, source text, destination text,
                size integer not null default 0, state text not null,
                started_at integer not null, finished_at integer, error text,
                source_hash_hex text, undo_status text not null default 'not_applicable'
            );
            create table review (id text primary key, path text not null,
                reason text not null, added_at integer not null, resolved integer not null default 0);
            insert into journal(op_id, rule_id, rule_name, kind, source, destination, size, state, started_at, undo_status)
                values('old-op', 'r', 'Old rule', 'move', '/a', '/b', 3, 'completed', 1700000000, 'available');
            "#,
        )?;
    }
    let db = Db::open(&path)?;
    let version: i32 = db
        .lock()
        .query_row("PRAGMA user_version", [], |r| r.get(0))?;
    assert_eq!(version, fileflow_core::db::SCHEMA_VERSION);
    let journal = Journal::new(Arc::new(db));
    let entry = journal.get("old-op")?.expect("old rows survive");
    assert_eq!(entry.rule_name, "Old rule");
    assert_eq!(entry.dest_modified_ns, None);
    Ok(())
}

#[test]
fn hash_helpers_agree() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let p = dir.path().join("f");
    fs::write(&p, b"abc")?;
    assert_eq!(
        hex(&hash_file(&p)?),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    Ok(())
}
