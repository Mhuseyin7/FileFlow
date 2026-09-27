//! Filesystem operations. Correctness rules:
//!
//! 1. **Never** delete the source before the destination is verified.
//! 2. Same-filesystem moves use `std::fs::rename` (atomic on POSIX / Win32).
//! 3. Cross-filesystem moves are copy + fsync + optional hash verify + delete.
//! 4. Large files stream through an 8 MiB buffer; no full-file loads.
//! 5. Collisions are resolved before we touch anything.
//!
//! Everything runs as a normal user; no privilege escalation is attempted.

use crate::error::{Error, Result};
use crate::model::{CollisionPolicy, OpKind, PlannedOp};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const COPY_BUF: usize = 8 * 1024 * 1024;

/// Outcome of an executed op — enough to journal it and to power undo.
#[derive(Debug, Clone)]
pub struct Executed {
    pub op_id: String,
    pub kind: OpKind,
    pub source: Option<PathBuf>,
    pub destination: Option<PathBuf>,
    pub bytes: u64,
    /// SHA-256 of the source (pre-move), used to verify undo safety. Only set
    /// for `Move`/`Copy`/`Rename` when `verify_hash` is on.
    pub source_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecuteOpts {
    /// Verify checksum on cross-filesystem moves and on Copy actions before
    /// deleting the source. Defaults on.
    pub verify_hash: bool,
    /// When true, no writes happen; the executor reports what it would do.
    pub dry_run: bool,
    /// Route `Trash` actions through the native OS trash (when compiled with
    /// the `os-trash` feature). When false or unavailable, we fall back to a
    /// local `.fileflow-trash/` directory next to the source's parent — that
    /// path is knowable, so undo can restore the file.
    pub use_os_trash: bool,
}

impl Default for ExecuteOpts {
    fn default() -> Self {
        Self { verify_hash: true, dry_run: false, use_os_trash: cfg!(feature = "os-trash") }
    }
}

/// Execute a single planned op. Callers are expected to have run the safety
/// validator; this function still enforces the invariants because defense in
/// depth is cheap.
pub fn execute(op: &PlannedOp, opts: ExecuteOpts) -> Result<Executed> {
    match op.kind {
        OpKind::Move => run_move(op, opts),
        OpKind::Copy => run_copy(op, opts),
        OpKind::Rename => run_rename(op, opts),
        OpKind::CreateFolder => run_create_folder(op, opts),
        OpKind::Tag => Ok(Executed {
            op_id: op.id.clone(),
            kind: op.kind,
            source: op.source.clone(),
            destination: None,
            bytes: 0,
            source_hash: None,
        }),
        OpKind::Review => Ok(Executed {
            op_id: op.id.clone(),
            kind: op.kind,
            source: op.source.clone(),
            destination: None,
            bytes: 0,
            source_hash: None,
        }),
        OpKind::Trash => run_trash(op, opts),
    }
}

fn require<'a>(o: &'a Option<PathBuf>, what: &str) -> Result<&'a PathBuf> {
    o.as_ref().ok_or_else(|| Error::Other(format!("missing {what}")))
}

fn run_create_folder(op: &PlannedOp, opts: ExecuteOpts) -> Result<Executed> {
    let dst = require(&op.destination, "destination")?;
    if !opts.dry_run {
        fs::create_dir_all(dst)?;
    }
    Ok(Executed {
        op_id: op.id.clone(),
        kind: op.kind,
        source: None,
        destination: Some(dst.clone()),
        bytes: 0,
        source_hash: None,
    })
}

fn run_move(op: &PlannedOp, opts: ExecuteOpts) -> Result<Executed> {
    let src = require(&op.source, "source")?;
    let dst = require(&op.destination, "destination")?;
    let dst = resolve_collision(dst, op.collision)?;

    if opts.dry_run {
        let bytes = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
        return Ok(Executed {
            op_id: op.id.clone(),
            kind: op.kind,
            source: Some(src.clone()),
            destination: Some(dst),
            bytes,
            source_hash: None,
        });
    }

    ensure_parent(&dst)?;
    // Try atomic rename first.
    match fs::rename(src, &dst) {
        Ok(()) => {
            let bytes = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
            Ok(Executed {
                op_id: op.id.clone(),
                kind: op.kind,
                source: Some(src.clone()),
                destination: Some(dst),
                bytes,
                source_hash: None,
            })
        }
        Err(e) if is_cross_device(&e) => {
            // Cross-filesystem: copy + verify + delete source.
            let (bytes, hash) = copy_verify(src, &dst, opts.verify_hash)?;
            fs::remove_file(src)?;
            Ok(Executed {
                op_id: op.id.clone(),
                kind: op.kind,
                source: Some(src.clone()),
                destination: Some(dst),
                bytes,
                source_hash: hash,
            })
        }
        Err(e) => Err(Error::Io(e)),
    }
}

fn run_copy(op: &PlannedOp, opts: ExecuteOpts) -> Result<Executed> {
    let src = require(&op.source, "source")?;
    let dst = require(&op.destination, "destination")?;
    let dst = resolve_collision(dst, op.collision)?;
    if opts.dry_run {
        let bytes = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
        return Ok(Executed {
            op_id: op.id.clone(),
            kind: op.kind,
            source: Some(src.clone()),
            destination: Some(dst),
            bytes,
            source_hash: None,
        });
    }
    ensure_parent(&dst)?;
    let (bytes, hash) = copy_verify(src, &dst, opts.verify_hash)?;
    Ok(Executed {
        op_id: op.id.clone(),
        kind: op.kind,
        source: Some(src.clone()),
        destination: Some(dst),
        bytes,
        source_hash: hash,
    })
}

fn run_rename(op: &PlannedOp, opts: ExecuteOpts) -> Result<Executed> {
    let src = require(&op.source, "source")?;
    let dst = require(&op.destination, "destination")?;
    let dst = resolve_collision(dst, op.collision)?;
    if opts.dry_run {
        let bytes = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
        return Ok(Executed {
            op_id: op.id.clone(),
            kind: op.kind,
            source: Some(src.clone()),
            destination: Some(dst),
            bytes,
            source_hash: None,
        });
    }
    ensure_parent(&dst)?;
    fs::rename(src, &dst)?;
    let bytes = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
    Ok(Executed {
        op_id: op.id.clone(),
        kind: op.kind,
        source: Some(src.clone()),
        destination: Some(dst),
        bytes,
        source_hash: None,
    })
}

fn run_trash(op: &PlannedOp, opts: ExecuteOpts) -> Result<Executed> {
    let src = require(&op.source, "source")?;
    if opts.dry_run {
        return Ok(Executed {
            op_id: op.id.clone(),
            kind: op.kind,
            source: Some(src.clone()),
            destination: None,
            bytes: 0,
            source_hash: None,
        });
    }
    let bytes = fs::metadata(src).map(|m| m.len()).unwrap_or(0);

    // Native OS trash first, when the caller opted in AND the feature is
    // compiled in. The destination path is not knowable in the OS trash
    // (macOS especially), so we clear `destination` — the journal records
    // `undo_status = not_applicable` and points the user at their OS trash.
    #[cfg(feature = "os-trash")]
    if opts.use_os_trash {
        match trash::delete(src) {
            Ok(()) => {
                return Ok(Executed {
                    op_id: op.id.clone(),
                    kind: op.kind,
                    source: Some(src.clone()),
                    destination: None,
                    bytes,
                    source_hash: None,
                });
            }
            Err(_e) => {
                // Fall through to local trash below rather than surface a
                // partial-failure to the user.
            }
        }
    }

    // Local `.fileflow-trash/` fallback (always available). Undo restores
    // via `fs::rename` back to the original path.
    let trash = trash_dir_for(src)?;
    fs::create_dir_all(&trash)?;
    let dst = trash.join(unique_name_in(&trash, src));
    fs::rename(src, &dst)?;
    let bytes = fs::metadata(&dst).map(|m| m.len()).unwrap_or(bytes);
    Ok(Executed {
        op_id: op.id.clone(),
        kind: op.kind,
        source: Some(src.clone()),
        destination: Some(dst),
        bytes,
        source_hash: None,
    })
}

fn trash_dir_for(p: &Path) -> Result<PathBuf> {
    let parent = p
        .parent()
        .ok_or_else(|| Error::Other("no parent".into()))?;
    Ok(parent.join(".fileflow-trash"))
}

fn unique_name_in(dir: &Path, src: &Path) -> String {
    let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = split_stem_ext(name);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            format!("{stem} ({i})")
        } else {
            format!("{stem} ({i}).{ext}")
        };
        if !dir.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{name}.{}", uuid::Uuid::new_v4())
}

fn split_stem_ext(name: &str) -> (String, String) {
    if let Some((s, e)) = name.rsplit_once('.') {
        if !s.is_empty() {
            return (s.to_string(), e.to_string());
        }
    }
    (name.to_string(), String::new())
}

fn ensure_parent(p: &Path) -> Result<()> {
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn is_cross_device(e: &std::io::Error) -> bool {
    // ErrorKind::CrossesDevices is nightly-only. Fall back on raw_os_error.
    #[cfg(unix)]
    {
        matches!(e.raw_os_error(), Some(libc_exdev) if libc_exdev == 18)
    }
    #[cfg(windows)]
    {
        // ERROR_NOT_SAME_DEVICE = 17
        matches!(e.raw_os_error(), Some(17))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

fn copy_verify(src: &Path, dst: &Path, verify: bool) -> Result<(u64, Option<[u8; 32]>)> {
    let mut r = File::open(src)?;
    let mut w = File::create(dst)?;
    let mut buf = vec![0u8; COPY_BUF];
    let mut total = 0u64;
    let mut hasher = if verify { Some(Sha256::new()) } else { None };
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n])?;
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        total += n as u64;
    }
    w.sync_all()?;
    let src_hash = hasher.map(|h| {
        let out = h.finalize();
        let mut a = [0u8; 32];
        a.copy_from_slice(&out);
        a
    });
    if let Some(sh) = src_hash {
        // Read destination back and verify.
        let mut r2 = File::open(dst)?;
        let mut h2 = Sha256::new();
        loop {
            let n = r2.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h2.update(&buf[..n]);
        }
        let out = h2.finalize();
        if out.as_slice() != sh {
            // Destination is corrupt — remove it, propagate.
            let _ = fs::remove_file(dst);
            return Err(Error::Integrity("checksum mismatch after copy".into()));
        }
    }
    Ok((total, src_hash))
}

fn resolve_collision(dst: &Path, policy: CollisionPolicy) -> Result<PathBuf> {
    if !dst.exists() {
        return Ok(dst.to_path_buf());
    }
    match policy {
        CollisionPolicy::Skip => Err(Error::Conflict(format!("skip: {}", dst.display()))),
        CollisionPolicy::Ask => Err(Error::Conflict(format!(
            "ask: {} (ui must prompt user)",
            dst.display()
        ))),
        CollisionPolicy::Replace => Ok(dst.to_path_buf()),
        CollisionPolicy::Rename => Ok(next_available(dst)),
    }
}

/// `foo.txt` -> `foo (1).txt`, `foo (2).txt`, …
pub fn next_available(dst: &Path) -> PathBuf {
    let parent = dst.parent().unwrap_or(Path::new(""));
    let name = dst.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let (stem, ext) = split_stem_ext(name);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            parent.join(format!("{stem} ({i})"))
        } else {
            parent.join(format!("{stem} ({i}).{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{name}.{}", uuid::Uuid::new_v4()))
}
