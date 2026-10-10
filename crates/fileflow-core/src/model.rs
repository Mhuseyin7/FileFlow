//! Core data model. Everything here is serde-friendly so it can cross the
//! Tauri IPC boundary and be persisted in SQLite without a translation layer.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::category::Category;
use crate::error::{Error, Result};
use crate::paths;

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
    pub fn from_path(path: &Path) -> Result<Self> {
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
        let (stem, extension) = {
            let (stem, ext) = crate::paths::split_name(&name);
            (stem.to_string(), ext.to_lowercase())
        };
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

fn systime_to_odt(
    t: std::time::SystemTime,
) -> std::result::Result<OffsetDateTime, time::error::ComponentRange> {
    let d = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
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
    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Canonical form that gets stored: `~` expanded in every path and
    /// extensions lowercased without their dot.
    pub fn normalized(mut self) -> Self {
        self.name = self.name.trim().to_string();
        for s in &mut self.sources {
            *s = paths::expand_tilde(s);
        }
        self.condition.normalize();
        match &mut self.action {
            Action::Move { dest_dir, .. } | Action::Copy { dest_dir, .. } => {
                *dest_dir = paths::expand_tilde(dest_dir)
            }
            Action::CreateFolder { path } => *path = paths::expand_tilde(path),
            _ => {}
        }
        self
    }

    /// Rejects rules that could never behave sensibly or that would match
    /// far more than the user intended. Call on a [`Rule::normalized`] rule.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(invalid("give the rule a name"));
        }
        if self.name.chars().count() > 120 {
            return Err(invalid(
                "the rule name is too long (120 characters at most)",
            ));
        }
        self.condition.validate()?;
        for s in &self.sources {
            require_absolute(s, "source folder")?;
        }
        self.action.validate()
    }
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::InvalidRule(msg.into())
}

fn require_absolute(p: &Path, what: &str) -> Result<()> {
    if !p.is_absolute() {
        return Err(invalid(format!(
            "{what} must be an absolute path, got '{}'",
            p.display()
        )));
    }
    if p.components().any(|c| c == Component::ParentDir) {
        return Err(invalid(format!("{what} must not contain '..'")));
    }
    Ok(())
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

impl ConditionNode {
    /// Calls `f` for every leaf, depth first.
    pub fn visit(&self, f: &mut dyn FnMut(&Condition)) {
        match self {
            ConditionNode::All { children } | ConditionNode::Any { children } => {
                children.iter().for_each(|c| c.visit(f))
            }
            ConditionNode::Leaf { condition } => f(condition),
        }
    }

    fn normalize(&mut self) {
        match self {
            ConditionNode::All { children } | ConditionNode::Any { children } => {
                children.iter_mut().for_each(ConditionNode::normalize)
            }
            ConditionNode::Leaf { condition } => condition.normalize(),
        }
    }

    /// An empty `All` matches every file, so empty groups are rejected.
    fn validate(&self) -> Result<()> {
        match self {
            ConditionNode::All { children } | ConditionNode::Any { children } => {
                if children.is_empty() {
                    return Err(invalid(
                        "add at least one condition — a rule without conditions matches every file",
                    ));
                }
                children.iter().try_for_each(ConditionNode::validate)
            }
            ConditionNode::Leaf { condition } => condition.validate(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// The file's parent directory equals or descends from this path.
    SourceFolder {
        path: PathBuf,
        recursive: bool,
    },
    /// Case-insensitive filename substring.
    FilenameContains {
        needle: String,
    },
    /// Regex against the whole filename.
    FilenameRegex {
        pattern: String,
    },
    Extension {
        any_of: Vec<String>,
    },
    Category {
        any_of: Vec<Category>,
    },
    MimeType {
        any_of: Vec<String>,
    },
    SizeBytes {
        op: NumOp,
        value: u64,
    },
    AgeDays {
        op: NumOp,
        value: i64,
    },
    Hidden {
        equals: bool,
    },
    /// Modified date within [from, to] inclusive (either bound may be null).
    ModifiedBetween {
        #[serde(with = "time::serde::rfc3339::option")]
        from: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339::option")]
        to: Option<OffsetDateTime>,
    },
    /// Photo EXIF capture date. Requires `metadata` feature; without it, the
    /// condition never matches.
    PhotoCaptureAgeDays {
        op: NumOp,
        value: i64,
    },
    /// PDF page count.
    PdfPageCount {
        op: NumOp,
        value: u32,
    },
}

impl Condition {
    fn normalize(&mut self) {
        match self {
            Condition::SourceFolder { path, .. } => *path = paths::expand_tilde(path),
            Condition::Extension { any_of } => {
                for e in any_of.iter_mut() {
                    *e = e.trim().trim_start_matches('.').to_lowercase();
                }
            }
            _ => {}
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Condition::SourceFolder { path, .. } => require_absolute(path, "source folder"),
            Condition::FilenameContains { needle } if needle.trim().is_empty() => {
                Err(invalid("'filename contains' needs some text"))
            }
            Condition::FilenameRegex { pattern } => {
                if pattern.is_empty() {
                    return Err(invalid("the filename pattern is empty"));
                }
                crate::rule::compile_regex(pattern)
                    .map(|_| ())
                    .map_err(|e| invalid(format!("invalid filename pattern: {e}")))
            }
            Condition::Extension { any_of } => {
                if any_of.is_empty()
                    || any_of
                        .iter()
                        .any(|e| e.is_empty() || e.contains(['/', '\\']))
                {
                    return Err(invalid("list at least one extension, for example pdf"));
                }
                Ok(())
            }
            Condition::Category { any_of } if any_of.is_empty() => {
                Err(invalid("pick at least one category"))
            }
            Condition::MimeType { any_of } if any_of.is_empty() => {
                Err(invalid("list at least one MIME type"))
            }
            Condition::AgeDays { value, .. } | Condition::PhotoCaptureAgeDays { value, .. }
                if *value < 0 =>
            {
                Err(invalid("an age cannot be negative"))
            }
            Condition::ModifiedBetween { from, to } => match (from, to) {
                (None, None) => Err(invalid("set at least one date for 'modified between'")),
                (Some(a), Some(b)) if a > b => Err(invalid("the start date is after the end date")),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
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
    Move {
        dest_dir: PathBuf,
        rename: Option<String>,
    },
    Copy {
        dest_dir: PathBuf,
        rename: Option<String>,
    },
    Rename {
        pattern: String,
    },
    CreateFolder {
        path: PathBuf,
    },
    Tag {
        tags: Vec<String>,
    },
    Review,
    Trash,
}

impl Action {
    fn validate(&self) -> Result<()> {
        match self {
            Action::Move { dest_dir, rename } | Action::Copy { dest_dir, rename } => {
                require_absolute(dest_dir, "destination folder")?;
                crate::rename::validate_dir_template(dest_dir)
                    .map_err(|e| invalid(e.to_string()))?;
                if let Some(t) = rename {
                    crate::rename::validate_template(t).map_err(|e| invalid(e.to_string()))?;
                }
                Ok(())
            }
            Action::Rename { pattern } => {
                if pattern.is_empty() {
                    return Err(invalid("the new name is empty"));
                }
                crate::rename::validate_template(pattern).map_err(|e| invalid(e.to_string()))
            }
            Action::CreateFolder { path } => {
                require_absolute(path, "folder")?;
                crate::rename::validate_dir_template(path).map_err(|e| invalid(e.to_string()))
            }
            Action::Tag { tags } => {
                if tags.is_empty()
                    || tags
                        .iter()
                        .any(|t| t.trim().is_empty() || t.chars().count() > 64)
                {
                    return Err(invalid("tags must be 1 to 64 characters"));
                }
                Ok(())
            }
            Action::Review | Action::Trash => Ok(()),
        }
    }
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

impl OpKind {
    /// The same spelling serde uses; the journal stores it as text.
    pub fn as_str(self) -> &'static str {
        match self {
            OpKind::Move => "move",
            OpKind::Copy => "copy",
            OpKind::Rename => "rename",
            OpKind::CreateFolder => "create_folder",
            OpKind::Tag => "tag",
            OpKind::Review => "review",
            OpKind::Trash => "trash",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "move" => OpKind::Move,
            "copy" => OpKind::Copy,
            "rename" => OpKind::Rename,
            "create_folder" => OpKind::CreateFolder,
            "tag" => OpKind::Tag,
            "review" => OpKind::Review,
            "trash" => OpKind::Trash,
            _ => return None,
        })
    }
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
    /// Tags to attach (only for `OpKind::Tag`).
    #[serde(default)]
    pub tags: Vec<String>,
}

impl PlannedOp {
    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }
}
