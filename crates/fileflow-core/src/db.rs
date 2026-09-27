//! SQLite schema. Everything FileFlow persists lives here.
//!
//! Tables:
//!
//! * `rules`      — rule definitions (serialized as JSON in one column)
//! * `folders`    — user-granted sandbox roots
//! * `journal`    — action journal (see journal.rs)
//! * `settings`   — key/value store for app settings
//! * `review`     — queue of files awaiting user approval
//! * `meta`       — schema version, first-run timestamp
//!
//! We never store file *contents* — only paths and optional pre-hash for undo
//! identity checks.

use crate::error::Result;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i32 = 1;

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        create table if not exists meta (
            key   text primary key,
            value text not null
        );

        create table if not exists rules (
            id       text primary key,
            name     text not null,
            priority integer not null default 100,
            enabled  integer not null default 0,
            profile  text,
            json     text not null,
            updated_at integer not null
        );

        create table if not exists folders (
            path       text primary key,
            monitored  integer not null default 1,
            added_at   integer not null
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

        create table if not exists review (
            id         text primary key,
            path       text not null,
            reason     text not null,
            added_at   integer not null,
            resolved   integer not null default 0
        );
        "#,
    )?;
    conn.execute(
        "insert into meta(key, value) values('schema_version', ?1)
         on conflict(key) do update set value=excluded.value",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}
