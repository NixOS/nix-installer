use std::path::PathBuf;

use crate::{
    action::{ActionError, ActionErrorKind, ActionTag, StatefulAction},
    execute_command,
    settings::{NIX_STORE_PATH, NSS_CACERT_STORE_PATH},
};

use std::process::Command;
use tracing::{span, Span};

use crate::action::{Action, ActionDescription};

use crate::action::base::CreateFile;

/**
Setup the default system channel with nixpkgs-unstable.
 */
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct SetupChannels {
    create_file: StatefulAction<CreateFile>,
}

impl SetupChannels {
    fn get_root_home() -> Result<PathBuf, SetupChannelsError> {
        // Use nix::unistd to get the actual root user's home, not $HOME env var
        // This avoids issues where sudo preserves HOME on some platforms (macOS)
        use nix::unistd::{Uid, User};

        if Uid::effective().is_root() {
            User::from_uid(Uid::from_raw(0))
                .ok()
                .flatten()
                .map(|user| user.dir)
                .ok_or(SetupChannelsError::NoRootHome)
        } else {
            dirs::home_dir().ok_or(SetupChannelsError::NoRootHome)
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan() -> Result<StatefulAction<Self>, ActionError> {
        let create_file = CreateFile::plan(
            Self::get_root_home()
                .map_err(Self::error)?
                .join(".nix-channels"),
            None,
            None,
            0o664,
            "https://nixos.org/channels/nixpkgs-unstable nixpkgs\n".to_string(),
            false,
        )?;
        Ok(Self { create_file }.into())
    }
}

#[typetag::serde(name = "setup_channels")]
impl Action for SetupChannels {
    fn action_tag() -> ActionTag {
        ActionTag("setup_channels")
    }
    fn tracing_synopsis(&self) -> String {
        "Setup the default system channel".to_string()
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "setup_channels",)
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        let mut explanation = vec![];

        if let Some(val) = self.create_file.describe_execute().first() {
            explanation.push(val.description.clone())
        }

        explanation.push("Run `nix-channel --update nixpkgs`".to_string());

        vec![ActionDescription::new(self.tracing_synopsis(), explanation)]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        // Place channel configuration
        self.create_file.try_execute()?;

        let nix_pkg = PathBuf::from(NIX_STORE_PATH.trim());
        let nss_ca_cert_pkg = PathBuf::from(NSS_CACERT_STORE_PATH.trim());

        // Update nixpkgs channel
        execute_command(
            Command::new(nix_pkg.join("bin/nix-channel"))
                .arg("--update")
                .arg("nixpkgs")
                .stdin(std::process::Stdio::null())
                .env("HOME", Self::get_root_home().map_err(Self::error)?)
                .env(
                    "NIX_SSL_CERT_FILE",
                    nss_ca_cert_pkg.join("etc/ssl/certs/ca-bundle.crt"),
                ), /* We could rely on setup_default_profile setting this
                   environment variable, but add this just to be explicit. */
        )
        .map_err(Self::error)?;

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            "Remove system channel configuration".to_string(),
            vec![],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        self.create_file.try_revert()?;

        // We could try to rollback
        // /nix/var/nix/profiles/per-user/root/channels, but that will happen
        // anyways when /nix gets cleaned up.

        Ok(())
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SetupChannelsError {
    #[error("No root home found to place channel configuration in")]
    NoRootHome,
}

impl From<SetupChannelsError> for ActionErrorKind {
    fn from(val: SetupChannelsError) -> Self {
        ActionErrorKind::Custom(Box::new(val))
    }
}
