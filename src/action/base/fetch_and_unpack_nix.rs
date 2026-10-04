use core::time::Duration;
use std::path::PathBuf;
use std::{io::Cursor, time::SystemTime};

use tracing::{Span, span};
use walkdir::WalkDir;

use crate::{
    action::{Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction},
    settings::embedded_nix_tarball,
    util::OnMissing,
};

/**
Unpack the embedded Nix tarball to the destination directory
*/
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "fetch_and_unpack_nix")]
pub struct FetchAndUnpackNix {
    dest: PathBuf,
}

impl FetchAndUnpackNix {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(dest: PathBuf) -> Result<StatefulAction<Self>, ActionError> {
        Ok(Self { dest }.into())
    }
}

#[typetag::serde(name = "fetch_and_unpack_nix")]
impl Action for FetchAndUnpackNix {
    fn action_tag() -> ActionTag {
        ActionTag("fetch_and_unpack_nix")
    }

    fn tracing_synopsis(&self) -> String {
        format!("Unpack embedded Nix to `{}`", self.dest.display())
    }

    fn tracing_span(&self) -> Span {
        span!(
            tracing::Level::DEBUG,
            "fetch_and_unpack_nix",
            dest = tracing::field::display(self.dest.display()),
        )
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        tracing::trace!("Unpacking embedded tar.zst");

        // Remove destination if it exists (from a previous failed install)
        if self.dest.exists() {
            crate::util::remove_dir_all(&self.dest, OnMissing::Ignore)
                .map_err(|e| Self::error(ActionErrorKind::Remove(self.dest.clone(), e)))?;
        }

        // Decompress zstd
        let zstd_reader = Cursor::new(embedded_nix_tarball().map_err(Self::error)?);
        let tar_data =
            zstd::decode_all(zstd_reader).map_err(|e| Self::error(UnpackError::Zstd(e)))?;

        // Unpack tar
        let mut archive = tar::Archive::new(Cursor::new(tar_data));
        archive.set_preserve_permissions(true);
        // NOTE: Sigh... tar-rs forgets to preserve directories mtime.
        archive.set_preserve_mtime(true);
        archive.set_unpack_xattrs(true);
        archive
            .unpack(&self.dest)
            .map_err(|e| Self::error(UnpackError::Unarchive(e)))?;

        // A bit of an oversimplification, but the only thing that needs correct mtime
        // is the store - everything else is irrelevant.
        let store_mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1);

        // Because tar-rs is borked we have to set mtime on directories ourselves.
        for entry in WalkDir::new(&self.dest).into_iter() {
            let entry = entry.map_err(|e| Self::error(UnpackError::Unarchive(e.into())))?;

            if entry.file_type().is_dir() {
                filetime::set_file_mtime(entry.path(), store_mtime.into())
                    .map_err(|e| Self::error(UnpackError::Unarchive(e)))?;
            }
        }

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![/* Deliberately empty -- this is a noop */]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        Ok(())
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error("Zstd decompression error")]
    Zstd(#[source] std::io::Error),
    #[error("Tar extraction error")]
    Unarchive(#[source] std::io::Error),
}

impl From<UnpackError> for ActionErrorKind {
    fn from(val: UnpackError) -> Self {
        ActionErrorKind::Custom(Box::new(val))
    }
}
