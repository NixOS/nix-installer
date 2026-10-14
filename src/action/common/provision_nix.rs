use tracing::{Span, span};

use super::CreateNixTree;
use crate::{
    action::{
        Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction,
        base::{FetchAndUnpackNix, MoveUnpackedNix},
    },
    settings::{CommonSettings, SCRATCH_DIR},
};
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;

pub(crate) const NIX_STORE_LOCATION: &str = "/nix/store";

/// Group writable plus the sticky bit, matching what Nix itself applies on first use.
const NIX_STORE_MODE: u32 = 0o1775;

/**
Place Nix and it's requirements onto the target
 */
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "provision_nix")]
pub struct ProvisionNix {
    nix_store_gid: u32,

    pub(crate) fetch_nix: StatefulAction<FetchAndUnpackNix>,
    pub(crate) create_nix_tree: StatefulAction<CreateNixTree>,
    pub(crate) move_unpacked_nix: StatefulAction<MoveUnpackedNix>,
}

impl ProvisionNix {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(settings: &CommonSettings) -> Result<StatefulAction<Self>, ActionError> {
        let fetch_nix = FetchAndUnpackNix::plan(PathBuf::from(SCRATCH_DIR))?;

        let create_nix_tree = CreateNixTree::plan().map_err(Self::error)?;
        let move_unpacked_nix =
            MoveUnpackedNix::plan(PathBuf::from(SCRATCH_DIR)).map_err(Self::error)?;
        Ok(Self {
            nix_store_gid: settings.nix_build_group_id,
            fetch_nix,
            create_nix_tree,
            move_unpacked_nix,
        }
        .into())
    }
}

#[typetag::serde(name = "provision_nix")]
impl Action for ProvisionNix {
    fn action_tag() -> ActionTag {
        ActionTag("provision_nix")
    }
    fn tracing_synopsis(&self) -> String {
        "Provision Nix".to_string()
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "provision_nix",)
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        let Self {
            fetch_nix,
            create_nix_tree,
            move_unpacked_nix,
            nix_store_gid,
        } = &self;

        let mut buf = Vec::default();
        buf.append(&mut fetch_nix.describe_execute());

        buf.append(&mut create_nix_tree.describe_execute());
        buf.append(&mut move_unpacked_nix.describe_execute());

        buf.push(ActionDescription::new(
            "Synchronize /nix/store ownership".to_string(),
            vec![format!(
                "Will set /nix/store to mode {NIX_STORE_MODE:o} and group ID {nix_store_gid}, and the Nix installed inside it to User ID 0, Group ID 0"
            )],
        ));

        buf
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        self.fetch_nix.try_execute().map_err(Self::error)?;

        self.create_nix_tree.try_execute().map_err(Self::error)?;

        self.move_unpacked_nix.try_execute().map_err(Self::error)?;

        ensure_nix_store_group(self.nix_store_gid);

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        let Self {
            fetch_nix,
            create_nix_tree,
            move_unpacked_nix,
            nix_store_gid: _,
        } = &self;

        let mut buf = Vec::default();
        buf.append(&mut move_unpacked_nix.describe_revert());
        buf.append(&mut create_nix_tree.describe_revert());

        buf.append(&mut fetch_nix.describe_revert());
        buf
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        let mut errors = vec![];

        if let Err(err) = self.fetch_nix.try_revert() {
            errors.push(err)
        }

        if let Err(err) = self.create_nix_tree.try_revert() {
            errors.push(err)
        }

        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            Err(errors
                .into_iter()
                .next()
                .expect("Expected 1 len Vec to have at least 1 item"))
        } else {
            Err(Self::error(ActionErrorKind::MultipleChildren(errors)))
        }
    }
}

/// `/nix/store` itself is group-owned by `nix_store_gid` and group writable, so build users can
/// create outputs there. Everything inside it, recursively, is owned by 0:0.
fn ensure_nix_store_group(nix_store_gid: u32) {
    let store = Path::new(NIX_STORE_LOCATION);
    fix_ownership(store, 0, nix_store_gid, Some(NIX_STORE_MODE));

    for path in WalkDir::new(store)
        .min_depth(1)
        .follow_links(false)
        .same_file_system(true)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.into_path()),
            Err(e) => {
                let path = e
                    .path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                tracing::warn!(%e, path, "Failed to get entry in /nix/store");
                None
            },
        })
    {
        fix_ownership(&path, 0, 0, None);
    }
}

/// Chown, and optionally chmod, unless already correct. Failures are only warnings: a single
/// path we cannot fix should not abort the install.
fn fix_ownership(path: &Path, uid: u32, gid: u32, mode: Option<u32>) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        tracing::warn!(path = %path.display(), "Failed to read ownership and mode data");
        return;
    };

    let mode = mode.filter(|mode| metadata.mode() & 0o7777 != *mode);

    if metadata.uid() == uid && metadata.gid() == gid && mode.is_none() {
        return;
    }

    if metadata.uid() != uid || metadata.gid() != gid {
        tracing::debug!(path = %path.display(), "Re-owning path to {uid}:{gid}");
        if let Err(e) = std::os::unix::fs::lchown(path, Some(uid), Some(gid)) {
            tracing::warn!(
                path = %path.display(),
                %e,
                "Failed to set the owner:group to {uid}:{gid}"
            );
        }
    }

    if let Some(mode) = mode {
        tracing::debug!(path = %path.display(), "Setting mode to {mode:o}");
        if let Err(e) =
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))
        {
            tracing::warn!(path = %path.display(), %e, "Failed to set the mode to {mode:o}");
        }
    }
}
