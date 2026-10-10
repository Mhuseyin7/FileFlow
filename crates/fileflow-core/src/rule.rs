//! Rule evaluation. Deterministic and side-effect-free: given the same
//! `FileMeta` and `now`, `evaluate` always returns the same reasons vector.
//!
//! Two modes share one code path:
//!
//! * [`evaluate`] visits every leaf so the simulator can explain a
//!   non-match completely.
//! * [`evaluate_fast`] stops at the first leaf that decides the outcome. The
//!   planner uses it so an expensive leaf (PDF/EXIF metadata) is only read
//!   for files that survived the cheap ones.

use crate::model::{Condition, ConditionNode, FileMeta, NumOp};
use crate::paths;
use regex::{Regex, RegexBuilder};
use std::cell::RefCell;
use std::collections::HashMap;
use time::OffsetDateTime;

/// Result of evaluating a rule tree against a file.
#[derive(Debug, Clone)]
pub struct EvalOutcome {
    pub matched: bool,
    /// Human-readable trace, one entry per leaf visited. Prefixed with ✓ or ✗
    /// so the UI simulator can render it directly.
    pub reasons: Vec<String>,
}

pub fn evaluate(node: &ConditionNode, file: &FileMeta, now: OffsetDateTime) -> EvalOutcome {
    run(node, file, now, true)
}

pub fn evaluate_fast(node: &ConditionNode, file: &FileMeta, now: OffsetDateTime) -> EvalOutcome {
    run(node, file, now, false)
}

fn run(node: &ConditionNode, file: &FileMeta, now: OffsetDateTime, full: bool) -> EvalOutcome {
    let mut reasons = Vec::new();
    let matched = eval_node(node, file, now, full, &mut reasons);
    EvalOutcome { matched, reasons }
}

fn eval_node(
    node: &ConditionNode,
    file: &FileMeta,
    now: OffsetDateTime,
    full: bool,
    reasons: &mut Vec<String>,
) -> bool {
    match node {
        ConditionNode::All { children } => {
            let mut all = true;
            for c in children {
                if !eval_node(c, file, now, full, reasons) {
                    all = false;
                    if !full {
                        break;
                    }
                }
            }
            all
        }
        ConditionNode::Any { children } => {
            let mut any = false;
            for c in children {
                if eval_node(c, file, now, full, reasons) {
                    any = true;
                    if !full {
                        break;
                    }
                }
            }
            any
        }
        ConditionNode::Leaf { condition } => {
            let result = eval_leaf(condition, file, now);
            let mark = if result == Some(true) { "✓" } else { "✗" };
            let unknown = if result.is_none() { " (unknown)" } else { "" };
            reasons.push(format!("{mark} {}{unknown}", label(condition)));
            result == Some(true)
        }
    }
}

/// `None` means the file does not carry the data the condition needs.
fn eval_leaf(c: &Condition, f: &FileMeta, now: OffsetDateTime) -> Option<bool> {
    Some(match c {
        Condition::SourceFolder { path, recursive } => {
            let ci = paths::CASE_INSENSITIVE_FS;
            if *recursive {
                paths::starts_with(&f.parent, path, ci)
            } else {
                paths::starts_with(&f.parent, path, ci) && paths::starts_with(path, &f.parent, ci)
            }
        }
        Condition::FilenameContains { needle } => {
            f.name.to_lowercase().contains(&needle.to_lowercase())
        }
        Condition::FilenameRegex { pattern } => regex_is_match(pattern, &f.name)?,
        Condition::Extension { any_of } => any_of
            .iter()
            .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&f.extension)),
        Condition::Category { any_of } => any_of.contains(&f.category),
        Condition::MimeType { any_of } => {
            let mime = f.mime.as_deref()?;
            any_of.iter().any(|x| x.eq_ignore_ascii_case(mime))
        }
        Condition::SizeBytes { op, value } => op.compare(&f.size, value),
        Condition::AgeDays { op, value } => op.compare(&f.age_days(now)?, value),
        Condition::Hidden { equals } => f.is_hidden == *equals,
        Condition::ModifiedBetween { from, to } => {
            let t = f.modified?;
            from.map_or(true, |d| t >= d) && to.map_or(true, |d| t <= d)
        }
        Condition::PhotoCaptureAgeDays { op, value } => {
            let taken = crate::metadata::read_photo(&f.path)?.capture_date?;
            op.compare(&(now - taken).whole_days(), value)
        }
        Condition::PdfPageCount { op, value } => {
            let pages = crate::metadata::read_pdf(&f.path)?.page_count?;
            op.compare(&pages, value)
        }
    })
}

fn label(c: &Condition) -> String {
    match c {
        Condition::SourceFolder { path, .. } => format!("folder = {}", path.display()),
        Condition::FilenameContains { needle } => format!("filename contains \"{needle}\""),
        Condition::FilenameRegex { pattern } => format!("filename ~ /{pattern}/"),
        Condition::Extension { any_of } => format!("extension in [{}]", any_of.join(", ")),
        Condition::Category { any_of } => format!(
            "category in [{}]",
            any_of
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Condition::MimeType { any_of } => format!("mime in [{}]", any_of.join(", ")),
        Condition::SizeBytes { op, value } => format!("size {} {value} B", op_label(*op)),
        Condition::AgeDays { op, value } => format!("age {} {value}d", op_label(*op)),
        Condition::Hidden { equals } => format!("hidden = {equals}"),
        Condition::ModifiedBetween { .. } => "modified in range".to_string(),
        Condition::PhotoCaptureAgeDays { op, value } => {
            format!("photo age {} {value}d", op_label(*op))
        }
        Condition::PdfPageCount { op, value } => format!("pdf pages {} {value}", op_label(*op)),
    }
}

fn op_label(op: NumOp) -> &'static str {
    match op {
        NumOp::Lt => "<",
        NumOp::Le => "<=",
        NumOp::Eq => "=",
        NumOp::Ge => ">=",
        NumOp::Gt => ">",
    }
}

/// Patterns are user input. The regex crate has no catastrophic
/// backtracking, but the compiled size is still capped.
pub fn compile_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern).size_limit(1 << 20).build()
}

thread_local! {
    static REGEX_CACHE: RefCell<HashMap<String, Option<Regex>>> = RefCell::new(HashMap::new());
}

/// Compiling per file made large scans quadratic in practice; patterns are
/// compiled once per thread. `None` means the pattern does not compile.
fn regex_is_match(pattern: &str, text: &str) -> Option<bool> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 256 {
            cache.clear();
        }
        cache
            .entry(pattern.to_string())
            .or_insert_with(|| compile_regex(pattern).ok())
            .as_ref()
            .map(|re| re.is_match(text))
    })
}
