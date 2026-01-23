use std::path::Path;

use std::process::Command;
use tracing::{span, Span};

use crate::action::{ActionError, ActionErrorKind, ActionTag};
use crate::execute_command;
use crate::util::which;

use crate::action::{Action, ActionDescription, StatefulAction};

/**
Run `systemctl daemon-reload` (on both execute and revert)
*/
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct SystemctlDaemonReload;

impl SystemctlDaemonReload {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan() -> Result<StatefulAction<Self>, ActionError> {
        if !Path::new("/run/systemd/system").exists() {
            return Err(Self::error(ActionErrorKind::SystemdMissing));
        }

        if which("systemctl").is_none() {
            return Err(Self::error(ActionErrorKind::SystemdMissing));
        }

        Ok(StatefulAction::uncompleted(SystemctlDaemonReload))
    }
}

#[typetag::serde(name = "systemctl_daemon_reload")]
impl Action for SystemctlDaemonReload {
    fn action_tag() -> ActionTag {
        ActionTag("systemctl_daemon_reload")
    }
    fn tracing_synopsis(&self) -> String {
        "Run `systemctl daemon-reload`".to_string()
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "systemctl_daemon_reload",)
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        execute_command(
            Command::new("systemctl")
                .arg("daemon-reload")
                .stdin(std::process::Stdio::null()),
        )
        .map_err(Self::error)?;

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        execute_command(
            Command::new("systemctl")
                .arg("daemon-reload")
                .stdin(std::process::Stdio::null()),
        )
        .map_err(Self::error)?;

        Ok(())
    }
}
