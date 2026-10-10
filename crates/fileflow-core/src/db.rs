//! SQLite access. Everything FileFlow persists lives in one database:
//!
//! * `rules`      — rule definitions (JSON in one column)
//! * `folders`    — user-granted sandbox roots
//! * `journal`    — action journal (see journal.rs)
//! * `settings`   — key/value app settings
//! * `review`     — files waiting for a human decision
//! * `file_tags`  — tags applied by the Tag action
//! * `last_runs`  — when each scheduled rule last ran
//!
//! We never store file *contents* — only paths, sizes, timestamps and the
//! optional SHA-256 used for undo identity checks.
//!
//! The schema is versioned with `PRAGMA user_version`; each migration runs
//! once, inside a transaction.

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub const SCHEMA_VERSION: i32 = 2;

/// One connection behind a mutex, shared (via `Arc`) by the journal and the
/// store so they see the same transactions.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self::init(conn)?;
        restrict_permissions(path);
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Durability matters more than speed here: the journal entry for an
        // operation is written *before* the file is touched.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch(
            "PRAGMA synchronous = FULL; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;",
        )?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Locks the connection. A poisoned lock only means another thread
    /// panicked mid-query; SQLite itself is still consistent.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The database holds file paths, so keep it private to the user.
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn migrate(conn: &Connection) -> Result<()> {
    let mut version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    while version < SCHEMA_VERSION {
        let tx = conn.unchecked_transaction()?;
        match version {
            0 => v1(&tx)?,
            1 => v2(&tx)?,
            _ => unreachable!("no migration from version {version}"),
        }
        version += 1;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// The original schema (v0.1). `if not exists` keeps databases that were
/// created before versioning working.
fn v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        create table if not exists rules (
            id         text primary key,
            name       text not null,
            priority   integer not null default 100,
            enabled    integer not null default 0,
            profile    text,
            json       text not null,
            updated_at integer not null
        );

        create table if not exists folders (
            path      text primary key,
            monitored integer not null default 1,
            added_at  integer not null
        );

        create table if not exists journal (
            op_id           text primary key,
            rule_id         text not null,
            rule_name       text not null,
            kind            text not null,
            source          text,
            destination     text,
            size            integer not null default 0,
            state           text not null,
            started_at      integer not null,
            finished_at     integer,
            error           text,
            source_hash_hex text,
            undo_status     text not null default 'not_applicable'
        );
        create index if not exists journal_started on journal(started_at desc);
        create index if not exists journal_rule on journal(rule_id);

        create table if not exists settings (
            key   text primary key,
            value text not null
        );
        "#,
    )?;
    Ok(())
}

/// Identity checks for undo, the review queue, tags and schedule state.
fn v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        alter table journal add column dest_modified_ns integer;
        alter table journal add column replaced_path text;

        drop table if exists meta;
        drop table if exists review;

        create table review (
            id         text primary key,
            path       text not null,
            rule_id    text not null,
            rule_name  text not null,
            kind       text not null,
            reason     text not null,
            op_json    text,
            added_at   integer not null,
            resolved   integer not null default 0,
            resolution text
        );
        create index review_open on review(resolved, added_at desc);

        create table file_tags (
            path     text not null,
            tag      text not null,
            added_at integer not null,
            primary key (path, tag)
        );

        create table last_runs (
            rule_id text primary key,
            ran_at  integer not null
        );
        "#,
    )?;
    Ok(())
}
