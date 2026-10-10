//! IPC command layer. The functions here are plain Rust so they can be unit
//! tested; the `tauri`-feature module below re-exports them as `#[command]`s.
//!
//! Naming convention: `fn ff_<verb>_<subject>` — matches the JS-side helper
//! names in `src/api.ts`.

use crate::state::AppState;
use fileflow_core::fs_ops::{execute, ExecuteOpts};
use fileflow_core::journal::{JournalEntry, Recovery};
use fileflow_core::model::{FileMeta, OpKind, PlannedOp, Rule};
use fileflow_core::planner::{plan, summarize, PlanIssue, PreviewSummary};
use fileflow_core::safety::{default_forbidden_roots, validate, validate_op, vet_root, Sandbox};
use fileflow_core::store::ReviewItem;
use fileflow_core::Error;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

pub fn ff_list_rules(state: &AppState) -> Vec<Rule> {
    state.rules.lock().unwrap().clone()
}

/// Normalizes, validates and persists a rule. A rule that would match every
/// file, or that has an invalid path/template, is rejected with a message
/// the UI can show as-is.
pub fn ff_save_rule(state: &AppState, mut rule: Rule) -> anyhow::Result<Rule> {
    if rule.id.is_empty() {
        rule.id = Rule::new_id();
    }
    let rule = rule.normalized();
    rule.validate()?;
    state.store.save_rule(&rule)?;
    let mut rules = state.rules.lock().unwrap();
    match rules.iter_mut().find(|r| r.id == rule.id) {
        Some(existing) => *existing = rule.clone(),
        None => rules.push(rule.clone()),
    }
    rules.sort_by_key(|r| r.priority);
    Ok(rule)
}

pub fn ff_delete_rule(state: &AppState, id: &str) -> anyhow::Result<()> {
    state.store.delete_rule(id)?;
    state.rules.lock().unwrap().retain(|r| r.id != id);
    Ok(())
}

/// Replaces the granted folders. Every folder is vetted (not a drive root,
/// not a system location, exists) before anything is stored.
pub fn ff_set_sandbox_roots(state: &AppState, roots: Vec<PathBuf>) -> anyhow::Result<()> {
    let forbidden = default_forbidden_roots(std::slice::from_ref(&state.data_dir));
    let vetted = roots
        .iter()
        .map(|r| vet_root(r, &forbidden))
        .collect::<fileflow_core::Result<Vec<_>>>()?;
    state.store.set_folders(&vetted)?;
    *state.sandbox.lock().unwrap() = Sandbox::new(vetted, forbidden);
    Ok(())
}

pub fn ff_get_sandbox_roots(state: &AppState) -> Vec<PathBuf> {
    state.sandbox.lock().unwrap().allowed_roots().to_vec()
}

/// What the last start found and fixed after an unclean shutdown.
pub fn ff_recovered(state: &AppState) -> Vec<Recovery> {
    state.recovered.clone()
}

pub fn ff_review_queue(state: &AppState) -> anyhow::Result<Vec<ReviewItem>> {
    Ok(state.store.list_review()?)
}

pub fn ff_resolve_review(state: &AppState, id: &str, resolution: &str) -> anyhow::Result<()> {
    Ok(state.store.resolve_review(id, resolution)?)
}

pub fn ff_scan_folder(root: &Path, limit: usize) -> anyhow::Result<Vec<FileMeta>> {
    let mut out = Vec::new();
    scan_recursive(root, limit, &mut out)?;
    Ok(out)
}

fn scan_recursive(dir: &Path, limit: usize, out: &mut Vec<FileMeta>) -> anyhow::Result<()> {
    if out.len() >= limit {
        return Ok(());
    }
    let iter = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return Ok(()),
    };
    for entry in iter {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == fileflow_core::fs_ops::LOCAL_TRASH_DIR {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            scan_recursive(&path, limit, out)?;
            if out.len() >= limit {
                return Ok(());
            }
            continue;
        }
        if let Ok(meta) = FileMeta::from_path(&path) {
            out.push(meta);
            if out.len() >= limit {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Full preview: plan against a folder, validate against the sandbox, and
/// return ops + summary. Nothing is executed.
///
/// With `rule_id`, only that rule is planned (even if it is disabled), so the
/// preview shows what the rule itself would do rather than what is left over
/// after higher-priority rules took their files.
pub fn ff_preview(
    state: &AppState,
    root: &Path,
    rule_id: Option<&str>,
) -> anyhow::Result<PreviewResult> {
    let root = fileflow_core::paths::expand_tilde(root);
    let sandbox = state.sandbox.lock().unwrap().clone();
    sandbox.check(&root, "folder")?;
    let files = ff_scan_folder(&root, 5_000)?;
    let mut rules = state.rules.lock().unwrap().clone();
    if let Some(id) = rule_id {
        rules.retain(|r| r.id == id);
    }
    let planned = plan(&rules, &files, OffsetDateTime::now_utc());
    let (ops, errors) = validate(&sandbox, planned.ops);
    let summary = summarize(&ops);
    Ok(PreviewResult {
        ops,
        summary,
        errors: errors
            .into_iter()
            .map(|(id, e)| PreviewError {
                op_id: id,
                message: e.to_string(),
            })
            .collect(),
        issues: planned.issues,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewResult {
    pub ops: Vec<PlannedOp>,
    pub summary: PreviewSummary,
    pub errors: Vec<PreviewError>,
    /// Matched files a rule could not produce an operation for.
    pub issues: Vec<PlanIssue>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewError {
    pub op_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Done,
    /// Left alone on purpose (the collision policy said skip).
    Skipped,
    /// Waiting in the review queue for a human decision.
    NeedsReview,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunOutcome {
    pub op_id: String,
    pub ok: bool,
    pub status: RunStatus,
    pub destination: Option<String>,
    pub error: Option<String>,
}

/// Runs a previewed op set. The frontend is not trusted: every op is
/// validated against the sandbox again immediately before it runs, so a
/// forged or stale op cannot reach outside the granted folders.
pub fn ff_run(state: &AppState, ops: Vec<PlannedOp>, dry_run: bool) -> Vec<RunOutcome> {
    let _serial = state.run_lock.lock().unwrap_or_else(|p| p.into_inner());
    let sandbox = state.sandbox.lock().unwrap().clone();
    ops.into_iter()
        .map(|mut op| run_one(state, &sandbox, &mut op, dry_run))
        .collect()
}

fn run_one(state: &AppState, sandbox: &Sandbox, op: &mut PlannedOp, dry_run: bool) -> RunOutcome {
    let op_id = op.id.clone();
    let outcome = |status: RunStatus, dest: Option<String>, error: Option<String>| RunOutcome {
        op_id: op_id.clone(),
        ok: matches!(status, RunStatus::Done),
        status,
        destination: dest,
        error,
    };
    if let Err(e) = validate_op(sandbox, op) {
        if !dry_run {
            let _ = state.journal.record_start(op);
            let _ = state.journal.record_failure(&op.id, &e);
        }
        return outcome(RunStatus::Failed, None, Some(e.to_string()));
    }
    if !dry_run {
        if let Err(e) = state.journal.record_start(op) {
            // Never touch a file we cannot write down.
            return outcome(RunStatus::Failed, None, Some(e.to_string()));
        }
    }

    // Bookkeeping operations: no filesystem change.
    if matches!(op.kind, OpKind::Tag | OpKind::Review) {
        if dry_run {
            return outcome(RunStatus::Done, None, None);
        }
        let result = match (op.kind, op.source.as_deref()) {
            (OpKind::Tag, Some(src)) => state.store.add_tags(src, &op.tags),
            (OpKind::Review, Some(src)) => {
                state
                    .store
                    .add_review(src, &op.rule_id, &op.rule_name, "review", "flagged by rule")
            }
            _ => Err(Error::Other("missing source".into())),
        };
        return match result {
            Ok(()) => {
                let _ = state
                    .journal
                    .record_skipped(&op.id, "recorded; no file changed");
                outcome(RunStatus::Done, None, None)
            }
            Err(e) => {
                let _ = state.journal.record_failure(&op.id, &e);
                outcome(RunStatus::Failed, None, Some(e.to_string()))
            }
        };
    }

    match execute(
        op,
        &ExecuteOpts {
            dry_run,
            ..ExecuteOpts::default()
        },
    ) {
        Ok(e) => {
            if !dry_run {
                let _ = state.journal.record_success(&e);
                if let Some(d) = e.destination.as_deref() {
                    state.remember_output(d);
                }
            }
            outcome(
                RunStatus::Done,
                e.destination.map(|p| p.to_string_lossy().into_owned()),
                None,
            )
        }
        Err(Error::Skipped(why)) => {
            if !dry_run {
                let _ = state.journal.record_skipped(&op.id, &why);
            }
            outcome(RunStatus::Skipped, None, Some(why))
        }
        Err(Error::NeedsInput(why)) => {
            if !dry_run {
                if let Some(src) = op.source.as_deref() {
                    let _ = state.store.add_review(
                        src,
                        &op.rule_id,
                        &op.rule_name,
                        op.kind.as_str(),
                        &why,
                    );
                }
                let _ = state.journal.record_skipped(&op.id, &why);
            }
            outcome(RunStatus::NeedsReview, None, Some(why))
        }
        Err(err) => {
            if !dry_run {
                let _ = state.journal.record_failure(&op.id, &err);
            }
            outcome(RunStatus::Failed, None, Some(err.to_string()))
        }
    }
}

pub fn ff_journal(state: &AppState, limit: usize) -> anyhow::Result<Vec<JournalEntry>> {
    Ok(state.journal.list_recent(limit.min(1_000))?)
}

pub fn ff_undo(state: &AppState, op_id: &str) -> anyhow::Result<()> {
    let _serial = state.run_lock.lock().unwrap_or_else(|p| p.into_inner());
    state.journal.undo(op_id)?;
    Ok(())
}

// -------------------------------------------------------------------------
// Tauri bindings (feature-gated).
// -------------------------------------------------------------------------

#[cfg(feature = "tauri")]
pub mod tauri_bindings {
    use super::*;
    use std::sync::Arc;
    use tauri::State;

    #[tauri::command]
    pub fn list_rules(state: State<'_, Arc<AppState>>) -> Vec<Rule> {
        ff_list_rules(&state)
    }

    #[tauri::command]
    pub fn save_rule(state: State<'_, Arc<AppState>>, rule: Rule) -> Result<Rule, String> {
        ff_save_rule(&state, rule).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn delete_rule(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
        ff_delete_rule(&state, &id).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn recovered(state: State<'_, Arc<AppState>>) -> Vec<Recovery> {
        ff_recovered(&state)
    }

    #[tauri::command]
    pub fn review_queue(state: State<'_, Arc<AppState>>) -> Result<Vec<ReviewItem>, String> {
        ff_review_queue(&state).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn resolve_review(
        state: State<'_, Arc<AppState>>,
        id: String,
        resolution: String,
    ) -> Result<(), String> {
        ff_resolve_review(&state, &id, &resolution).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn set_sandbox_roots(
        state: State<'_, Arc<AppState>>,
        roots: Vec<PathBuf>,
    ) -> Result<(), String> {
        ff_set_sandbox_roots(&state, roots).map_err(|e| e.to_string())?;
        // Watch exactly what is granted now.
        crate::engine::start_watcher(&state).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn get_sandbox_roots(state: State<'_, Arc<AppState>>) -> Vec<PathBuf> {
        ff_get_sandbox_roots(&state)
    }

    #[tauri::command]
    pub fn preview(
        state: State<'_, Arc<AppState>>,
        root: PathBuf,
        rule_id: Option<String>,
    ) -> Result<PreviewResult, String> {
        ff_preview(&state, &root, rule_id.as_deref()).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn run(
        state: State<'_, Arc<AppState>>,
        ops: Vec<PlannedOp>,
        dry_run: bool,
    ) -> Vec<RunOutcome> {
        ff_run(&state, ops, dry_run)
    }

    #[tauri::command]
    pub fn journal(
        state: State<'_, Arc<AppState>>,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, String> {
        ff_journal(&state, limit).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn undo(state: State<'_, Arc<AppState>>, op_id: String) -> Result<(), String> {
        ff_undo(&state, &op_id).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn start_watcher(state: State<'_, Arc<AppState>>) -> Result<(), String> {
        crate::engine::start_watcher(&state).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn stop_watcher(state: State<'_, Arc<AppState>>) {
        crate::engine::stop_watcher(&state);
    }

    #[tauri::command]
    pub fn watcher_running(state: State<'_, Arc<AppState>>) -> bool {
        crate::engine::watcher_running(&state)
    }
}
