use std::os::unix::process::ExitStatusExt;

use nix::unistd::User;
use std::process::Command;
use target_lexicon::OperatingSystem;
use tracing::{span, Span};

use crate::action::{ActionError, ActionErrorKind, ActionTag};
use crate::execute_command;
use crate::util::which;

use crate::action::{Action, ActionDescription, StatefulAction};

static WARNED_USER_HIDDEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/**
Create an operating system level user in the given group
*/
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "create_user")]
pub struct CreateUser {
    pub(crate) name: String,
    pub(crate) uid: u32,
    pub(crate) groupname: String,
    pub(crate) gid: u32,
    comment: String,
}

impl CreateUser {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(
        name: String,
        uid: u32,
        groupname: String,
        gid: u32,
        comment: String,
        check_completed: bool,
    ) -> Result<StatefulAction<Self>, ActionError> {
        let this = Self {
            name: name.clone(),
            uid,
            groupname,
            gid,
            comment,
        };

        match OperatingSystem::host() {
            OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => (),
            _ => {
                if !(which("useradd").is_some() || which("adduser").is_some()) {
                    return Err(Self::error(ActionErrorKind::MissingUserCreationCommand));
                }
                if !(which("userdel").is_some() || which("deluser").is_some()) {
                    return Err(Self::error(ActionErrorKind::MissingUserDeletionCommand));
                }
            },
        }

        if check_completed {
            // Ensure user does not exist
            if let Some(user) = User::from_name(name.as_str())
                .map_err(|e| ActionErrorKind::GettingUserId(name.clone(), e))
                .map_err(Self::error)?
            {
                if user.uid.as_raw() != uid {
                    return Err(Self::error(ActionErrorKind::UserUidMismatch(
                        name.clone(),
                        user.uid.as_raw(),
                        uid,
                    )));
                }

                if user.gid.as_raw() != gid {
                    return Err(Self::error(ActionErrorKind::UserGidMismatch(
                        name.clone(),
                        user.gid.as_raw(),
                        gid,
                    )));
                }

                tracing::debug!("Creating user `{}` already complete", this.name);
                return Ok(StatefulAction::completed(this));
            }
        }

        Ok(StatefulAction::uncompleted(this))
    }
}

#[typetag::serde(name = "create_user")]
impl Action for CreateUser {
    fn action_tag() -> ActionTag {
        ActionTag("create_user")
    }
    fn tracing_synopsis(&self) -> String {
        format!(
            "Create user `{}` (UID {}) in group `{}` (GID {})",
            self.name, self.uid, self.groupname, self.gid
        )
    }

    fn tracing_span(&self) -> Span {
        span!(
            tracing::Level::DEBUG,
            "create_user",
            user = self.name,
            uid = self.uid,
            groupname = self.groupname,
            gid = self.gid,
        )
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            self.tracing_synopsis(),
            vec![format!(
                "The Nix daemon requires system users it can act as in order to build"
            )],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        let Self {
            name,
            uid,
            groupname,
            gid,
            comment,
        } = self;

        match OperatingSystem::host() {
            OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => {
                create_user_macos(name, *uid, *gid).map_err(Self::error)?;
            },
            _ => {
                if which("useradd").is_some() {
                    execute_command(
                        Command::new("useradd")
                            .args([
                                "--home-dir",
                                "/var/empty",
                                "--comment",
                                comment,
                                "--gid",
                                &gid.to_string(),
                                "--groups",
                                &gid.to_string(),
                                "--no-user-group",
                                "--system",
                                "--shell",
                                "/sbin/nologin",
                                "--uid",
                                &uid.to_string(),
                                "--password",
                                "!",
                                name,
                            ])
                            .stdin(std::process::Stdio::null()),
                    )
                    .map_err(Self::error)?;
                } else if which("adduser").is_some() {
                    execute_command(
                        Command::new("adduser")
                            .args([
                                "--home",
                                "/var/empty",
                                "-H", // Don't create a home.
                                "--gecos",
                                comment,
                                "--ingroup",
                                groupname,
                                "--system",
                                "--shell",
                                "/sbin/nologin",
                                "--uid",
                                &uid.to_string(),
                                "--disabled-password",
                                name,
                            ])
                            .stdin(std::process::Stdio::null()),
                    )
                    .map_err(Self::error)?;
                } else {
                    return Err(Self::error(ActionErrorKind::MissingUserCreationCommand));
                }
            },
        }

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            format!(
                "Delete user `{}` (UID {}) in group {} (GID {})",
                self.name, self.uid, self.groupname, self.gid
            ),
            vec![format!(
                "The Nix daemon requires system users it can act as in order to build"
            )],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        match OperatingSystem::host() {
            OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => {
                delete_user_macos(&self.name).map_err(Self::error)?;
            },
            _ => {
                if which("userdel").is_some() {
                    execute_command(
                        Command::new("userdel")
                            .arg(&self.name)
                            .stdin(std::process::Stdio::null()),
                    )
                    .map_err(Self::error)?;
                } else if which("deluser").is_some() {
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
}

#[tracing::instrument]
fn execute_dscl_retry_on_specific_errors(dscl_args: &[&str]) -> Result<(), ActionErrorKind> {
    let mut retry_tokens: usize = 10;
    loop {
        let mut command = Command::new("/usr/bin/dscl");
        command.args(dscl_args);
        command.stdin(std::process::Stdio::null());
        tracing::debug!(%retry_tokens, command = ?command, "Waiting for user create/update to succeed");

        let output = command
            .output()
            .map_err(|e| ActionErrorKind::command(&command, e))?;
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            tracing::trace!(
                stderr = %stderr,
                stdout = %String::from_utf8_lossy(&output.stdout),
                "Command success"
            );
            break;
        } else if retry_tokens == 0 {
            return Err(ActionErrorKind::command_output(&command, output));
        } else {
            if output.status.code() == Some(140) && stderr.contains("-14988 (eNotYetImplemented)") {
                // Retry due to buggy macOS user behavior?
                // https://github.com/DeterminateSystems/nix-installer/issues/1300
                // https://github.com/ansible/ansible/issues/73505
            } else if output.status.signal() == Some(9) {
                // If the command was SIGKILLed, let's retry and hope it doesn't happen again.
            } else {
                // If the command failed for a reason that we weren't "expecting", return that as an
                // error.
                return Err(ActionErrorKind::command_output(&command, output));
            }

            retry_tokens = retry_tokens.saturating_sub(1);
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    Ok(())
}

#[tracing::instrument(level = "debug", skip_all)]
fn create_user_macos(name: &str, uid: u32, gid: u32) -> Result<(), ActionErrorKind> {
    execute_dscl_retry_on_specific_errors(&[".", "-create", &format!("/Users/{name}")])?;

    execute_dscl_retry_on_specific_errors(&[
        ".",
        "-create",
        &format!("/Users/{name}"),
        "UniqueID",
        &format!("{uid}"),
    ])?;
    execute_dscl_retry_on_specific_errors(&[
        ".",
        "-create",
        &format!("/Users/{name}"),
        "PrimaryGroupID",
        &format!("{gid}"),
    ])?;
    execute_dscl_retry_on_specific_errors(&[
        ".",
        "-create",
        &format!("/Users/{name}"),
        "NFSHomeDirectory",
        "/var/empty",
    ])?;
    execute_dscl_retry_on_specific_errors(&[
        ".",
        "-create",
        &format!("/Users/{name}"),
        "UserShell",
        "/sbin/nologin",
    ])?;
    execute_dscl_retry_on_specific_errors(&[
        ".",
        "-create",
        &format!("/Users/{name}"),
        "RealName",
        name,
    ])?;
    execute_dscl_retry_on_specific_errors(&[
        ".",
        "-create",
        &format!("/Users/{name}"),
        "IsHidden",
        "1",
    ])

    .or_else(|e| {
        if let ActionErrorKind::CommandOutput { ref output, .. } = e {
            if output.status.signal() == Some(9) {
                if !WARNED_USER_HIDDEN.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    tracing::warn!("Failed to automatically mark nixbld users as hidden. See: https://dtr.mn/mark-user-hidden");
                }
                return Ok(())
            }
        }

        Err(e)
    })?;

    Ok(())
}

#[tracing::instrument(level = "debug", skip_all)]
pub fn delete_user_macos(name: &str) -> Result<(), ActionErrorKind> {
    // MacOS is a "Special" case
    // It's only possible to delete users under certain conditions.
    // Documentation on https://it.megocollector.com/macos/cant-delete-a-macos-user-with-dscl-resolution/ and http://www.aixperts.co.uk/?p=214 suggested it was a secure token
    // That is correct, however it's a bit more nuanced. It appears to be that a user must be graphically logged in for some other user on the system to be deleted.
    let mut command = Command::new("/usr/bin/dscl");
    command.args([".", "-delete", &format!("/Users/{}", name)]);
    command.stdin(std::process::Stdio::null());

    let output = command
        .output()
        .map_err(|e| ActionErrorKind::command(&command, e))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        Some(0) => (),
        Some(40) if stderr.contains("-14120") => {
            // The user is on an ephemeral Mac, like detsys uses
            // These Macs cannot always delete users, as sometimes there is no graphical login
            tracing::warn!("Encountered an exit code 40 with -14120 error while removing user, this is likely because the initial executing user did not have a secure token, or that there was no graphical login session. To delete the user, log in graphically, then run `/usr/bin/dscl . -delete /Users/{}`", name);
        },
        Some(185) if stderr.contains("-14009 (eDSUnknownNodeName)") => {
            // The user has already been deleted
            tracing::debug!("User already deleted: /Users/{}", name);
        },
        _ => {
            // Something went wrong
            return Err(ActionErrorKind::command_output(&command, output));
        },
    }

    Ok(())
}
