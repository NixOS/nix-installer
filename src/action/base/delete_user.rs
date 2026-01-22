use nix::unistd::User;
use std::process::Command;
use target_lexicon::OperatingSystem;
use tracing::{span, Span};

use crate::action::base::create_user::delete_user_macos;
use crate::action::{ActionError, ActionErrorKind, ActionTag};
use crate::execute_command;

use crate::action::{Action, ActionDescription, StatefulAction};

/**
Delete an operating system level user
*/
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "delete_user")]
pub struct DeleteUser {
    name: String,
}

impl DeleteUser {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(name: String) -> Result<StatefulAction<Self>, ActionError> {
        let this = Self { name: name.clone() };

        match OperatingSystem::host() {
            OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => (),
            _ => {
                if !(which::which("userdel").is_ok() || which::which("deluser").is_ok()) {
                    return Err(Self::error(ActionErrorKind::MissingUserDeletionCommand));
                }
            },
        }

        // Ensure user exists
        let _ = User::from_name(name.as_str())
            .map_err(|e| ActionErrorKind::GettingUserId(name.clone(), e))
            .map_err(Self::error)?
            .ok_or_else(|| ActionErrorKind::NoUser(name.clone()))
            .map_err(Self::error)?;

        // There is no "StatefulAction::completed" for this action since if the user is to be deleted
        // it is an error if it does not exist.

        Ok(StatefulAction::uncompleted(this))
    }
}

#[typetag::serde(name = "delete_user")]
impl Action for DeleteUser {
    fn action_tag() -> ActionTag {
        ActionTag("delete_user")
    }
    fn tracing_synopsis(&self) -> String {
        format!(
            "Delete user `{}`, which exists due to a previous install, but is no longer required",
            self.name
        )
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "delete_user", user = self.name,)
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            self.tracing_synopsis(),
            vec![format!(
                "Nix with `auto-allocate-uids = true` no longer requires explicitly created users, so this user can be removed"
            )],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        match OperatingSystem::host() {
            OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => {
                delete_user_macos(&self.name).map_err(Self::error)?;
            },
            _ => {
                if which::which("userdel").is_ok() {
                    execute_command(
                        Command::new("userdel")
                            .arg(&self.name)
                            .stdin(std::process::Stdio::null()),
                    )
                    .map_err(Self::error)?;
                } else if which::which("deluser").is_ok() {
                    execute_command(
                        Command::new("deluser")
                            .arg(&self.name)
                            .stdin(std::process::Stdio::null()),
                    )
                    .map_err(Self::error)?;
                } else {
                    return Err(Self::error(ActionErrorKind::MissingUserDeletionCommand));
                }
            },
        };

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        Ok(())
    }
}
