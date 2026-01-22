use std::path::Path;

use crate::action::ActionErrorKind;

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

    std::fs::rename(&temp, &destination)
        .map_err(|e| ActionErrorKind::Rename(temp, destination.into(), e))?;

    Ok(())
}
