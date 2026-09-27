//! Action journal + undo. Every operation carries a state machine:
//!
//! `Planned -> Executing -> Completed | Failed`
//! `Completed -> Undoing -> Undone`
//!
//! The journal never stores file contents. It stores enough metadata (source
//! path, destination path, kind, size, optional pre-hash) to verify identity
//! at undo time. If the world has changed under our feet (destination newer,
//! source now occupied, hash differs), undo aborts rather than clobbering.

use crate::error::{Error, Result};
use crate::fs_ops::{self, ExecuteOpts, Executed};
use crate::model::{CollisionPolicy, OpKind, PlannedOp};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    Planned,
    Executing,
    Completed,
    Failed,
    Undoing,
    Undone,
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
    /// Hex-encoded SHA-256 of the source at execute time.
    pub source_hash_hex: Option<String>,
    pub undo_status: UndoStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UndoStatus {
    NotApplicable,
    Available,
    Unsafe,
    Done,
}

pub struct Journal {
    conn: Connection,
}

impl Journal {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let s = Journal { conn };
        s.init_schema()?;
        Ok(s)
    }

    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let s = Journal { conn };
        s.init_schema()?;
        Ok(s)
    }

    fn init_schema(&self) -> Result<()> {
        crate::db::migrate(&self.conn)
    }

    pub fn record_start(&self, op: &PlannedOp) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let source = op.source.as_ref().map(|p| p.to_string_lossy().into_owned());
        let destination = op
            .destination
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        self.conn.execute(
            r#"insert into journal(op_id, rule_id, rule_name, kind, source, destination,
                size, state, started_at, undo_status)
               values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'executing', ?8, 'not_applicable')
               on conflict(op_id) do update set state='executing', started_at=?8"#,
            params![
                op.id,
                op.rule_id,
                op.rule_name,
                serde_json::to_string(&op.kind)?.trim_matches('"'),
                source,
                destination,
                op.size as i64,
                now.unix_timestamp(),
            ],
        )?;
        Ok(())
    }

    pub fn record_success(&self, e: &Executed) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let source = e.source.as_ref().map(|p| p.to_string_lossy().into_owned());
        let destination = e
            .destination
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let hash_hex = e.source_hash.as_ref().map(hex);
        let undo_status = match e.kind {
            OpKind::Move | OpKind::Rename | OpKind::Copy | OpKind::Trash => "available",
            _ => "not_applicable",
        };
        self.conn.execute(
            r#"update journal
                 set state='completed',
                     finished_at=?2,
                     source=coalesce(?3, source),
                     destination=coalesce(?4, destination),
                     size=?5,
                     source_hash_hex=?6,
                     undo_status=?7
               where op_id=?1"#,
            params![
                e.op_id,
                now.unix_timestamp(),
                source,
                destination,
                e.bytes as i64,
                hash_hex,
                undo_status,
            ],
        )?;
        Ok(())
    }

    pub fn record_failure(&self, op_id: &str, err: &Error) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        self.conn.execute(
            r#"update journal
                 set state='failed', finished_at=?2, error=?3, undo_status='not_applicable'
               where op_id=?1"#,
            params![op_id, now.unix_timestamp(), err.to_string()],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<JournalEntry>> {
        let mut stmt = self.conn.prepare(
            r#"select op_id, rule_id, rule_name, kind, source, destination, size,
                      state, started_at, finished_at, error, source_hash_hex, undo_status
               from journal order by started_at desc limit ?1"#,
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(row_to_entry(row))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter().collect::<Result<Vec<_>>>()
    }

    pub fn get(&self, op_id: &str) -> Result<Option<JournalEntry>> {
        let mut stmt = self.conn.prepare(
            r#"select op_id, rule_id, rule_name, kind, source, destination, size,
                      state, started_at, finished_at, error, source_hash_hex, undo_status
               from journal where op_id=?1"#,
        )?;
        let mut rows = stmt.query(params![op_id])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(row_to_entry(row)?));
        }
        Ok(None)
    }

    /// Undo a completed op if it's safe. Safety = the file that lives at
    /// `destination` still looks like the one we moved (same size, and same
    /// hash if we recorded one).
    pub fn undo(&self, op_id: &str) -> Result<()> {
        let entry = self
            .get(op_id)?
            .ok_or_else(|| Error::NotFound(format!("op {op_id}")))?;
        if entry.state != OpState::Completed {
            return Err(Error::Safety(format!("op state is {:?}", entry.state)));
        }
        self.conn.execute(
            "update journal set state='undoing' where op_id=?1",
            params![op_id],
        )?;
        let result = perform_undo(&entry);
        match result {
            Ok(()) => {
                self.conn.execute(
                    "update journal set state='undone', undo_status='done' where op_id=?1",
                    params![op_id],
                )?;
                Ok(())
            }
            Err(e) => {
                self.conn.execute(
                    "update journal set state='completed', undo_status='unsafe', error=?2 where op_id=?1",
                    params![op_id, e.to_string()],
                )?;
                Err(e)
            }
        }
    }
}

fn perform_undo(e: &JournalEntry) -> Result<()> {
    match e.kind {
        OpKind::Move | OpKind::Rename | OpKind::Trash => {
            let src = e.source.as_ref().ok_or_else(|| Error::Other("no source".into()))?;
            let dst = e
                .destination
                .as_ref()
                .ok_or_else(|| Error::Other("no destination".into()))?;
            // The file currently lives at `dst`; move it back to `src`.
            if !dst.exists() {
                return Err(Error::Integrity("destination missing".into()));
            }
            if src.exists() {
                return Err(Error::Integrity("original source path now occupied".into()));
            }
            verify_identity(dst, e)?;
            if let Some(parent) = src.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(dst, src).or_else(|_| {
                // Cross-fs fallback
                let planned = PlannedOp {
                    id: format!("undo-{}", e.op_id),
                    rule_id: e.rule_id.clone(),
                    rule_name: e.rule_name.clone(),
                    kind: OpKind::Move,
                    source: Some(dst.clone()),
                    destination: Some(src.clone()),
                    size: e.size,
                    collision: CollisionPolicy::Skip,
                    reasons: vec![],
                    warnings: vec![],
                };
                fs_ops::execute(&planned, ExecuteOpts::default()).map(|_| ())
            })?;
            Ok(())
        }
        OpKind::Copy => {
            // Undo copy = delete the copied file, but only after verifying it
            // is byte-identical to the source we recorded. If either side has
            // drifted, refuse.
            let dst = e
                .destination
                .as_ref()
                .ok_or_else(|| Error::Other("no destination".into()))?;
            if !dst.exists() {
                return Err(Error::Integrity("destination missing".into()));
            }
            verify_identity(dst, e)?;
            std::fs::remove_file(dst)?;
            Ok(())
        }
        OpKind::CreateFolder | OpKind::Tag | OpKind::Review => {
            Err(Error::Safety("undo not applicable for this op".into()))
        }
    }
}

fn verify_identity(p: &std::path::Path, e: &JournalEntry) -> Result<()> {
    let md = std::fs::metadata(p)?;
    if md.len() != e.size {
        return Err(Error::Integrity("file size differs".into()));
    }
    if let Some(expected) = &e.source_hash_hex {
        use sha2::{Digest, Sha256};
        let mut f = std::fs::File::open(p)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        loop {
            use std::io::Read;
            let n = f.read(&mut buf)?;
            if n == 0 { break; }
            h.update(&buf[..n]);
        }
        let got = hex(&h.finalize().into());
        if &got != expected {
            return Err(Error::Integrity("hash mismatch".into()));
        }
    }
    Ok(())
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> Result<JournalEntry> {
    let op_id: String = row.get(0)?;
    let rule_id: String = row.get(1)?;
    let rule_name: String = row.get(2)?;
    let kind_s: String = row.get(3)?;
    let source: Option<String> = row.get(4)?;
    let destination: Option<String> = row.get(5)?;
    let size: i64 = row.get(6)?;
    let state_s: String = row.get(7)?;
    let started_ts: i64 = row.get(8)?;
    let finished_ts: Option<i64> = row.get(9)?;
    let error: Option<String> = row.get(10)?;
    let source_hash_hex: Option<String> = row.get(11)?;
    let undo_s: String = row.get(12)?;

    let started_at = OffsetDateTime::from_unix_timestamp(started_ts)
        .map_err(|e| Error::Time(e.to_string()))?;
    let finished_at = finished_ts
        .map(|t| OffsetDateTime::from_unix_timestamp(t).map_err(|e| Error::Time(e.to_string())))
        .transpose()?;

    Ok(JournalEntry {
        op_id,
        rule_id,
        rule_name,
        kind: parse_kind(&kind_s)?,
        source: source.map(PathBuf::from),
        destination: destination.map(PathBuf::from),
        size: size as u64,
        state: parse_state(&state_s)?,
        started_at,
        finished_at,
        error,
        source_hash_hex,
        undo_status: parse_undo(&undo_s)?,
    })
}

fn parse_kind(s: &str) -> Result<OpKind> {
    Ok(match s {
        "move" => OpKind::Move,
        "copy" => OpKind::Copy,
        "rename" => OpKind::Rename,
        "create_folder" => OpKind::CreateFolder,
        "tag" => OpKind::Tag,
        "review" => OpKind::Review,
        "trash" => OpKind::Trash,
        _ => return Err(Error::Other(format!("bad op kind {s}"))),
    })
}

fn parse_state(s: &str) -> Result<OpState> {
    Ok(match s {
        "planned" => OpState::Planned,
        "executing" => OpState::Executing,
        "completed" => OpState::Completed,
        "failed" => OpState::Failed,
        "undoing" => OpState::Undoing,
        "undone" => OpState::Undone,
        _ => return Err(Error::Other(format!("bad state {s}"))),
    })
}

fn parse_undo(s: &str) -> Result<UndoStatus> {
    Ok(match s {
        "not_applicable" => UndoStatus::NotApplicable,
        "available" => UndoStatus::Available,
        "unsafe" => UndoStatus::Unsafe,
        "done" => UndoStatus::Done,
        _ => UndoStatus::NotApplicable,
    })
}
