//! Action planner. Turns matched (rule, file) pairs into `PlannedOp`s. Never
//! touches the filesystem; the safety validator and executor come after.

use crate::error::Result;
use crate::model::{
    Action, ConditionNode, FileMeta, OpKind, PlannedOp, Rule, RuleContinuation,
};
use crate::rename;
use crate::rule::evaluate;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// Plan every op that a batch of files would trigger against an ordered list
/// of rules. Rules must already be sorted by priority.
pub fn plan(rules: &[Rule], files: &[FileMeta], now: OffsetDateTime) -> Vec<PlannedOp> {
    let mut ops = Vec::new();
    for file in files {
        for rule in rules {
            if !rule.enabled {
                continue;
            }
            if !source_allowed(rule, file) {
                continue;
            }
            let outcome = evaluate(&rule.condition, file, now);
            if !outcome.matched {
                continue;
            }
            match plan_one(rule, file, outcome.reasons, now) {
                Ok(op) => ops.push(op),
                Err(_e) => continue, // invalid rename template etc.; safety catches it too
            }
            if matches!(rule.then, RuleContinuation::StopAfterAction) {
                break;
            }
        }
    }
    ops
}

fn source_allowed(rule: &Rule, file: &FileMeta) -> bool {
    if rule.sources.is_empty() {
        return true;
    }
    rule.sources.iter().any(|s| file.parent.starts_with(s))
}

fn plan_one(
    rule: &Rule,
    file: &FileMeta,
    reasons: Vec<String>,
    now: OffsetDateTime,
) -> Result<PlannedOp> {
    let (kind, dest) = match &rule.action {
        Action::Move { dest_dir, rename } => {
            let name = match rename {
                Some(tmpl) => rename::render(tmpl, file, now)?,
                None => file.name.clone(),
            };
            (OpKind::Move, Some(dest_dir.join(name)))
        }
        Action::Copy { dest_dir, rename } => {
            let name = match rename {
                Some(tmpl) => rename::render(tmpl, file, now)?,
                None => file.name.clone(),
            };
            (OpKind::Copy, Some(dest_dir.join(name)))
        }
        Action::Rename { pattern } => {
            let name = rename::render(pattern, file, now)?;
            (OpKind::Rename, Some(file.parent.join(name)))
        }
        Action::CreateFolder { path } => (OpKind::CreateFolder, Some(path.clone())),
        Action::Tag { .. } => (OpKind::Tag, None),
        Action::Review => (OpKind::Review, None),
        Action::Trash => (OpKind::Trash, None),
    };
    Ok(PlannedOp {
        id: PlannedOp::new_id(),
        rule_id: rule.id.clone(),
        rule_name: rule.name.clone(),
        kind,
        source: Some(file.path.clone()),
        destination: dest,
        size: file.size,
        collision: rule.on_conflict,
        reasons,
        warnings: Vec::new(),
    })
}

/// Detects rules that would match overlapping file sets, so we can warn the
/// user during editing. This is a *shape* check (source folders + extension
/// sets), not an exhaustive semantic overlap detector.
pub fn detect_overlaps(rules: &[Rule]) -> Vec<(String, String, String)> {
    let mut hits = Vec::new();
    for i in 0..rules.len() {
        for j in (i + 1)..rules.len() {
            if let Some(reason) = overlap_reason(&rules[i], &rules[j]) {
                hits.push((rules[i].id.clone(), rules[j].id.clone(), reason));
            }
        }
    }
    hits
}

fn overlap_reason(a: &Rule, b: &Rule) -> Option<String> {
    let a_ext = collect_extensions(&a.condition);
    let b_ext = collect_extensions(&b.condition);
    let a_src = collect_sources(&a.condition, &a.sources);
    let b_src = collect_sources(&b.condition, &b.sources);
    let src_overlap = paths_overlap(&a_src, &b_src);
    let ext_overlap = match (&a_ext, &b_ext) {
        (Some(a), Some(b)) => a.iter().any(|x| b.contains(x)),
        _ => true, // no explicit ext constraint on one side ⇒ potentially any
    };
    if src_overlap && ext_overlap {
        Some("shared source folder and overlapping extensions".to_string())
    } else {
        None
    }
}

fn collect_extensions(node: &ConditionNode) -> Option<Vec<String>> {
    let mut out = Vec::new();
    walk(node, &mut |c| {
        if let crate::model::Condition::Extension { any_of } = c {
            for e in any_of {
                out.push(e.trim_start_matches('.').to_lowercase());
            }
        }
    });
    if out.is_empty() { None } else { Some(out) }
}

fn collect_sources(node: &ConditionNode, explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = explicit.to_vec();
    walk(node, &mut |c| {
        if let crate::model::Condition::SourceFolder { path, .. } = c {
            out.push(path.clone());
        }
    });
    out
}

fn walk(node: &ConditionNode, visit: &mut dyn FnMut(&crate::model::Condition)) {
    match node {
        ConditionNode::All { children } | ConditionNode::Any { children } => {
            for c in children {
                walk(c, visit);
            }
        }
        ConditionNode::Leaf { condition } => visit(condition),
    }
}

fn paths_overlap(a: &[PathBuf], b: &[PathBuf]) -> bool {
    if a.is_empty() || b.is_empty() {
        return true;
    }
    a.iter().any(|x| b.iter().any(|y| overlap(x, y)))
}

fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Aggregate metrics for the preview banner.
pub fn summarize(ops: &[PlannedOp]) -> PreviewSummary {
    let mut s = PreviewSummary { count: ops.len(), ..Default::default() };
    for op in ops {
        s.total_bytes = s.total_bytes.saturating_add(op.size);
        if !op.warnings.is_empty() {
            s.warnings += 1;
        }
    }
    s
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviewSummary {
    pub count: usize,
    pub total_bytes: u64,
    pub warnings: usize,
}

impl PreviewSummary {
    pub fn human_bytes(&self) -> String { human_bytes(self.total_bytes) }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut n = bytes as f64;
    let mut i = 0;
    while n >= 1024.0 && i < UNITS.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{n:.1} {}", UNITS[i])
    }
}
