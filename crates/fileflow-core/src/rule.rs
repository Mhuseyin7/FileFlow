//! Rule evaluation. Deterministic and side-effect-free: given the same
//! `FileMeta` and `now`, `evaluate` always returns the same reasons vector.

use crate::model::{Condition, ConditionNode, FileMeta, NumOp};
use regex::Regex;
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
    let mut reasons = Vec::new();
    let matched = eval_node(node, file, now, &mut reasons);
    EvalOutcome { matched, reasons }
}

fn eval_node(
    node: &ConditionNode,
    file: &FileMeta,
    now: OffsetDateTime,
    reasons: &mut Vec<String>,
) -> bool {
    match node {
        ConditionNode::All { children } => {
            let mut all = true;
            for c in children {
                if !eval_node(c, file, now, reasons) {
                    all = false;
                }
            }
            all
        }
        ConditionNode::Any { children } => {
            if children.is_empty() {
                return false;
            }
            let mut any = false;
            for c in children {
                if eval_node(c, file, now, reasons) {
                    any = true;
                }
            }
            any
        }
        ConditionNode::Leaf { condition } => {
            let (ok, label) = eval_leaf(condition, file, now);
            reasons.push(format!("{} {label}", if ok { "✓" } else { "✗" }));
            ok
        }
    }
}

fn eval_leaf(c: &Condition, f: &FileMeta, now: OffsetDateTime) -> (bool, String) {
    match c {
        Condition::SourceFolder { path, recursive } => {
            let ok = if *recursive {
                f.parent.starts_with(path)
            } else {
                f.parent == *path
            };
            (ok, format!("folder = {}", path.display()))
        }
        Condition::FilenameContains { needle } => {
            let ok = f.name.to_lowercase().contains(&needle.to_lowercase());
            (ok, format!("filename contains \"{needle}\""))
        }
        Condition::FilenameRegex { pattern } => match Regex::new(pattern) {
            Ok(re) => (re.is_match(&f.name), format!("filename ~ /{pattern}/")),
            Err(_) => (false, format!("filename ~ /{pattern}/ (invalid)")),
        },
        Condition::Extension { any_of } => {
            let ok = any_of.iter().any(|e| {
                let e = e.trim_start_matches('.').to_lowercase();
                e == f.extension
            });
            (ok, format!("extension in [{}]", any_of.join(", ")))
        }
        Condition::Category { any_of } => {
            let ok = any_of.contains(&f.category);
            (ok, format!("category in {any_of:?}"))
        }
        Condition::MimeType { any_of } => {
            let ok = match &f.mime {
                Some(m) => any_of.iter().any(|x| x == m),
                None => false,
            };
            (ok, format!("mime in [{}]", any_of.join(", ")))
        }
        Condition::SizeBytes { op, value } => {
            (op.compare(&f.size, value), format!("size {} {}", op_label(*op), value))
        }
        Condition::AgeDays { op, value } => match f.age_days(now) {
            Some(age) => (op.compare(&age, value), format!("age {} {}d", op_label(*op), value)),
            None => (false, format!("age unknown vs {value}d")),
        },
        Condition::Hidden { equals } => (f.is_hidden == *equals, format!("hidden = {equals}")),
        Condition::ModifiedBetween { from, to } => {
            let m = f.modified;
            let ok = match m {
                Some(t) => {
                    let lo = from.map(|d| t >= d).unwrap_or(true);
                    let hi = to.map(|d| t <= d).unwrap_or(true);
                    lo && hi
                }
                None => false,
            };
            (ok, "modified in range".to_string())
        }
        Condition::PhotoCaptureAgeDays { op, value } => {
            let meta = crate::metadata::read_photo(&f.path);
            let capture = meta.and_then(|m| m.capture_date);
            match capture {
                Some(t) => {
                    let age = (now - t).whole_days();
                    (op.compare(&age, value), format!("photo age {} {}d", op_label(*op), value))
                }
                None => (false, format!("photo age unknown vs {value}d")),
            }
        }
        Condition::PdfPageCount { op, value } => {
            let meta = crate::metadata::read_pdf(&f.path);
            match meta.and_then(|m| m.page_count) {
                Some(n) => (op.compare(&n, value), format!("pdf pages {} {}", op_label(*op), value)),
                None => (false, format!("pdf pages unknown vs {value}")),
            }
        }
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
