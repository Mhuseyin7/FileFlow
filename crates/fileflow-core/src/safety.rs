//! Safety validator. Runs between the planner and the executor. Anything that
//! could:
//!
//! * silently delete data,
//! * cross a symlink out of the sandbox,
//! * overwrite a newer file,
//! * touch a monitored root that wasn't explicitly granted,
//!
//! must be caught here. Errors block execution; warnings attach to the
//! `PlannedOp` and surface in the preview.

use crate::error::{Error, Result};
use crate::model::{CollisionPolicy, OpKind, PlannedOp};
use std::path::{Path, PathBuf};

/// Sandbox describes the roots the user has explicitly granted.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    pub allowed_roots: Vec<PathBuf>,
    /// Roots we refuse to touch even if the user granted them (system dirs).
    pub forbidden_roots: Vec<PathBuf>,
}

impl Sandbox {
    pub fn contains(&self, p: &Path) -> bool {
        let canon = canonical_or_self(p);
        self.allowed_roots.iter().any(|r| starts_with(&canon, r))
    }
    pub fn is_forbidden(&self, p: &Path) -> bool {
        let canon = canonical_or_self(p);
        self.forbidden_roots.iter().any(|r| starts_with(&canon, r))
    }
}

fn canonical_or_self(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn starts_with(child: &Path, parent: &Path) -> bool {
    // Compare canonicalized parent when possible.
    let parent_c = canonical_or_self(parent);
    child.starts_with(&parent_c)
}

/// Validate a batch of ops; annotates warnings in place and returns the
/// filtered list (blocking errors are removed) alongside per-op errors.
pub fn validate(sandbox: &Sandbox, mut ops: Vec<PlannedOp>) -> (Vec<PlannedOp>, Vec<(String, Error)>) {
    let mut errors = Vec::new();
    let mut ok = Vec::with_capacity(ops.len());
    for mut op in ops.drain(..) {
        match validate_one(sandbox, &mut op) {
            Ok(()) => ok.push(op),
            Err(e) => errors.push((op.id.clone(), e)),
        }
    }
    ok
        .iter_mut()
        .for_each(|op| dedupe_warnings(&mut op.warnings));
    (ok, errors)
}

fn dedupe_warnings(w: &mut Vec<String>) {
    w.sort();
    w.dedup();
}

fn validate_one(sandbox: &Sandbox, op: &mut PlannedOp) -> Result<()> {
    if let Some(src) = &op.source {
        if sandbox.is_forbidden(src) {
            return Err(Error::Safety(format!("source in forbidden root: {}", src.display())));
        }
        // Source doesn't strictly have to be inside the sandbox — the user may
        // move a file into a sandbox — but if it isn't, that's worth warning
        // about, since watchers only fire inside sandbox roots.
        if !sandbox.allowed_roots.is_empty() && !sandbox.contains(src) {
            op.warnings.push(format!("source outside sandbox: {}", src.display()));
        }
    }
    if let Some(dst) = &op.destination {
        if sandbox.is_forbidden(dst) {
            return Err(Error::Safety(format!("destination in forbidden root: {}", dst.display())));
        }
        if !sandbox.allowed_roots.is_empty() && !sandbox.contains(dst) {
            // For destinations we're stricter — never write outside sandbox.
            return Err(Error::Safety(format!(
                "destination outside allowed roots: {}",
                dst.display()
            )));
        }
        if let Some(name) = dst.file_name().and_then(|s| s.to_str()) {
            crate::rename::sanitize_filename(name)?;
        } else {
            return Err(Error::InvalidFilename("no filename component".into()));
        }
        // Source == destination is a no-op we shouldn't record; block it so
        // callers don't accidentally log a "success" that did nothing.
        if let Some(src) = &op.source {
            if canonical_or_self(src) == canonical_or_self(dst) {
                return Err(Error::Safety("source and destination identical".into()));
            }
        }
        // Overwrite is only allowed via explicit Replace policy.
        if dst.exists()
            && matches!(op.kind, OpKind::Move | OpKind::Copy | OpKind::Rename)
            && !matches!(op.collision, CollisionPolicy::Replace | CollisionPolicy::Rename | CollisionPolicy::Ask)
        {
            return Err(Error::Conflict(format!("destination exists: {}", dst.display())));
        }
    }
    // Symlink hop-out: if source is a symlink pointing outside the sandbox,
    // refuse.
    if let Some(src) = &op.source {
        if let Ok(md) = std::fs::symlink_metadata(src) {
            if md.file_type().is_symlink() {
                if let Ok(target) = std::fs::read_link(src) {
                    let resolved = if target.is_absolute() {
                        target
                    } else {
                        src.parent().unwrap_or(Path::new("")).join(target)
                    };
                    if !sandbox.allowed_roots.is_empty() && !sandbox.contains(&resolved) {
                        return Err(Error::Safety(format!(
                            "symlink target escapes sandbox: {}",
                            resolved.display()
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}
