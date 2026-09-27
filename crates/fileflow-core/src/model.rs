//! Core data model. Everything here is serde-friendly so it can cross the
//! Tauri IPC boundary and be persisted in SQLite without a translation layer.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::category::Category;

/// Normalized description of a file at a point in time. All rule evaluation
/// runs against this — never against raw filesystem events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub path: PathBuf,
    pub parent: PathBuf,
    pub name: String,
    pub stem: String,
    pub extension: String, // lowercase, no dot; "" if none
    pub size: u64,
    pub is_hidden: bool,
    pub is_symlink: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub created: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub modified: Option<OffsetDateTime>,
    pub mime: Option<String>,
    pub category: Category,
}

impl FileMeta {
    /// Build a FileMeta from a real path, using `std::fs::metadata`. This is
    /// the only place where we normalize what the OS gives us.
    pub fn from_path(path: &Path) -> crate::Result<Self> {
        let md = std::fs::symlink_metadata(path)?;
        let is_symlink = md.file_type().is_symlink();
        let md_follow = if is_symlink {
            std::fs::metadata(path).ok()
        } else {
            None
        };
        let size = md_follow.as_ref().unwrap_or(&md).len();

        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let parent = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        let (stem, extension) = split_stem_ext(&name);
        let is_hidden = name.starts_with('.');

        let created = md.created().ok().and_then(|t| systime_to_odt(t).ok());
        let modified = md.modified().ok().and_then(|t| systime_to_odt(t).ok());

        let category = Category::from_extension(&extension);
        let mime = crate::category::guess_mime(&extension);

        Ok(FileMeta {
            path: path.to_path_buf(),
            parent,
            name,
            stem,
            extension,
            size,
            is_hidden,
            is_symlink,
            created,
            modified,
            mime,
            category,
        })
    }

    /// Age of the file relative to `now` using the modified timestamp; falls
    /// back to created if modified is missing.
    pub fn age_days(&self, now: OffsetDateTime) -> Option<i64> {
        let ts = self.modified.or(self.created)?;
        Some((now - ts).whole_days())
    }
}

fn split_stem_ext(name: &str) -> (String, String) {
    // Preserve leading-dot filenames as stem-only (".env" -> stem ".env", ext "")
    if let Some(rest) = name.strip_prefix('.') {
        if let Some((stem, ext)) = rest.rsplit_once('.') {
            return (format!(".{stem}"), ext.to_lowercase());
        }
        return (name.to_string(), String::new());
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), ext.to_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

fn systime_to_odt(t: std::time::SystemTime) -> Result<OffsetDateTime, time::error::ComponentRange> {
    let d = t
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    OffsetDateTime::from_unix_timestamp_nanos(d.as_nanos() as i128)
}

// =========================================================================
// Rules
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Explicit ordering: lower runs first.
    pub priority: i32,
    pub enabled: bool,
    /// Optional profile grouping ("Work", "Personal", …).
    #[serde(default)]
    pub profile: Option<String>,
    pub condition: ConditionNode,
    pub action: Action,
    pub on_conflict: CollisionPolicy,
    /// Continue vs stop after this rule matches.
    #[serde(default)]
    pub then: RuleContinuation,
    /// Which folders this rule can consume from. Empty = any monitored folder.
    #[serde(default)]
    pub sources: Vec<PathBuf>,
    /// When it runs. `RealTime` means driven by the watcher; scheduled rules
    /// fire on their cadence.
    #[serde(default)]
    pub trigger: Trigger,
}

impl Rule {
    pub fn new_id() -> String { Uuid::new_v4().to_string() }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleContinuation {
    #[default]
    StopAfterAction,
    Continue,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    #[default]
    RealTime,
    Manual,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    Ask,
    Skip,
    #[default]
    Rename,
    Replace,
}

// =========================================================================
// Conditions
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConditionNode {
    All { children: Vec<ConditionNode> },
    Any { children: Vec<ConditionNode> },
    Leaf { condition: Condition },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// The file's parent directory equals or descends from this path.
    SourceFolder { path: PathBuf, recursive: bool },
    /// Case-insensitive filename substring.
    FilenameContains { needle: String },
    /// Regex against the whole filename.
    FilenameRegex { pattern: String },
    Extension { any_of: Vec<String> },
    Category { any_of: Vec<Category> },
    MimeType { any_of: Vec<String> },
    SizeBytes { op: NumOp, value: u64 },
    AgeDays { op: NumOp, value: i64 },
    Hidden { equals: bool },
    /// Modified date within [from, to] inclusive (either bound may be null).
    ModifiedBetween {
        #[serde(with = "time::serde::rfc3339::option")]
        from: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339::option")]
        to: Option<OffsetDateTime>,
    },
    /// Photo EXIF capture date. Requires `metadata` feature; without it, the
    /// condition never matches.
    PhotoCaptureAgeDays { op: NumOp, value: i64 },
    /// PDF page count.
    PdfPageCount { op: NumOp, value: u32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NumOp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl NumOp {
    pub fn compare<T: PartialOrd>(self, a: &T, b: &T) -> bool {
        match self {
            NumOp::Lt => a < b,
            NumOp::Le => a <= b,
            NumOp::Eq => a == b,
            NumOp::Ge => a >= b,
            NumOp::Gt => a > b,
        }
    }
}

// =========================================================================
// Actions
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Move { dest_dir: PathBuf, rename: Option<String> },
    Copy { dest_dir: PathBuf, rename: Option<String> },
    Rename { pattern: String },
    CreateFolder { path: PathBuf },
    Tag { tags: Vec<String> },
    Review,
    Trash,
}

// =========================================================================
// Planned operations
// =========================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Move,
    Copy,
    Rename,
    CreateFolder,
    Tag,
    Review,
    Trash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedOp {
    pub id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub kind: OpKind,
    pub source: Option<PathBuf>,
    /// Where the file will end up after the op. `None` for tag/review-only ops.
    pub destination: Option<PathBuf>,
    /// Bytes affected, best-effort. Used for previews only.
    pub size: u64,
    pub collision: CollisionPolicy,
    /// Explanation for the UI simulator ("age > 30d ✓", …).
    pub reasons: Vec<String>,
    /// Warnings surfaced by the safety validator that don't block execution.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl PlannedOp {
    pub fn new_id() -> String { Uuid::new_v4().to_string() }
}
