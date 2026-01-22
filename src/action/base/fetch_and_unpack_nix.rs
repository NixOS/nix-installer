use std::io::Cursor;
use std::path::PathBuf;

use tracing::{span, Span};

use crate::{
    action::{Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction},
    settings::{EMBEDDED_NIX_TARBALL, NIX_VERSION},
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
        format!(
            "Unpack embedded Nix {} to `{}`",
            NIX_VERSION.trim(),
            self.dest.display()
        )
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
        let zstd_reader = Cursor::new(EMBEDDED_NIX_TARBALL);
        let tar_data =
            zstd::decode_all(zstd_reader).map_err(|e| Self::error(UnpackError::Zstd(e)))?;

        // Unpack tar
        let mut archive = tar::Archive::new(Cursor::new(tar_data));
        archive.set_preserve_permissions(true);
        archive.set_preserve_mtime(true);
        archive.set_unpack_xattrs(true);
        archive
            .unpack(&self.dest)
            .map_err(|e| Self::error(UnpackError::Unarchive(e)))?;

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
