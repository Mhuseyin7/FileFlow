//! Safety validator. Runs between the planner and the executor, and again
//! right before every execution. Anything that could
//!
//! * touch a folder the user never granted,
//! * escape a granted folder through `..`, a symlink or a junction,
//! * write into an operating-system directory,
//! * target an invalid file name,
//!
//! is rejected here. Errors block execution; warnings attach to the
//! `PlannedOp` and surface in the preview.
//!
//! The sandbox is **fail-closed**: with no granted folders nothing is allowed.

use crate::error::{Error, Result};
use crate::model::{CollisionPolicy, OpKind, PlannedOp};
use crate::paths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The roots the user explicitly granted plus the roots FileFlow refuses to
/// touch no matter what.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    allowed: Vec<PathBuf>,
    forbidden: Vec<PathBuf>,
}

impl Sandbox {
    /// Roots are resolved once here. Allowed roots that cannot be resolved
    /// (missing, unreadable) are dropped, which only ever makes the sandbox
    /// smaller.
    pub fn new(
        allowed_roots: impl IntoIterator<Item = PathBuf>,
        forbidden_roots: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        Self {
            allowed: allowed_roots
                .into_iter()
                .filter_map(|p| paths::resolve(&p).ok())
                .collect(),
            forbidden: forbidden_roots
                .into_iter()
                .map(|p| paths::resolve(&p).unwrap_or(p))
                .collect(),
        }
    }

    pub fn allowed_roots(&self) -> &[PathBuf] {
        &self.allowed
    }

    /// Resolves `path` (see [`paths::resolve_entry`]) and checks it against
    /// both lists. Returns the resolved path on success.
    pub fn check(&self, path: &Path, what: &str) -> Result<PathBuf> {
        let resolved = paths::resolve_entry(path)?;
        if self.is_forbidden_resolved(&resolved) {
            return Err(Error::Safety(format!(
                "{what} is in a protected system location: {}",
                path.display()
            )));
        }
        if !self.contains_resolved(&resolved) {
            return Err(Error::Safety(format!(
                "{what} is outside the folders you granted: {}",
                path.display()
            )));
        }
        Ok(resolved)
    }

    pub fn contains(&self, path: &Path) -> bool {
        paths::resolve_entry(path).is_ok_and(|r| self.contains_resolved(&r))
    }

    fn contains_resolved(&self, resolved: &Path) -> bool {
        self.allowed
            .iter()
            .any(|root| paths::starts_with(resolved, root, false))
    }

    fn is_forbidden_resolved(&self, resolved: &Path) -> bool {
        self.forbidden
            .iter()
            .any(|root| paths::starts_with(resolved, root, paths::CASE_INSENSITIVE_FS))
    }
}

/// Operating-system locations that are never valid sources or destinations,
/// plus whatever the caller adds (for example FileFlow's own data directory).
pub fn default_forbidden_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    for var in [
        "SystemRoot",
        "windir",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
    ] {
        if let Some(v) = std::env::var_os(var) {
            roots.push(PathBuf::from(v));
        }
    }
    #[cfg(target_os = "macos")]
    roots.extend(
        [
            "/System",
            "/Library",
            "/usr",
            "/bin",
            "/sbin",
            "/dev",
            "/cores",
            "/private/etc",
            "/private/var/db",
        ]
        .map(PathBuf::from),
    );
    #[cfg(all(unix, not(target_os = "macos")))]
    roots.extend(
        [
            "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/proc", "/run", "/sbin",
            "/sys", "/usr", "/var/lib", "/var/log",
        ]
        .map(PathBuf::from),
    );
    roots.extend(extra.iter().cloned());
    roots
}

/// Vets a folder the user wants to grant. Returns the canonical path to
/// store. Rejects relative paths, missing folders, files, filesystem roots,
/// protected locations and any folder that contains a protected location
/// (granting `C:\` would otherwise sneak `C:\Windows` in).
pub fn vet_root(candidate: &Path, forbidden: &[PathBuf]) -> Result<PathBuf> {
    let resolved = paths::resolve(&paths::expand_tilde(candidate))?;
    let md = std::fs::metadata(&resolved)?;
    if !md.is_dir() {
        return Err(Error::Safety(format!(
            "not a folder: {}",
            resolved.display()
        )));
    }
    if paths::is_fs_root(&resolved) {
        return Err(Error::Safety(
            "a whole drive or filesystem root cannot be granted".into(),
        ));
    }
    for f in forbidden {
        let f = paths::resolve(f).unwrap_or_else(|_| f.clone());
        let ci = paths::CASE_INSENSITIVE_FS;
        if paths::starts_with(&resolved, &f, ci) || paths::starts_with(&f, &resolved, ci) {
            return Err(Error::Safety(format!(
                "{} overlaps a protected system location",
                resolved.display()
            )));
        }
    }
    Ok(resolved)
}

/// Validates a batch. Blocking problems come back as `(op_id, error)`;
/// everything else is returned with warnings attached.
pub fn validate(sandbox: &Sandbox, ops: Vec<PlannedOp>) -> (Vec<PlannedOp>, Vec<(String, Error)>) {
    let mut errors = Vec::new();
    let mut ok = Vec::with_capacity(ops.len());
    for mut op in ops {
        match validate_op(sandbox, &mut op) {
            Ok(()) => ok.push(op),
            Err(e) => errors.push((op.id.clone(), e)),
        }
    }
    flag_duplicate_destinations(&mut ok);
    (ok, errors)
}

fn flag_duplicate_destinations(ops: &mut [PlannedOp]) {
    let key = |p: &Path| {
        let s = p.to_string_lossy().into_owned();
        if paths::CASE_INSENSITIVE_FS {
            s.to_lowercase()
        } else {
            s
        }
    };
    let mut seen: HashMap<String, usize> = HashMap::new();
    for op in ops.iter() {
        if matches!(op.kind, OpKind::Move | OpKind::Copy | OpKind::Rename) {
            if let Some(d) = &op.destination {
                *seen.entry(key(d)).or_default() += 1;
            }
        }
    }
    for op in ops.iter_mut() {
        if let Some(d) = &op.destination {
            if seen.get(&key(d)).is_some_and(|n| *n > 1) {
                op.warnings
                    .push("another file in this batch has the same destination".into());
            }
        }
    }
}

/// Validates one op. Also called by the runner immediately before executing,
/// because the filesystem may have changed since the preview.
pub fn validate_op(sandbox: &Sandbox, op: &mut PlannedOp) -> Result<()> {
    let src = match &op.source {
        Some(s) => Some(sandbox.check(s, "source")?),
        None => None,
    };
    let dst = match &op.destination {
        Some(d) => Some(sandbox.check(d, "destination")?),
        None => None,
    };

    if let Some(dst) = &dst {
        let name = dst
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidFilename("destination name is not valid UTF-8".into()))?;
        if crate::rename::sanitize_filename(name)? != name {
            return Err(Error::InvalidFilename(format!(
                "'{name}' contains characters that are not allowed in file names"
            )));
        }
        if src.as_ref() == Some(dst) {
            return Err(Error::Safety("source and destination are the same".into()));
        }
        if dst.symlink_metadata().is_ok()
            && matches!(op.kind, OpKind::Move | OpKind::Copy | OpKind::Rename)
        {
            op.warnings.push(
                match op.collision {
                    CollisionPolicy::Rename => "destination exists: the file will be renamed",
                    CollisionPolicy::Skip => "destination exists: the file will be skipped",
                    CollisionPolicy::Ask => "destination exists: the file goes to the review queue",
                    CollisionPolicy::Replace => {
                        "destination exists: the existing file moves to FileFlow trash"
                    }
                }
                .into(),
            );
        }
    }

    if let (Some(src), OpKind::Copy) = (&src, op.kind) {
        if src
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(Error::Safety(
                "refusing to copy through a symbolic link".into(),
            ));
        }
    }
    Ok(())
}
