use std::fs::{OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tracing::{Span, span};

use crate::action::{
    Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction,
};
use crate::util::OnMissing;

/// Where the nix-mountd binary is installed. It must live outside `/nix` since
/// its job is to keep `/nix` mounted.
pub const NIX_MOUNTD_DEST: &str = "/usr/local/bin/nix-mountd";

/// Install the embedded nix-mountd binary onto the target.
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "provision_nix_mountd")]
pub struct ProvisionNixMountd {
    dest: PathBuf,
}

impl ProvisionNixMountd {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan() -> Result<StatefulAction<Self>, ActionError> {
        Ok(StatefulAction::uncompleted(Self {
            dest: NIX_MOUNTD_DEST.into(),
        }))
    }
}

#[typetag::serde(name = "provision_nix_mountd")]
impl Action for ProvisionNixMountd {
    fn action_tag() -> ActionTag {
        ActionTag("provision_nix_mountd")
    }
    fn tracing_synopsis(&self) -> String {
        format!("Install nix-mountd to `{}`", self.dest.display())
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "provision_nix_mountd", dest = %self.dest.display())
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        crate::util::remove_file(&self.dest, OnMissing::Ignore)
            .map_err(|e| Self::error(ActionErrorKind::Remove(self.dest.clone(), e)))?;

        if let Some(parent) = self.dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Self::error(ActionErrorKind::CreateDirectory(parent.into(), e)))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.dest)
            .map_err(|e| Self::error(ActionErrorKind::Open(self.dest.clone(), e)))?;
        file.write_all(crate::distribution::NIX_MOUNTD_BINARY)
            .map_err(|e| Self::error(ActionErrorKind::Write(self.dest.clone(), e)))?;
        file.set_permissions(Permissions::from_mode(0o555))
            .map_err(|e| Self::error(ActionErrorKind::Write(self.dest.clone(), e)))?;

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            format!("Remove `{}`", self.dest.display()),
            vec![],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        crate::util::remove_file(&self.dest, OnMissing::Ignore)
            .map_err(|e| Self::error(ActionErrorKind::Remove(self.dest.clone(), e)))?;
        Ok(())
    }
}
