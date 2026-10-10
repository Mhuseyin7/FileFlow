//! Action journal + undo. Every operation carries a state machine:
//!
//! `Executing -> Completed | Failed | Skipped`
//! `Completed -> Undoing -> Undone`
//!
//! The entry is written *before* the file is touched, so a crash always
//! leaves a trace; [`Journal::recover`] reconciles those traces with what is
//! actually on disk at the next start and never repeats an operation.
//!
//! The journal never stores file contents. It stores enough metadata
//! (paths, size, modification time, optional SHA-256) to verify identity at
//! undo time. If the world has changed under our feet — the file was edited,
//! the original location is occupied — undo refuses rather than clobbering.

use crate::db::Db;
use crate::error::{Error, Result};
use crate::fs_ops::{self, hash_file, hex, modified_ns, rename_no_clobber, ExecuteOpts, Executed};
use crate::model::{OpKind, PlannedOp};
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    Executing,
    Completed,
    Failed,
    /// Nothing was done because the collision policy said to leave it.
    Skipped,
    Undoing,
    Undone,
}

impl OpState {
    pub fn as_str(self) -> &'static str {
        match self {
            OpState::Executing => "executing",
            OpState::Completed => "completed",
            OpState::Failed => "failed",
            OpState::Skipped => "skipped",
            OpState::Undoing => "undoing",
            OpState::Undone => "undone",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "executing" => OpState::Executing,
            "completed" => OpState::Completed,
            "failed" => OpState::Failed,
            "skipped" => OpState::Skipped,
            "undoing" => OpState::Undoing,
            "undone" => OpState::Undone,
            _ => return Err(Error::Other(format!("unknown journal state '{s}'"))),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UndoStatus {
    NotApplicable,
    Available,
    /// The last undo attempt was refused because the files no longer match.
    Unsafe,
    Done,
}

impl UndoStatus {
    fn as_str(self) -> &'static str {
        match self {
            UndoStatus::NotApplicable => "not_applicable",
            UndoStatus::Available => "available",
            UndoStatus::Unsafe => "unsafe",
            UndoStatus::Done => "done",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "available" => UndoStatus::Available,
            "unsafe" => UndoStatus::Unsafe,
            "done" => UndoStatus::Done,
            _ => UndoStatus::NotApplicable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub op_id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub kind: OpKind,
    pub source: Option<PathBuf>,
    pub destination: Option<PathBuf>,
    pub size: u64,
    pub state: OpState,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    pub error: Option<String>,
    /// Hex-encoded SHA-256 of the file, when one was computed.
    pub source_hash_hex: Option<String>,
    pub undo_status: UndoStatus,
    /// Modification time of the destination right after the operation.
    pub dest_modified_ns: Option<i64>,
    /// Where a destination displaced by `Replace` was put.
    pub replaced: Option<PathBuf>,
}

/// What recovery concluded about an operation that was interrupted.
#[derive(Debug, Clone, Serialize)]
pub struct Recovery {
    pub op_id: String,
    pub path: Option<PathBuf>,
    pub outcome: RecoveryOutcome,
    pub note: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryOutcome {
    /// The interrupted operation had not changed anything.
    NotStarted,
    /// It had finished; only the bookkeeping was missing.
    Completed,
    /// The disk is in a state a human should look at. Nothing was deleted.
    NeedsAttention,
}

const COLS: &str = "op_id, rule_id, rule_name, kind, source, destination, size, state, \
                    started_at, finished_at, error, source_hash_hex, undo_status, \
                    dest_modified_ns, replaced_path";

pub struct Journal {
    db: Arc<Db>,
}

impl Journal {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self::new(Arc::new(Db::open_in_memory()?)))
    }

    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self::new(Arc::new(Db::open(path)?)))
    }

    pub fn record_start(&self, op: &PlannedOp) -> Result<()> {
        self.db.lock().execute(
            r#"insert into journal(op_id, rule_id, rule_name, kind, source, destination,
                                   size, state, started_at, undo_status)
               values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'executing', ?8, 'not_applicable')
               on conflict(op_id) do update
                 set state='executing', started_at=?8, finished_at=null, error=null"#,
            params![
                op.id,
                op.rule_id,
                op.rule_name,
                op.kind.as_str(),
                path_text(op.source.as_deref()),
                path_text(op.destination.as_deref()),
                op.size as i64,
                now_ts(),
            ],
        )?;
        Ok(())
    }

    pub fn record_success(&self, e: &Executed) -> Result<()> {
        // OS-trash and bookkeeping operations have no destination to restore
        // from, so promising an undo would be a lie.
        let undoable = e.destination.is_some()
            && matches!(
                e.kind,
                OpKind::Move | OpKind::Rename | OpKind::Copy | OpKind::Trash
            );
        let undo = if undoable {
            UndoStatus::Available
        } else {
            UndoStatus::NotApplicable
        };
        self.db.lock().execute(
            r#"update journal
                 set state='completed', finished_at=?2,
                     source=coalesce(?3, source), destination=coalesce(?4, destination),
                     size=?5, source_hash_hex=?6, undo_status=?7,
                     dest_modified_ns=?8, replaced_path=?9, error=null
               where op_id=?1"#,
            params![
                e.op_id,
                now_ts(),
                path_text(e.source.as_deref()),
                path_text(e.destination.as_deref()),
                e.bytes as i64,
                e.source_hash.as_ref().map(hex),
                undo.as_str(),
                e.dest_modified_ns,
                path_text(e.replaced.as_deref()),
            ],
        )?;
        Ok(())
    }

    pub fn record_failure(&self, op_id: &str, err: &Error) -> Result<()> {
        self.finish(op_id, OpState::Failed, &err.to_string())
    }

    pub fn record_skipped(&self, op_id: &str, reason: &str) -> Result<()> {
        self.finish(op_id, OpState::Skipped, reason)
    }

    fn finish(&self, op_id: &str, state: OpState, note: &str) -> Result<()> {
        self.db.lock().execute(
            "update journal set state=?2, finished_at=?3, error=?4, undo_status='not_applicable'
             where op_id=?1",
            params![op_id, state.as_str(), now_ts(), note],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<JournalEntry>> {
        self.query(
            "order by started_at desc, rowid desc limit ?1",
            params![limit as i64],
        )
    }

    pub fn get(&self, op_id: &str) -> Result<Option<JournalEntry>> {
        Ok(self
            .query("where op_id = ?1", params![op_id])?
            .into_iter()
            .next())
    }

    fn query(&self, tail: &str, p: impl rusqlite::Params) -> Result<Vec<JournalEntry>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!("select {COLS} from journal {tail}"))?;
        let rows = stmt.query_map(p, |row| Ok(row_to_entry(row)))?;
        rows.map(|r| r?).collect()
    }

    /// Undo a completed op if it is safe. Safe means the file at the
    /// destination still looks like the one FileFlow put there (size,
    /// modification time and, when recorded, SHA-256) and the original
    /// location is still free. A refused undo can be retried.
    pub fn undo(&self, op_id: &str) -> Result<()> {
        let entry = self
            .get(op_id)?
            .ok_or_else(|| Error::NotFound(format!("operation {op_id}")))?;
        if entry.state != OpState::Completed {
            return Err(Error::Safety(format!(
                "this operation is {}; only completed operations can be undone",
                entry.state.as_str()
            )));
        }
        if !matches!(
            entry.undo_status,
            UndoStatus::Available | UndoStatus::Unsafe
        ) {
            return Err(Error::Safety(
                "undo is not available for this operation".into(),
            ));
        }
        self.set_state(op_id, OpState::Undoing, UndoStatus::Available, None)?;
        match perform_undo(&entry) {
            Ok(()) => self.set_state(op_id, OpState::Undone, UndoStatus::Done, None),
            Err(e) => {
                self.set_state(
                    op_id,
                    OpState::Completed,
                    UndoStatus::Unsafe,
                    Some(&e.to_string()),
                )?;
                Err(e)
            }
        }
    }

    fn set_state(
        &self,
        op_id: &str,
        state: OpState,
        undo: UndoStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.db.lock().execute(
            "update journal set state=?2, undo_status=?3, error=?4 where op_id=?1",
            params![op_id, state.as_str(), undo.as_str(), error],
        )?;
        Ok(())
    }

    /// Reconciles operations that were interrupted (crash, power loss,
    /// force-quit) with what is on disk. Nothing is repeated and nothing is
    /// deleted: an interrupted operation is either recorded as not started,
    /// recorded as completed, or flagged for a human.
    pub fn recover(&self) -> Result<Vec<Recovery>> {
        let stuck = self.query(
            "where state in ('executing', 'undoing') order by started_at, rowid",
            [],
        )?;
        let mut out = Vec::with_capacity(stuck.len());
        for e in stuck {
            out.push(match e.state {
                OpState::Undoing => self.recover_undo(&e)?,
                _ => self.recover_forward(&e)?,
            });
        }
        Ok(out)
    }

    fn recover_forward(&self, e: &JournalEntry) -> Result<Recovery> {
        let src = present(e.source.as_deref());
        let dst = present(e.destination.as_deref());
        let (outcome, note) = match (e.kind, src, dst) {
            (OpKind::Move | OpKind::Rename, true, false) | (OpKind::Copy, true, false) => (
                RecoveryOutcome::NotStarted,
                "interrupted before anything changed; the file is still at its original location",
            ),
            (OpKind::Move | OpKind::Rename, false, true) => {
                (RecoveryOutcome::Completed, "the move had finished")
            }
            (OpKind::Move | OpKind::Rename, true, true) => (
                RecoveryOutcome::NeedsAttention,
                "both the original and the destination exist; nothing was deleted, please check them",
            ),
            (OpKind::Copy, true, true) => {
                let same = match (e.source.as_deref(), e.destination.as_deref()) {
                    (Some(s), Some(d)) => hash_file(s).ok().zip(hash_file(d).ok()).is_some_and(|(a, b)| a == b),
                    _ => false,
                };
                if same {
                    (RecoveryOutcome::Completed, "the copy had finished")
                } else {
                    (
                        RecoveryOutcome::NeedsAttention,
                        "a partial copy may remain at the destination; the original is intact",
                    )
                }
            }
            (OpKind::Trash, true, _) => (
                RecoveryOutcome::NotStarted,
                "interrupted before the file was trashed",
            ),
            (OpKind::Trash, false, _) => {
                (RecoveryOutcome::Completed, "the file had been trashed")
            }
            (OpKind::CreateFolder, _, true) => {
                (RecoveryOutcome::Completed, "the folder exists")
            }
            (OpKind::CreateFolder, _, false) | (OpKind::Tag | OpKind::Review, _, _) => (
                RecoveryOutcome::NotStarted,
                "interrupted before it was recorded",
            ),
            _ => (
                RecoveryOutcome::NeedsAttention,
                "the file is missing from both locations",
            ),
        };

        match outcome {
            RecoveryOutcome::Completed => {
                let dest = e.destination.clone();
                self.record_success(&Executed {
                    op_id: e.op_id.clone(),
                    kind: e.kind,
                    source: e.source.clone(),
                    bytes: dest
                        .as_deref()
                        .and_then(|d| fs::metadata(d).ok())
                        .map_or(e.size, |m| m.len()),
                    dest_modified_ns: dest.as_deref().and_then(modified_ns),
                    destination: dest,
                    source_hash: None,
                    replaced: None,
                })?;
            }
            _ => self.finish(&e.op_id, OpState::Failed, &format!("interrupted: {note}"))?,
        }
        Ok(Recovery {
            op_id: e.op_id.clone(),
            path: e.source.clone().or_else(|| e.destination.clone()),
            outcome,
            note: note.into(),
        })
    }

    fn recover_undo(&self, e: &JournalEntry) -> Result<Recovery> {
        let src = present(e.source.as_deref());
        let dst = present(e.destination.as_deref());
        // For a copy the original always exists; only the copy tells us
        // whether the undo got as far as trashing it.
        let undone = match e.kind {
            OpKind::Copy => !dst,
            _ => src && !dst,
        };
        let not_undone = dst && (e.kind == OpKind::Copy || !src);
        let (outcome, note) = if undone {
            self.set_state(&e.op_id, OpState::Undone, UndoStatus::Done, None)?;
            (RecoveryOutcome::Completed, "the undo had finished")
        } else if not_undone {
            self.set_state(&e.op_id, OpState::Completed, UndoStatus::Available, None)?;
            (RecoveryOutcome::NotStarted, "the undo had not started")
        } else {
            self.set_state(
                &e.op_id,
                OpState::Completed,
                UndoStatus::Unsafe,
                Some("an undo was interrupted and the files are in an unexpected state"),
            )?;
            (
                RecoveryOutcome::NeedsAttention,
                "an undo was interrupted and the files are in an unexpected state",
            )
        };
        Ok(Recovery {
            op_id: e.op_id.clone(),
            path: e.source.clone(),
            outcome,
            note: note.into(),
        })
    }
}

fn perform_undo(e: &JournalEntry) -> Result<()> {
    let dst = e.destination.as_deref().ok_or_else(|| {
        Error::Safety("this file went to the system trash; restore it from there".into())
    })?;
    if !present(Some(dst)) {
        return Err(Error::Integrity(format!(
            "the file is no longer at {}",
            dst.display()
        )));
    }
    verify_identity(dst, e)?;

    match e.kind {
        OpKind::Move | OpKind::Rename | OpKind::Trash => {
            let src = e
                .source
                .as_deref()
                .ok_or_else(|| Error::Other("the journal has no original location".into()))?;
            restore(dst, src)?;
        }
        // The copy is verified to be the one FileFlow made, then set aside in
        // the local trash rather than deleted.
        OpKind::Copy => {
            fs_ops::move_into_local_trash(dst)?;
        }
        OpKind::CreateFolder | OpKind::Tag | OpKind::Review => {
            return Err(Error::Safety(
                "undo is not available for this operation".into(),
            ))
        }
    }

    // A file displaced by `Replace` goes back where it was.
    if let Some(displaced) = e.replaced.as_deref().filter(|p| present(Some(p))) {
        rename_no_clobber(displaced, dst).map_err(|err| {
            Error::Integrity(format!(
                "the operation was undone, but the file it had replaced could not be put back from {}: {err}",
                displaced.display()
            ))
        })?;
    }
    Ok(())
}

/// Moves `from` back to `to` without ever replacing something at `to`.
fn restore(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match rename_no_clobber(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(Error::Integrity(format!(
            "something else is now at the original location {}; nothing was changed",
            to.display()
        ))),
        Err(e) if fs_ops::is_cross_device(&e) => {
            fs_ops::move_by_copy(from, to, &ExecuteOpts::default()).map(|_| ())
        }
        Err(e) => Err(e.into()),
    }
}

fn verify_identity(p: &Path, e: &JournalEntry) -> Result<()> {
    let md = fs::metadata(p)?;
    if md.len() != e.size {
        return Err(Error::Integrity(
            "the file size has changed since the move".into(),
        ));
    }
    if let Some(expected) = e.dest_modified_ns {
        if modified_ns(p) != Some(expected) {
            return Err(Error::Integrity(
                "the file was modified after FileFlow moved it".into(),
            ));
        }
    }
    if let Some(expected) = &e.source_hash_hex {
        if &hex(&hash_file(p)?) != expected {
            return Err(Error::Integrity(
                "the file content has changed since the move".into(),
            ));
        }
    }
    Ok(())
}

fn present(p: Option<&Path>) -> bool {
    p.is_some_and(|p| p.symlink_metadata().is_ok())
}

fn path_text(p: Option<&Path>) -> Option<String> {
    p.map(|p| p.to_string_lossy().into_owned())
}

fn now_ts() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn ts(secs: i64) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(secs).map_err(|e| Error::Time(e.to_string()))
}

fn row_to_entry(row: &Row<'_>) -> Result<JournalEntry> {
    let kind: String = row.get(3)?;
    let state: String = row.get(7)?;
    let undo: String = row.get(12)?;
    Ok(JournalEntry {
        op_id: row.get(0)?,
        rule_id: row.get(1)?,
        rule_name: row.get(2)?,
        kind: OpKind::parse(&kind)
            .ok_or_else(|| Error::Other(format!("unknown journal operation '{kind}'")))?,
        source: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
        destination: row.get::<_, Option<String>>(5)?.map(PathBuf::from),
        size: row.get::<_, i64>(6)? as u64,
        state: OpState::parse(&state)?,
        started_at: ts(row.get(8)?)?,
        finished_at: row.get::<_, Option<i64>>(9)?.map(ts).transpose()?,
        error: row.get(10)?,
        source_hash_hex: row.get(11)?,
        undo_status: UndoStatus::parse(&undo),
        dest_modified_ns: row.get(13)?,
        replaced: row.get::<_, Option<String>>(14)?.map(PathBuf::from),
    })
}
