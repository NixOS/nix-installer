//! Atomic file writes.

use std::{
    fs::File,
    io,
    io::Write as _,
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes through a sibling temp file and a rename, so readers never see a
/// partial write. `path` must be absolute. `mode: None` uses the process
/// default (`0o666` masked by the umask).
pub fn atomic_write(path: &Path, contents: &[u8], mode: Option<u32>) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(invalid_path(path, "must be absolute"));
    }
    let parent = parent_of(path)?;
    let tmp = temp_sibling(path);
    match write_and_rename(&tmp, path, contents, mode) {
        Ok(()) => {
            sync_dir_best_effort(parent);
            Ok(())
        },
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        },
    }
}

/// `parent()` is `None` only for `/`.
fn parent_of(path: &Path) -> io::Result<&Path> {
    path.parent()
        .ok_or_else(|| invalid_path(path, "has no parent directory"))
}

fn invalid_path(path: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` {why}", path.display()),
    )
}

/// A sibling, so the rename stays on one filesystem.
fn temp_sibling(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".nix-installer-tmp.{}.{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed),
    ))
}

fn write_and_rename(tmp: &Path, path: &Path, contents: &[u8], mode: Option<u32>) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    if let Some(mode) = mode {
        options.mode(mode);
    }
    let mut file = options.open(tmp)?;
    file.write_all(contents)?;
    if let Some(mode) = mode {
        set_exact_mode(&file, mode)?;
    }
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)
}

/// The rename has already happened, so a failed directory sync is logged
/// rather than reported as a failed write.
/// `OpenOptionsExt::mode` is subject to the umask, so the requested mode
/// is applied again once the file exists.
fn set_exact_mode(file: &File, mode: u32) -> io::Result<()> {
    file.set_permissions(std::fs::Permissions::from_mode(mode))
}

fn sync_dir_best_effort(dir: &Path) {
    if let Err(err) = File::open(dir).and_then(|dir| dir.sync_all()) {
        tracing::warn!("syncing `{}`: {err}", dir.display());
    }
}
