//! Filesystem operations. Correctness rules:
//!
//! 1. **Never** delete the source before the destination is verified.
//! 2. **Never** overwrite: moves use an atomic no-clobber primitive and
//!    copies open the destination with `create_new`. A `Replace` policy
//!    first moves the existing file into FileFlow's trash.
//! 3. Same-filesystem moves are a rename (a hard link plus unlink, so the
//!    destination cannot appear in between); cross-filesystem moves are
//!    copy + fsync + SHA-256 verify + delete.
//! 4. Large files stream through a fixed buffer, report progress and can be
//!    cancelled; a cancelled or failed copy leaves no partial destination.
//! 5. Collisions are resolved before we touch anything.
//!
//! Everything runs as a normal user; no privilege escalation is attempted.

use crate::error::{Error, Result};
use crate::model::{CollisionPolicy, OpKind, PlannedOp};
use crate::paths::split_name;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

const COPY_BUF: usize = 1024 * 1024;

/// Name of the per-folder trash used when the OS trash is unavailable and
/// for files displaced by a `Replace` policy.
pub const LOCAL_TRASH_DIR: &str = ".fileflow-trash";

/// `(bytes_done, bytes_total)`
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Outcome of an executed op — enough to journal it and to power undo.
#[derive(Debug, Clone)]
pub struct Executed {
    pub op_id: String,
    pub kind: OpKind,
    pub source: Option<PathBuf>,
    pub destination: Option<PathBuf>,
    pub bytes: u64,
    /// SHA-256 of the file, when it was computed (copies and cross-volume
    /// moves). Used to verify identity before an undo.
    pub source_hash: Option<[u8; 32]>,
    /// Modification time of the destination right after the op, in unix
    /// nanoseconds. A same-size edit made later is caught by this.
    pub dest_modified_ns: Option<i64>,
    /// Where the pre-existing destination went when the policy was `Replace`.
    pub replaced: Option<PathBuf>,
}

impl Executed {
    fn new(op: &PlannedOp) -> Self {
        Self {
            op_id: op.id.clone(),
            kind: op.kind,
            source: op.source.clone(),
            destination: None,
            bytes: 0,
            source_hash: None,
            dest_modified_ns: None,
            replaced: None,
        }
    }
}

#[derive(Clone)]
pub struct ExecuteOpts {
    /// Verify a checksum for `Copy`. Cross-volume moves are always verified
    /// because they delete the source afterwards.
    pub verify_hash: bool,
    /// When true, no writes happen; the executor reports what it would do.
    pub dry_run: bool,
    /// Route `Trash` through the native OS trash when compiled with the
    /// `os-trash` feature. Otherwise (or if the OS trash fails) the file goes
    /// to a local `.fileflow-trash/` folder, which keeps undo possible.
    pub use_os_trash: bool,
    /// Set to true to abort a running copy. The destination is removed and
    /// the source is left untouched.
    pub cancel: Option<Arc<AtomicBool>>,
    pub progress: Option<ProgressFn>,
}

impl Default for ExecuteOpts {
    fn default() -> Self {
        Self {
            verify_hash: true,
            dry_run: false,
            use_os_trash: cfg!(feature = "os-trash"),
            cancel: None,
            progress: None,
        }
    }
}

impl ExecuteOpts {
    pub fn dry_run() -> Self {
        Self {
            dry_run: true,
            ..Self::default()
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    }
}

/// Execute a single filesystem op. `Tag` and `Review` are bookkeeping, not
/// filesystem operations; the runner handles them.
pub fn execute(op: &PlannedOp, opts: &ExecuteOpts) -> Result<Executed> {
    match op.kind {
        OpKind::Move | OpKind::Rename => place(op, opts, false),
        OpKind::Copy => place(op, opts, true),
        OpKind::CreateFolder => create_folder(op, opts),
        OpKind::Trash => run_trash(op, opts),
        OpKind::Tag | OpKind::Review => Err(Error::Other(
            "tag and review operations do not touch the filesystem".into(),
        )),
    }
}

fn require<'a>(o: &'a Option<PathBuf>, what: &str) -> Result<&'a PathBuf> {
    o.as_ref()
        .ok_or_else(|| Error::Other(format!("missing {what}")))
}

fn create_folder(op: &PlannedOp, opts: &ExecuteOpts) -> Result<Executed> {
    let dst = require(&op.destination, "destination")?;
    if !opts.dry_run {
        fs::create_dir_all(dst)?;
    }
    let mut done = Executed::new(op);
    done.source = None;
    done.destination = Some(dst.clone());
    Ok(done)
}

fn place(op: &PlannedOp, opts: &ExecuteOpts, copy: bool) -> Result<Executed> {
    let src = require(&op.source, "source")?;
    let wanted = require(&op.destination, "destination")?;
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        return Err(Error::Safety(
            "folders cannot be moved or copied by a rule".into(),
        ));
    }
    let (dst, replace) = resolve_collision(wanted, op.collision)?;

    let mut done = Executed::new(op);
    done.destination = Some(dst.clone());
    done.bytes = meta.len();
    if opts.dry_run {
        return Ok(done);
    }

    ensure_parent(&dst)?;
    let replaced = if replace {
        Some(move_into_local_trash(&dst)?)
    } else {
        None
    };
    let moved = if copy {
        copy_file(src, &dst, opts, opts.verify_hash).map(|(bytes, hash)| (bytes, Some(hash)))
    } else {
        move_file(src, &dst, opts)
    };
    match moved {
        Ok((bytes, hash)) => {
            done.bytes = bytes;
            done.source_hash = hash;
            done.dest_modified_ns = modified_ns(&dst);
            done.replaced = replaced;
            Ok(done)
        }
        Err(e) => {
            // Put the displaced file back; the user must not lose it
            // because the replacement failed.
            if let Some(trashed) = replaced {
                let _ = rename_no_clobber(&trashed, &dst);
            }
            Err(e)
        }
    }
}

fn move_file(src: &Path, dst: &Path, opts: &ExecuteOpts) -> Result<(u64, Option<[u8; 32]>)> {
    match rename_no_clobber(src, dst) {
        Ok(()) => Ok((fs::metadata(dst).map(|m| m.len()).unwrap_or(0), None)),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(Error::Other(
            "another program created the destination while FileFlow was moving; nothing was changed"
                .into(),
        )),
        Err(e) if is_cross_device(&e) => {
            let (bytes, hash) = move_by_copy(src, dst, opts)?;
            Ok((bytes, Some(hash)))
        }
        Err(e) => Err(e.into()),
    }
}

/// The cross-volume move: copy, verify the checksum, and only then delete
/// the source. If the source cannot be deleted the copy is removed again, so
/// a failed move never leaves two files behind.
pub fn move_by_copy(src: &Path, dst: &Path, opts: &ExecuteOpts) -> Result<(u64, [u8; 32])> {
    let (bytes, hash) = copy_file(src, dst, opts, true)?;
    if let Err(e) = fs::remove_file(src) {
        let _ = fs::remove_file(dst);
        return Err(e.into());
    }
    Ok((bytes, hash))
}

/// Streams `src` to a brand-new `dst`, hashing as it goes. Returns the byte
/// count and the SHA-256 of what was read. On any failure — I/O error,
/// cancellation, source changed while copying, checksum mismatch — the
/// partial destination is removed.
fn copy_file(src: &Path, dst: &Path, opts: &ExecuteOpts, verify: bool) -> Result<(u64, [u8; 32])> {
    let mut reader = File::open(src)?;
    let before = reader.metadata()?;
    let mut writer = OpenOptions::new().write(true).create_new(true).open(dst)?;

    let result = (|| -> Result<(u64, [u8; 32])> {
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; COPY_BUF];
        let mut done = 0u64;
        loop {
            if opts.cancelled() {
                return Err(Error::Cancelled);
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            done += n as u64;
            if let Some(progress) = &opts.progress {
                progress(done, before.len());
            }
        }
        writer.sync_all()?;
        let hash: [u8; 32] = hasher.finalize().into();

        let after = fs::metadata(src)?;
        if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
            return Err(Error::Integrity(
                "the source changed while it was being copied".into(),
            ));
        }
        if verify && hash_file(dst)? != hash {
            return Err(Error::Integrity("checksum mismatch after copy".into()));
        }
        if let Ok(modified) = before.modified() {
            let _ = writer.set_modified(modified);
        }
        let _ = fs::set_permissions(dst, before.permissions());
        Ok((done, hash))
    })();

    if result.is_err() {
        drop(writer);
        let _ = fs::remove_file(dst);
    }
    result
}

/// Moves `src` to `dst` without ever replacing an existing `dst`.
///
/// A hard link is created first — it fails atomically with `AlreadyExists`
/// if the destination is taken — and only then is the source unlinked. Where
/// hard links are unavailable (FAT/exFAT, some network shares) or the source
/// is a symlink, a checked rename is used instead.
pub fn rename_no_clobber(src: &Path, dst: &Path) -> io::Result<()> {
    let src_is_link = fs::symlink_metadata(src).is_ok_and(|m| m.file_type().is_symlink());
    if !src_is_link {
        match fs::hard_link(src, dst) {
            Ok(()) => {
                if fs::remove_file(src).is_ok() {
                    return Ok(());
                }
                // The source could not be unlinked (a read-only file on
                // Windows, for example). Drop the extra name and fall back to
                // the checked rename below.
                if fs::remove_file(dst).is_err() {
                    return Err(io::Error::other(
                        "the file now has two names and neither could be removed; delete one by hand",
                    ));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists || is_cross_device(&e) => {
                return Err(e)
            }
            Err(_) => {}
        }
    }
    if dst.symlink_metadata().is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(src, dst)
}

fn run_trash(op: &PlannedOp, opts: &ExecuteOpts) -> Result<Executed> {
    let src = require(&op.source, "source")?;
    let meta = fs::symlink_metadata(src)?;
    let mut done = Executed::new(op);
    done.bytes = meta.len();
    if opts.dry_run {
        return Ok(done);
    }

    // The OS trash gives no knowable destination (macOS especially), so a
    // file sent there has no undo from FileFlow; the user restores it from
    // the system trash.
    #[cfg(feature = "os-trash")]
    if opts.use_os_trash && trash::delete(src).is_ok() {
        return Ok(done);
    }

    let dst = move_into_local_trash(src)?;
    done.destination = Some(dst.clone());
    done.dest_modified_ns = modified_ns(&dst);
    Ok(done)
}

/// Moves `path` into a `.fileflow-trash/` folder next to it, picking a free
/// name. Used for the Trash action and for files displaced by `Replace`.
pub fn move_into_local_trash(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("path has no parent folder".into()))?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::Other("path has no file name".into()))?;
    let dir = parent.join(LOCAL_TRASH_DIR);
    fs::create_dir_all(&dir)?;
    let mut candidate = dir.join(name);
    for _ in 0..10_000 {
        match rename_no_clobber(path, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                candidate = next_available(&candidate)
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::Other(
        "could not find a free name in the trash".into(),
    ))
}

fn ensure_parent(p: &Path) -> Result<()> {
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

pub fn is_cross_device(e: &io::Error) -> bool {
    // ErrorKind::CrossesDevices is not stable; match the raw OS error.
    // Unix: EXDEV (18). Windows: ERROR_NOT_SAME_DEVICE (17).
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(18)
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

/// Streaming SHA-256 of a file.
pub fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Modification time as unix nanoseconds.
pub fn modified_ns(path: &Path) -> Option<i64> {
    let nanos = fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    i64::try_from(nanos).ok()
}

/// Returns the path to use and whether an existing file there must be
/// displaced first (`Replace`).
fn resolve_collision(dst: &Path, policy: CollisionPolicy) -> Result<(PathBuf, bool)> {
    if dst.symlink_metadata().is_err() {
        return Ok((dst.to_path_buf(), false));
    }
    match policy {
        CollisionPolicy::Skip => Err(Error::Skipped(format!(
            "destination exists: {}",
            dst.display()
        ))),
        CollisionPolicy::Ask => Err(Error::NeedsInput(format!(
            "destination exists: {}",
            dst.display()
        ))),
        CollisionPolicy::Replace => Ok((dst.to_path_buf(), true)),
        CollisionPolicy::Rename => Ok((next_available(dst), false)),
    }
}

/// `foo.txt` -> `foo (1).txt`, `foo (2).txt`, …
pub fn next_available(dst: &Path) -> PathBuf {
    let parent = dst.parent().unwrap_or(Path::new(""));
    let name = dst.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let (stem, ext) = split_name(name);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            parent.join(format!("{stem} ({i})"))
        } else {
            parent.join(format!("{stem} ({i}).{ext}"))
        };
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
    }
    parent.join(format!("{name}.{}", uuid::Uuid::new_v4()))
}
