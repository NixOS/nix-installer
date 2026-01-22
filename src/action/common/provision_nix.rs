use std::str::FromStr;
use tracing::{span, Span};

use super::CreateNixTree;
use crate::{
    action::{
        base::{FetchAndUnpackNix, MoveUnpackedNix},
        Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction,
    },
    settings::{CommonSettings, UrlOrPath, SCRATCH_DIR},
};
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;

pub(crate) const NIX_STORE_LOCATION: &str = "/nix/store";

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
        let url_or_path = settings.nix_package_url.clone().unwrap_or_else(|| {
            UrlOrPath::from_str(crate::settings::NIX_TARBALL_URL)
                .expect("Fault: the built-in Nix tarball URL does not parse.")
        });

        let fetch_nix = FetchAndUnpackNix::plan(
            url_or_path,
            PathBuf::from(SCRATCH_DIR),
            settings.proxy.clone(),
            settings.ssl_cert_file.clone(),
        )?;

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
                "Will update existing files in the Nix Store to use the Nix build group ID {nix_store_gid}"
            )],
        ));

        buf
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        // Execute sequentially (no async parallelism needed)
        self.fetch_nix.try_execute().map_err(Self::error)?;

        self.create_nix_tree.try_execute().map_err(Self::error)?;

        self.move_unpacked_nix.try_execute().map_err(Self::error)?;

        ensure_nix_store_group(self.nix_store_gid).map_err(Self::error)?;

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

/// Everything under /nix/store should be group-owned by the nix_build_group_id.
/// This function walks /nix/store and makes sure that is true.
fn ensure_nix_store_group(nix_store_gid: u32) -> Result<(), ActionErrorKind> {
    let entryiter = walkdir::WalkDir::new(NIX_STORE_LOCATION)
        .follow_links(false)
        .same_file_system(true)
        .contents_first(true)
        .into_iter()
        .filter_entry(|entry| {
            let dominated_by_trustworthy_builder_process =
                // The current directory...
                entry.path() == std::path::Path::new(NIX_STORE_LOCATION)
                // ... or immediate children of the current directory
                // Children of children are owned by the build process, and we don't
                // want to own them to root.
                || entry.path().parent() == Some(std::path::Path::new(NIX_STORE_LOCATION));

            dominated_by_trustworthy_builder_process
        })
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(e) => {
                tracing::warn!(%e, "Failed to get entry in /nix/store");
                None
            },
        })
        .filter_map(|entry| match entry.metadata() {
            Ok(metadata) => Some((entry, metadata)),
            Err(e) => {
                tracing::warn!(
                    path = %entry.path().to_string_lossy(),
                    %e,
                    "Failed to read ownership and mode data"
                );
                None
            },
        })
        .filter_map(|(entry, metadata)| {
            // Dirents that are already the right group are to be skipped
            if metadata.gid() == nix_store_gid {
                return None;
            }

            Some((entry, metadata))
        });

    for (entry, _metadata) in entryiter {
        tracing::debug!(
            path = %entry.path().to_string_lossy(),
            "Re-owning path's group to {nix_store_gid}"
        );

        if let Err(e) = std::os::unix::fs::lchown(entry.path(), None, Some(nix_store_gid)) {
            tracing::warn!(
                path = %entry.path().to_string_lossy(),
                %e,
                "Failed to set the group to {nix_store_gid}"
            );
        }
    }
    Ok(())
}
