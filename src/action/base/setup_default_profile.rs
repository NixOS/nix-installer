use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::{
    action::{ActionError, ActionErrorKind, ActionTag, StatefulAction},
    profile::WriteToDefaultProfile,
    set_env,
    settings::{NIX_STORE_PATH, NIX_VERSION, NSS_CACERT_STORE_PATH},
};

use tracing::{span, Span};

use crate::action::{Action, ActionDescription};

/**
Setup the default Nix profile with `nss-cacert` and `nix` itself.
 */
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "setup_default_profile")]
pub struct SetupDefaultProfile {
    unpacked_path: PathBuf,
}

impl SetupDefaultProfile {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(unpacked_path: PathBuf) -> Result<StatefulAction<Self>, ActionError> {
        Ok(Self { unpacked_path }.into())
    }
}

#[typetag::serde(name = "setup_default_profile")]
impl Action for SetupDefaultProfile {
    fn action_tag() -> ActionTag {
        ActionTag("setup_default_profile")
    }
    fn tracing_synopsis(&self) -> String {
        "Setup the default Nix profile".to_string()
    }

    fn tracing_span(&self) -> Span {
        span!(
            tracing::Level::DEBUG,
            "setup_default_profile",
            unpacked_path = %self.unpacked_path.display(),
        )
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        let nix_pkg = PathBuf::from(NIX_STORE_PATH.trim());
        let nss_ca_cert_pkg = PathBuf::from(NSS_CACERT_STORE_PATH.trim());

        // Find the unpacked nix directory (nix-VERSION-SYSTEM)
        let nix_version = NIX_VERSION.trim();
        let found_nix_paths: Vec<_> = std::fs::read_dir(&self.unpacked_path)
            .map_err(|e| ActionErrorKind::ReadDir(self.unpacked_path.clone(), e))
            .map_err(Self::error)?
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("nix-{nix_version}"))
            })
            .collect();

        if found_nix_paths.len() != 1 {
            return Err(Self::error(ActionErrorKind::MalformedBinaryTarball));
        }
        let found_nix_path = found_nix_paths.into_iter().next().unwrap().path();

        let reginfo_path = found_nix_path.join(".reginfo");
        let reginfo = std::fs::read(&reginfo_path)
            .map_err(|e| ActionErrorKind::Read(reginfo_path.to_path_buf(), e))
            .map_err(Self::error)?;

        let mut load_db_command = Command::new(nix_pkg.join("bin/nix-store"));
        load_db_command.arg("--load-db");
        load_db_command.stdin(std::process::Stdio::piped());
        load_db_command.stdout(std::process::Stdio::piped());
        load_db_command.stderr(std::process::Stdio::piped());
        load_db_command.env(
            "HOME",
            dirs::home_dir().ok_or_else(|| Self::error(SetupDefaultProfileError::NoRootHome))?,
        );
        tracing::trace!(
            "Executing `{:?}` with stdin from `{}`",
            load_db_command,
            reginfo_path.display()
        );
        let mut handle = load_db_command
            .spawn()
            .map_err(|e| ActionErrorKind::command(&load_db_command, e))
            .map_err(Self::error)?;

        let mut stdin = handle.stdin.take().unwrap();
        stdin
            .write_all(&reginfo)
            .map_err(|e| ActionErrorKind::Write(PathBuf::from("/dev/stdin"), e))
            .map_err(Self::error)?;
        stdin
            .flush()
            .map_err(|e| ActionErrorKind::Write(PathBuf::from("/dev/stdin"), e))
            .map_err(Self::error)?;
        drop(stdin);
        tracing::trace!(
            "Wrote `{}` to stdin of `nix-store --load-db`",
            reginfo_path.display()
        );

        let output = handle
            .wait_with_output()
            .map_err(|e| ActionErrorKind::command(&load_db_command, e))
            .map_err(Self::error)?;
        if !output.status.success() {
            return Err(Self::error(ActionErrorKind::command_output(
                &load_db_command,
                output,
            )));
        };

        let profile = crate::profile::Profile {
            nix_store_path: &nix_pkg,
            nss_ca_cert_path: &nss_ca_cert_pkg,

            profile: std::path::Path::new("/nix/var/nix/profiles/default"),
            pkgs: &[&nix_pkg, &nss_ca_cert_pkg],
        };
        profile
            .install_packages(WriteToDefaultProfile::WriteToDefault)
            .map_err(SetupDefaultProfileError::NixProfile)
            .map_err(Self::error)?;

        set_env(
            "NIX_SSL_CERT_FILE",
            "/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt",
        );

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            "Unset the default Nix profile".to_string(),
            vec![],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        std::env::remove_var("NIX_SSL_CERT_FILE");

        Ok(())
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SetupDefaultProfileError {
    #[error("No root home found to place channel configuration in")]
    NoRootHome,

    #[error(transparent)]
    NixProfile(#[from] crate::profile::Error),
}

impl From<SetupDefaultProfileError> for ActionErrorKind {
    fn from(val: SetupDefaultProfileError) -> Self {
        ActionErrorKind::Custom(Box::new(val))
    }
}
