//! Path helpers shared by the safety validator, the rule engine and the
//! scanner. Everything that decides "is this path inside that folder" goes
//! through here so the answer is the same everywhere.

use crate::error::{Error, Result};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Windows and (default) macOS volumes treat `Docs` and `docs` as the same
/// entry. Used for conservative comparisons such as the forbidden-root check.
pub const CASE_INSENSITIVE_FS: bool = cfg!(any(windows, target_os = "macos"));

pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Expands a leading `~` component (`~`, `~/x`, `~\x`) to the home directory.
/// Anything else is returned unchanged.
pub fn expand_tilde(p: &Path) -> PathBuf {
    let mut comps = p.components();
    match comps.next() {
        Some(Component::Normal(first)) if first == "~" => match home_dir() {
            Some(home) => home.join(comps.as_path()),
            None => p.to_path_buf(),
        },
        _ => p.to_path_buf(),
    }
}

/// Resolves `p` to a canonical absolute path without requiring it to exist.
///
/// * relative paths and `..` components are rejected (traversal),
/// * the longest existing prefix is canonicalized, so symlinks and junctions
///   in the parent chain are followed and cannot be used to escape a root,
/// * the not-yet-existing tail is appended verbatim.
pub fn resolve(p: &Path) -> Result<PathBuf> {
    if !p.is_absolute() {
        return Err(Error::Safety(format!(
            "path must be absolute: {}",
            p.display()
        )));
    }
    let mut clean = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                return Err(Error::Safety(format!(
                    "'..' is not allowed in paths: {}",
                    p.display()
                )))
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }

    let mut existing: &Path = &clean;
    let mut tail: Vec<&OsStr> = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(mut canon) => {
                for part in tail.iter().rev() {
                    canon.push(part);
                }
                return Ok(canon);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match (existing.parent(), existing.file_name()) {
                    (Some(parent), Some(name)) => {
                        tail.push(name);
                        existing = parent;
                    }
                    _ => return Err(Error::from(e)),
                }
            }
            Err(e) => return Err(Error::from(e)),
        }
    }
}

/// Like [`resolve`] but does not follow the final component, so a symlink
/// that *is* the entry is treated as the entry itself rather than as its
/// target. Use for files that are about to be moved, copied or created.
pub fn resolve_entry(p: &Path) -> Result<PathBuf> {
    let (parent, name) = match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => (parent, name),
        _ => {
            return Err(Error::Safety(format!(
                "path has no file name: {}",
                p.display()
            )))
        }
    };
    Ok(resolve(parent)?.join(name))
}

/// Component-wise prefix test. With `case_insensitive` the comparison ignores
/// case, which is the conservative choice for deny-lists on Windows/macOS.
pub fn starts_with(child: &Path, parent: &Path, case_insensitive: bool) -> bool {
    if !case_insensitive {
        return child.starts_with(parent);
    }
    let mut child_comps = child.components();
    parent.components().all(|p| {
        child_comps
            .next()
            .is_some_and(|c| fold(c.as_os_str()) == fold(p.as_os_str()))
    })
}

fn fold(s: &OsStr) -> String {
    s.to_string_lossy().to_lowercase()
}

/// True for `/`, `C:\` and friends.
pub fn is_fs_root(p: &Path) -> bool {
    p.parent().is_none()
}

/// Splits `a.tar.gz` into `("a.tar", "gz")`. Dotfiles such as `.env` have no
/// extension. The extension keeps its original case.
pub fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}
