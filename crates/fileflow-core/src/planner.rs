//! Action planner. Turns matched (rule, file) pairs into `PlannedOp`s. Never
//! touches the filesystem; the safety validator and executor come after.

use crate::error::Result;
use crate::model::{
    Action, Condition, ConditionNode, FileMeta, OpKind, PlannedOp, Rule, RuleContinuation, Trigger,
};
use crate::paths;
use crate::rename;
use crate::rule::evaluate_fast;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// Something that stopped a matched rule from producing an operation. These
/// are reported, never swallowed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanIssue {
    pub path: PathBuf,
    pub rule_name: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub ops: Vec<PlannedOp>,
    pub issues: Vec<PlanIssue>,
}

/// Rules that should run for `trigger`: enabled and matching the trigger.
pub fn active_rules(rules: &[Rule], trigger: Trigger) -> Vec<Rule> {
    rules
        .iter()
        .filter(|r| r.enabled && r.trigger == trigger)
        .cloned()
        .collect()
}

/// Plans every op that a batch of files would trigger. Rules are evaluated in
/// priority order (lower first); the `enabled` flag is the caller's concern,
/// so a disabled rule can be previewed before it is switched on.
///
/// `Continue` lets later rules see the same file, but only after actions that
/// leave it where it is (copy, tag, create folder). Move, rename, trash and
/// review end the chain: a later rule would otherwise be handed a path that
/// no longer exists.
pub fn plan(rules: &[Rule], files: &[FileMeta], now: OffsetDateTime) -> Plan {
    let mut ordered: Vec<&Rule> = rules.iter().collect();
    ordered.sort_by_key(|r| r.priority);

    let mut out = Plan::default();
    let mut folders_seen: HashSet<PathBuf> = HashSet::new();
    for file in files {
        for rule in &ordered {
            if !source_allowed(rule, file) {
                continue;
            }
            let outcome = evaluate_fast(&rule.condition, file, now);
            if !outcome.matched {
                continue;
            }
            let op = match plan_one(rule, file, outcome.reasons, now) {
                Ok(op) => op,
                Err(e) => {
                    out.issues.push(PlanIssue {
                        path: file.path.clone(),
                        rule_name: rule.name.clone(),
                        message: e.to_string(),
                    });
                    break;
                }
            };
            // The file is already where the rule would put it (for example a
            // watcher event for a file this rule moved a moment ago).
            if op.destination.is_some()
                && op.destination.as_deref() == op.source.as_deref()
                && op.kind != OpKind::CreateFolder
            {
                break;
            }
            let relocates = matches!(
                op.kind,
                OpKind::Move | OpKind::Rename | OpKind::Trash | OpKind::Review
            );
            let repeated_folder = op.kind == OpKind::CreateFolder
                && !op
                    .destination
                    .clone()
                    .is_some_and(|d| folders_seen.insert(d));
            if !repeated_folder {
                out.ops.push(op);
            }
            if relocates || rule.then == RuleContinuation::StopAfterAction {
                break;
            }
        }
    }
    out
}

fn source_allowed(rule: &Rule, file: &FileMeta) -> bool {
    rule.sources.is_empty()
        || rule
            .sources
            .iter()
            .any(|s| paths::starts_with(&file.parent, s, paths::CASE_INSENSITIVE_FS))
}

fn plan_one(
    rule: &Rule,
    file: &FileMeta,
    reasons: Vec<String>,
    now: OffsetDateTime,
) -> Result<PlannedOp> {
    let (kind, dest) = match &rule.action {
        Action::Move { dest_dir, rename } => (
            OpKind::Move,
            Some(place(dest_dir, rename.as_deref(), file, now)?),
        ),
        Action::Copy { dest_dir, rename } => (
            OpKind::Copy,
            Some(place(dest_dir, rename.as_deref(), file, now)?),
        ),
        Action::Rename { pattern } => {
            let name = rename::render(pattern, file, now)?;
            (OpKind::Rename, Some(file.parent.join(name)))
        }
        Action::CreateFolder { path } => (
            OpKind::CreateFolder,
            Some(rename::render_dir(path, file, now)?),
        ),
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
        tags: match &rule.action {
            Action::Tag { tags } => tags.clone(),
            _ => Vec::new(),
        },
    })
}

fn place(
    dest_dir: &Path,
    rename: Option<&str>,
    file: &FileMeta,
    now: OffsetDateTime,
) -> Result<PathBuf> {
    let dir = rename::render_dir(dest_dir, file, now)?;
    let name = match rename {
        Some(tmpl) => rename::render(tmpl, file, now)?,
        None => file.name.clone(),
    };
    Ok(dir.join(name))
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
    let src_overlap = paths_overlap(
        &collect_sources(&a.condition, &a.sources),
        &collect_sources(&b.condition, &b.sources),
    );
    let ext_overlap = match (
        collect_extensions(&a.condition),
        collect_extensions(&b.condition),
    ) {
        (Some(a), Some(b)) => a.iter().any(|x| b.contains(x)),
        _ => true, // no explicit ext constraint on one side ⇒ potentially any
    };
    (src_overlap && ext_overlap)
        .then(|| "shared source folder and overlapping extensions".to_string())
}

fn collect_extensions(node: &ConditionNode) -> Option<Vec<String>> {
    let mut out = Vec::new();
    node.visit(&mut |c| {
        if let Condition::Extension { any_of } = c {
            out.extend(
                any_of
                    .iter()
                    .map(|e| e.trim_start_matches('.').to_lowercase()),
            );
        }
    });
    (!out.is_empty()).then_some(out)
}

fn collect_sources(node: &ConditionNode, explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = explicit.to_vec();
    node.visit(&mut |c| {
        if let Condition::SourceFolder { path, .. } = c {
            out.push(path.clone());
        }
    });
    out
}

fn paths_overlap(a: &[PathBuf], b: &[PathBuf]) -> bool {
    let ci = paths::CASE_INSENSITIVE_FS;
    a.is_empty()
        || b.is_empty()
        || a.iter().any(|x| {
            b.iter()
                .any(|y| paths::starts_with(x, y, ci) || paths::starts_with(y, x, ci))
        })
}

/// Aggregate metrics for the preview banner.
pub fn summarize(ops: &[PlannedOp]) -> PreviewSummary {
    let mut s = PreviewSummary {
        count: ops.len(),
        ..Default::default()
    };
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
