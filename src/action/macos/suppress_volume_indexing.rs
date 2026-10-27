use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{Span, span};

use crate::action::{
    Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction,
};
use crate::execute_command;

/**
Hide the Nix volume from Finder and stop fseventsd journaling on it.

On large Nix stores (hundreds of thousands of entries), Finder and the
system open/save panel XPC service enumerate `/nix/store` via the synthetic
firmlink under `/` and cache one `_FileCache` + `NSURL` object per entry in
`DesktopServicesPriv`, leaking on the order of a gigabyte of RSS and burning
CPU re-syncing on every fsevent. The `nobrowse` fstab option hides the
*volume* from the sidebar (and from Spotlight/mds), but not the firmlink
directory entry under `/` that Finder walks.

This action sets the BSD `UF_HIDDEN` flag on the mount point so directory
enumeration in Finder/NSOpenPanel skips it, and drops the Apple-documented
`.fseventsd/no_log` marker so fseventsd stops writing its event journal
during builds and GC. Spotlight needs no extra marker: `nobrowse` already
covers it, and `.metadata_never_index` is unreliable on recent macOS.
 */
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "suppress_volume_indexing")]
pub struct SuppressVolumeIndexing {
    mount_point: PathBuf,
}

impl SuppressVolumeIndexing {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(mount_point: impl AsRef<Path>) -> Result<StatefulAction<Self>, ActionError> {
        Ok(Self {
            mount_point: mount_point.as_ref().to_path_buf(),
        }
        .into())
    }
}

#[typetag::serde(name = "suppress_volume_indexing")]
impl Action for SuppressVolumeIndexing {
    fn action_tag() -> ActionTag {
        ActionTag("suppress_volume_indexing")
    }
    fn tracing_synopsis(&self) -> String {
        format!(
            "Hide `{}` from Finder and disable fseventsd journaling on it",
            self.mount_point.display()
        )
    }

    fn tracing_span(&self) -> Span {
        span!(
            tracing::Level::DEBUG,
            "suppress_volume_indexing",
            mount_point = %self.mount_point.display(),
        )
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        let fseventsd_dir = self.mount_point.join(".fseventsd");
        fs::create_dir_all(&fseventsd_dir)
            .map_err(|e| ActionErrorKind::CreateDirectory(fseventsd_dir.clone(), e))
            .map_err(Self::error)?;
        let no_log = fseventsd_dir.join("no_log");
        fs::write(&no_log, b"")
            .map_err(|e| ActionErrorKind::Write(no_log, e))
            .map_err(Self::error)?;

        execute_command(
            Command::new("/usr/bin/chflags")
                .arg("hidden")
                .arg(&self.mount_point)
                .stdin(std::process::Stdio::null()),
        )
        .map_err(Self::error)?;

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            format!(
                "Remove fseventsd opt-out marker and unhide `{}`",
                self.mount_point.display()
            ),
            vec![],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        // Best-effort: uninstall removes the volume anyway, so just log failures.
        let no_log = self.mount_point.join(".fseventsd").join("no_log");
        if let Err(err) = fs::remove_file(&no_log)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(?err, path = %no_log.display(), "Could not remove fseventsd opt-out marker");
        }

        if let Err(err) = execute_command(
            Command::new("/usr/bin/chflags")
                .arg("nohidden")
                .arg(&self.mount_point)
                .stdin(std::process::Stdio::null()),
        ) {
            tracing::warn!(?err, path = %self.mount_point.display(), "Could not clear hidden flag");
        }

        Ok(())
    }
}
