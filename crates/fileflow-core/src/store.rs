//! Persistence for everything except the journal: rules, granted folders,
//! the review queue, tags and per-rule schedule state. Shares the
//! connection (and therefore the transactions) with the journal via `Db`.

use crate::db::Db;
use crate::error::{Error, Result};
use crate::model::Rule;
use rusqlite::params;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem {
    pub id: String,
    pub path: PathBuf,
    pub rule_id: String,
    pub rule_name: String,
    pub kind: String,
    pub reason: String,
    pub added_at: i64,
}

pub struct Store {
    db: Arc<Db>,
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

impl Store {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    // ---- rules -------------------------------------------------------

    pub fn list_rules(&self) -> Result<Vec<Rule>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("select json from rules order by priority, name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            // A rule that no longer parses is skipped rather than blocking
            // the whole app from starting.
            if let Ok(rule) = serde_json::from_str::<Rule>(&json?) {
                out.push(rule);
            }
        }
        Ok(out)
    }

    pub fn save_rule(&self, rule: &Rule) -> Result<()> {
        let json = serde_json::to_string(rule).map_err(|e| Error::Other(e.to_string()))?;
        self.db.lock().execute(
            r#"insert into rules(id, name, priority, enabled, profile, json, updated_at)
               values(?1, ?2, ?3, ?4, ?5, ?6, ?7)
               on conflict(id) do update set name=?2, priority=?3, enabled=?4,
                 profile=?5, json=?6, updated_at=?7"#,
            params![
                rule.id,
                rule.name,
                rule.priority,
                rule.enabled,
                rule.profile,
                json,
                now()
            ],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("delete from rules where id=?1", [id])?;
        conn.execute("delete from last_runs where rule_id=?1", [id])?;
        Ok(())
    }

    // ---- granted folders ----------------------------------------------

    pub fn list_folders(&self) -> Result<Vec<PathBuf>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("select path from folders order by path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(PathBuf::from(r?))).collect()
    }

    /// Replaces the granted set in one transaction.
    pub fn set_folders(&self, folders: &[PathBuf]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute("delete from folders", [])?;
        for f in folders {
            tx.execute(
                "insert or ignore into folders(path, monitored, added_at) values(?1, 1, ?2)",
                params![f.to_string_lossy(), now()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---- review queue ---------------------------------------------------

    pub fn add_review(
        &self,
        path: &Path,
        rule_id: &str,
        rule_name: &str,
        kind: &str,
        reason: &str,
    ) -> Result<()> {
        let conn = self.db.lock();
        // One open item per file and rule; re-running does not pile up.
        let exists: bool = conn.query_row(
            "select exists(select 1 from review where path=?1 and rule_id=?2 and resolved=0)",
            params![path.to_string_lossy(), rule_id],
            |r| r.get(0),
        )?;
        if !exists {
            conn.execute(
                "insert into review(id, path, rule_id, rule_name, kind, reason, added_at)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    path.to_string_lossy(),
                    rule_id,
                    rule_name,
                    kind,
                    reason,
                    now()
                ],
            )?;
        }
        Ok(())
    }

    pub fn list_review(&self) -> Result<Vec<ReviewItem>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "select id, path, rule_id, rule_name, kind, reason, added_at
             from review where resolved=0 order by added_at desc",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ReviewItem {
                id: r.get(0)?,
                path: PathBuf::from(r.get::<_, String>(1)?),
                rule_id: r.get(2)?,
                rule_name: r.get(3)?,
                kind: r.get(4)?,
                reason: r.get(5)?,
                added_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn resolve_review(&self, id: &str, resolution: &str) -> Result<()> {
        self.db.lock().execute(
            "update review set resolved=1, resolution=?2 where id=?1",
            params![id, resolution],
        )?;
        Ok(())
    }

    // ---- tags -------------------------------------------------------------

    pub fn add_tags(&self, path: &Path, tags: &[String]) -> Result<()> {
        let conn = self.db.lock();
        for t in tags {
            conn.execute(
                "insert or ignore into file_tags(path, tag, added_at) values(?1, ?2, ?3)",
                params![path.to_string_lossy(), t, now()],
            )?;
        }
        Ok(())
    }

    pub fn tags_for(&self, path: &Path) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("select tag from file_tags where path=?1 order by tag")?;
        let rows = stmt.query_map([path.to_string_lossy()], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- schedule state ---------------------------------------------------

    pub fn last_run(&self, rule_id: &str) -> Result<Option<i64>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("select ran_at from last_runs where rule_id=?1")?;
        let mut rows = stmt.query([rule_id])?;
        Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
    }

    pub fn mark_run(&self, rule_id: &str) -> Result<()> {
        self.db.lock().execute(
            "insert into last_runs(rule_id, ran_at) values(?1, ?2)
             on conflict(rule_id) do update set ran_at=?2",
            params![rule_id, now()],
        )?;
        Ok(())
    }
}

impl crate::scheduler::LastRunStore for Arc<Store> {
    fn last_run(&self, rule_id: &str) -> Option<OffsetDateTime> {
        Store::last_run(self, rule_id)
            .ok()
            .flatten()
            .and_then(|t| OffsetDateTime::from_unix_timestamp(t).ok())
    }

    fn record_run(&self, rule_id: &str, _at: OffsetDateTime) {
        let _ = self.mark_run(rule_id);
    }
}
