use std::path::{Path, PathBuf};

use nix::unistd::{AccessFlags, access};

use crate::action::ActionErrorKind;

/// Find an executable in PATH, similar to the `which` command.
/// Returns the full path to the executable if found.
pub fn which(executable: impl AsRef<Path>) -> Option<PathBuf> {
    let executable = executable.as_ref();

    // If it's already an absolute path, check if it's executable
    if executable.is_absolute() {
        return if access(executable, AccessFlags::X_OK).is_ok() {
            Some(executable.to_path_buf())
        } else {
            None
        };
    }

    // Search in PATH
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let full_path = dir.join(executable);
        if access(&full_path, AccessFlags::X_OK).is_ok() {
            return Some(full_path);
        }
    }

    None
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OnMissing {
    Ignore,
    Error,
}

#[tracing::instrument(skip(path), fields(path = %path.display()))]
pub(crate) fn remove_file(path: &Path, on_missing: OnMissing) -> std::io::Result<()> {
    tracing::trace!("Removing file");
    let res = std::fs::remove_file(path);
    match res {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && on_missing == OnMissing::Ignore => {
            tracing::trace!("Ignoring nonexistent file");
            Ok(())
        },
        e @ Err(_) => e,
    }
}

#[tracing::instrument(skip(path), fields(path = %path.display()))]
pub(crate) fn remove_dir_all(path: &Path, on_missing: OnMissing) -> std::io::Result<()> {
    tracing::trace!("Removing directory and all contents");
    let res = std::fs::remove_dir_all(path);
    match res {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && on_missing == OnMissing::Ignore => {
            tracing::trace!("Ignoring nonexistent directory");
            Ok(())
        },
        e @ Err(_) => e,
    }
}

pub(crate) fn write_atomic(destination: &Path, body: &str) -> Result<(), ActionErrorKind> {
    let temp = destination.with_extension("tmp");

    std::fs::write(&temp, body).map_err(|e| ActionErrorKind::Write(temp.to_owned(), e))?;

    std::fs::rename(&temp, destination)
        .map_err(|e| ActionErrorKind::Rename(temp, destination.into(), e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_which_finds_ls() {
        let result = which("ls");
        assert!(result.is_some(), "ls should be found in PATH");
        let path = result.unwrap();
        assert!(path.is_absolute(), "returned path should be absolute");
        assert!(path.ends_with("ls"), "path should end with 'ls'");
    }

    #[test]
    fn test_which_nonexistent() {
        let result = which("this-command-definitely-does-not-exist-12345");
        assert!(result.is_none(), "nonexistent command should return None");
    }
}
