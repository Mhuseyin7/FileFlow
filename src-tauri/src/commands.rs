//! IPC command layer. The functions here are plain Rust so they can be unit
//! tested; the `tauri`-feature module below re-exports them as `#[command]`s.
//!
//! Naming convention: `fn ff_<verb>_<subject>` — matches the JS-side helper
//! names in `src/api.ts`.

use crate::state::AppState;
use fileflow_core::fs_ops::{execute, ExecuteOpts};
use fileflow_core::journal::JournalEntry;
use fileflow_core::model::{FileMeta, PlannedOp, Rule};
use fileflow_core::planner::{plan, summarize, PreviewSummary};
use fileflow_core::safety::validate;
use fileflow_core::watcher::{FileWatcher, WatcherConfig};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

pub fn ff_list_rules(state: &AppState) -> Vec<Rule> {
    state.rules.lock().unwrap().clone()
}

pub fn ff_save_rule(state: &AppState, mut rule: Rule) -> Rule {
    if rule.id.is_empty() {
        rule.id = Rule::new_id();
    }
    let mut rules = state.rules.lock().unwrap();
    if let Some(existing) = rules.iter_mut().find(|r| r.id == rule.id) {
        *existing = rule.clone();
    } else {
        rules.push(rule.clone());
    }
    rules.sort_by_key(|r| r.priority);
    rule
}

pub fn ff_delete_rule(state: &AppState, id: &str) {
    state.rules.lock().unwrap().retain(|r| r.id != id);
}

pub fn ff_set_sandbox_roots(state: &AppState, roots: Vec<PathBuf>) {
    let mut sb = state.sandbox.lock().unwrap();
    sb.allowed_roots = roots;
}

pub fn ff_get_sandbox_roots(state: &AppState) -> Vec<PathBuf> {
    state.sandbox.lock().unwrap().allowed_roots.clone()
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
pub fn ff_preview(state: &AppState, root: &Path) -> anyhow::Result<PreviewResult> {
    let files = ff_scan_folder(root, 5_000)?;
    let rules = state.rules.lock().unwrap().clone();
    let ops = plan(&rules, &files, OffsetDateTime::now_utc());
    let sandbox = state.sandbox.lock().unwrap().clone();
    let (ops, errors) = validate(&sandbox, ops);
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
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewResult {
    pub ops: Vec<PlannedOp>,
    pub summary: PreviewSummary,
    pub errors: Vec<PreviewError>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewError {
    pub op_id: String,
    pub message: String,
}

/// Run a preview'd op set. Dry-run + real modes.
pub fn ff_run(state: &AppState, ops: Vec<PlannedOp>, dry_run: bool) -> Vec<RunOutcome> {
    let mut out = Vec::with_capacity(ops.len());
    let journal = state.journal.lock().unwrap();
    for op in ops {
        if !dry_run {
            let _ = journal.record_start(&op);
        }
        match execute(
            &op,
            ExecuteOpts {
                dry_run,
                ..ExecuteOpts::default()
            },
        ) {
            Ok(e) => {
                if !dry_run {
                    let _ = journal.record_success(&e);
                }
                out.push(RunOutcome {
                    op_id: op.id.clone(),
                    ok: true,
                    destination: e.destination.map(|p| p.to_string_lossy().into_owned()),
                    error: None,
                });
            }
            Err(err) => {
                if !dry_run {
                    let _ = journal.record_failure(&op.id, &err);
                }
                out.push(RunOutcome {
                    op_id: op.id.clone(),
                    ok: false,
                    destination: None,
                    error: Some(err.to_string()),
                });
            }
        }
    }
    out
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunOutcome {
    pub op_id: String,
    pub ok: bool,
    pub destination: Option<String>,
    pub error: Option<String>,
}

pub fn ff_journal(state: &AppState, limit: usize) -> anyhow::Result<Vec<JournalEntry>> {
    Ok(state.journal.lock().unwrap().list_recent(limit)?)
}

pub fn ff_undo(state: &AppState, op_id: &str) -> anyhow::Result<()> {
    state.journal.lock().unwrap().undo(op_id)?;
    Ok(())
}

/// Start (or restart) the real-time file watcher on the current sandbox
/// roots. Idempotent: calling this while a watcher is running replaces it,
/// which is the intended path after the user grants a new folder.
pub fn ff_start_watcher(state: &AppState) -> anyhow::Result<()> {
    let roots = state.sandbox.lock().unwrap().allowed_roots.clone();
    let mut slot = state.watcher.lock().unwrap();
    if let Some(prev) = slot.take() {
        prev.shutdown();
    }
    if roots.is_empty() {
        return Ok(());
    }
    let watcher = FileWatcher::start(&roots, WatcherConfig::default())?;
    *slot = Some(watcher);
    Ok(())
}

pub fn ff_stop_watcher(state: &AppState) {
    if let Some(w) = state.watcher.lock().unwrap().take() {
        w.shutdown();
    }
}

pub fn ff_watcher_running(state: &AppState) -> bool {
    state.watcher.lock().unwrap().is_some()
}

// -------------------------------------------------------------------------
// Tauri bindings (feature-gated).
// -------------------------------------------------------------------------

#[cfg(feature = "tauri")]
pub mod tauri_bindings {
    use super::*;
    use tauri::State;

    #[tauri::command]
    pub fn list_rules(state: State<'_, AppState>) -> Vec<Rule> {
        ff_list_rules(&state)
    }

    #[tauri::command]
    pub fn save_rule(state: State<'_, AppState>, rule: Rule) -> Rule {
        ff_save_rule(&state, rule)
    }

    #[tauri::command]
    pub fn delete_rule(state: State<'_, AppState>, id: String) {
        ff_delete_rule(&state, &id);
    }

    #[tauri::command]
    pub fn set_sandbox_roots(state: State<'_, AppState>, roots: Vec<PathBuf>) {
        ff_set_sandbox_roots(&state, roots);
    }

    #[tauri::command]
    pub fn get_sandbox_roots(state: State<'_, AppState>) -> Vec<PathBuf> {
        ff_get_sandbox_roots(&state)
    }

    #[tauri::command]
    pub fn preview(state: State<'_, AppState>, root: PathBuf) -> Result<PreviewResult, String> {
        ff_preview(&state, &root).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn run(state: State<'_, AppState>, ops: Vec<PlannedOp>, dry_run: bool) -> Vec<RunOutcome> {
        ff_run(&state, ops, dry_run)
    }

    #[tauri::command]
    pub fn journal(state: State<'_, AppState>, limit: usize) -> Result<Vec<JournalEntry>, String> {
        ff_journal(&state, limit).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn undo(state: State<'_, AppState>, op_id: String) -> Result<(), String> {
        ff_undo(&state, &op_id).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn start_watcher(state: State<'_, AppState>) -> Result<(), String> {
        ff_start_watcher(&state).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn stop_watcher(state: State<'_, AppState>) {
        ff_stop_watcher(&state);
    }

    #[tauri::command]
    pub fn watcher_running(state: State<'_, AppState>) -> bool {
        ff_watcher_running(&state)
    }
}
