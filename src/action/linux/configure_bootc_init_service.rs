use crate::{
    action::{Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction},
    execute_command,
    settings::nix_store_path,
};
use std::{path::Path, process::Command};
use tracing::{Span, span};

const NIX_PROFILE: &str = "/nix/var/nix/profiles/default";
const DAEMON_FILES: [(&str, &str); 3] = [
    ("systemd/system", "nix-daemon.service"),
    ("systemd/system", "nix-daemon.socket"),
    ("tmpfiles.d", "nix-daemon.conf"),
];

/// Configure the bootc image's Nix daemon without starting services.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action_name", rename = "configure_bootc_init_service")]
pub struct ConfigureBootcInitService {
    selinux: bool,
}

impl ConfigureBootcInitService {
    pub fn plan(selinux: bool) -> StatefulAction<Self> {
        StatefulAction::uncompleted(Self { selinux })
    }

    fn install_daemon_file(directory: &str, name: &str, nix_path: &str) -> Result<(), ActionError> {
        let source = Path::new(NIX_PROFILE)
            .join("lib")
            .join(directory)
            .join(name);
        let destination_dir = Path::new("/usr/lib").join(directory);
        let destination = destination_dir.join(name);
        std::fs::create_dir_all(&destination_dir)
            .map_err(|e| Self::error(ActionErrorKind::CreateDirectory(destination_dir, e)))?;

        // bootc's linter cannot follow symlinks through /nix, so copy regular files.
        let contents = std::fs::read_to_string(&source)
            .map_err(|e| Self::error(ActionErrorKind::Read(source, e)))?;
        // Use the persisted profile so later OS images do not replace the running Nix.
        let contents = contents.replace(nix_path, NIX_PROFILE);
        std::fs::write(&destination, contents)
            .map_err(|e| Self::error(ActionErrorKind::Write(destination, e)))
    }

    fn configure_units(&self, operation: &str) -> Result<(), ActionError> {
        let mut command = Command::new("systemctl");
        command.args(["--root=/", operation, "nix.mount", "nix-daemon.socket"]);
        if self.selinux {
            command.arg("nix-store-label.service");
        }
        execute_command(&mut command)
            .map(|_| ())
            .map_err(Self::error)
    }
}

#[typetag::serde(name = "configure_bootc_init_service")]
impl Action for ConfigureBootcInitService {
    fn action_tag() -> ActionTag {
        ActionTag("configure_bootc_init_service")
    }

    fn tracing_synopsis(&self) -> String {
        "Configure the Nix daemon for bootc first boot".into()
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "configure_bootc_init_service")
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    fn execute(&mut self) -> Result<(), ActionError> {
        let nix_path = nix_store_path().map_err(Self::error)?;
        for (directory, name) in DAEMON_FILES {
            Self::install_daemon_file(directory, name, nix_path)?;
        }
        self.configure_units("enable")
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![]
    }

    fn revert(&mut self) -> Result<(), ActionError> {
        self.configure_units("disable")?;
        for (directory, name) in DAEMON_FILES {
            let path = Path::new("/usr/lib").join(directory).join(name);
            crate::util::remove_file(&path, crate::util::OnMissing::Ignore)
                .map_err(|e| Self::error(ActionErrorKind::Remove(path, e)))?;
        }
        Ok(())
    }
}
