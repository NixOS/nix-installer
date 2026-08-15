use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    NixInstallerError,
    action::{Action, ActionDescription, StatefulAction},
    planner::{BuiltinPlanner, Planner},
};
use owo_colors::OwoColorize;
use semver::{Version, VersionReq};

pub const RECEIPT_LOCATION: &str = "/nix/receipt.json";

/// A cancellation flag that can be shared across threads
pub type CancelSignal = Arc<AtomicBool>;

/// Create a new cancel signal
pub fn cancel_signal() -> CancelSignal {
    Arc::new(AtomicBool::new(false))
}

/**
A set of [`Action`]s, along with some metadata, which can be carried out to drive an install or
revert
*/
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct InstallPlan {
    pub(crate) version: Version,

    pub(crate) actions: Vec<StatefulAction<Box<dyn Action>>>,

    pub(crate) planner: Box<dyn Planner>,
}

impl InstallPlan {
    pub fn try_default() -> Result<Self, NixInstallerError> {
        let planner = BuiltinPlanner::try_default()?;

        let planner = planner.boxed();
        let actions = planner.plan()?;

        Ok(Self {
            planner,
            actions,
            version: current_version()?,
        })
    }

    pub fn plan<P>(planner: P) -> Result<Self, NixInstallerError>
    where
        P: Planner + 'static,
    {
        planner.platform_check()?;

        // Some Action `plan` calls may fail if we don't do these checks
        planner.pre_install_check()?;

        let actions = planner.plan()?;
        Ok(Self {
            planner: planner.boxed(),
            actions,
            version: current_version()?,
        })
    }

    pub fn pre_uninstall_check(&self) -> Result<(), NixInstallerError> {
        self.planner.platform_check()?;
        self.planner.pre_uninstall_check()?;
        Ok(())
    }

    pub fn pre_install_check(&self) -> Result<(), NixInstallerError> {
        self.planner.platform_check()?;
        self.planner.pre_install_check()?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn describe_install(&self, explain: bool) -> Result<String, NixInstallerError> {
        let Self {
            planner,
            actions,
            version,
            ..
        } = self;

        let plan_settings = if explain {
            // List all settings when explaining
            planner.settings()?
        } else {
            // Otherwise, only list user-configured settings
            planner.configured_settings()?
        };
        let mut plan_settings = plan_settings
            .into_iter()
            .map(|(k, v)| format!("* {k}: {v}", k = k.bold()))
            .collect::<Vec<_>>();
        // Stabilize output order
        plan_settings.sort();

        let buf = format!(
            "\
            Nix install plan (v{version})\n\
            Planner: {planner}{maybe_default_setting_note}\n\
            \n\
            {maybe_plan_settings}\
            Planned actions:\n\
            {actions}\n\
        ",
            planner = planner.typetag_name(),
            maybe_default_setting_note = if plan_settings.is_empty() {
                String::from(" (with default settings)")
            } else {
                String::new()
            },
            maybe_plan_settings = if plan_settings.is_empty() {
                String::new()
            } else {
                format!(
                    "\
                    Configured settings:\n\
                    {plan_settings}\n\
                    \n\
                ",
                    plan_settings = plan_settings.join("\n")
                )
            },
            actions = actions
                .iter()
                .flat_map(|v| v.describe_execute())
                .map(|desc| {
                    let ActionDescription {
                        description,
                        explanation,
                    } = desc;

                    let mut buf = String::default();
                    buf.push_str(&format!("* {description}"));
                    if explain {
                        for line in explanation {
                            buf.push_str(&format!("\n  {line}"));
                        }
                    }
                    buf
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
        Ok(buf)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn install(
        &mut self,
        cancel_signal: Option<CancelSignal>,
    ) -> Result<(), NixInstallerError> {
        self.check_compatible()?;
        self.pre_install_check()?;

        let Self { actions, .. } = self;

        // This is **deliberately sequential**.
        // Actions which are parallelizable are represented by "group actions" like CreateUsers
        // The plan itself represents the concept of the sequence of stages.
        for action in actions {
            if let Some(ref signal) = cancel_signal
                && signal.load(Ordering::Relaxed)
            {
                if let Err(err) = self.write_receipt() {
                    tracing::error!("Error saving receipt: {:?}", err);
                }

                return Err(NixInstallerError::Cancelled);
            }

            tracing::info!("Step: {}", action.tracing_synopsis());
            if let Err(err) = action.try_execute() {
                if let Err(err) = self.write_receipt() {
                    tracing::error!("Error saving receipt: {:?}", err);
                }

                let err = NixInstallerError::Action(err);

                return Err(err);
            }
        }

        self.write_receipt()?;

        if self.daemon_expected() {
            Self::wait_for_daemon_socket();
        }
        if self.shell_profile_modified() {
            if let Err(err) = crate::self_test::self_test().map_err(NixInstallerError::SelfTest) {
                tracing::warn!("{err:?}")
            }
        } else {
            tracing::debug!(
                "skipping self-test: shell profile not modified, `nix` won't be on PATH"
            );
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn describe_uninstall(&self, explain: bool) -> Result<String, NixInstallerError> {
        let Self {
            version,
            planner,
            actions,
            ..
        } = self;

        let plan_settings = if explain {
            // List all settings when explaining
            planner.settings()?
        } else {
            // Otherwise, only list user-configured settings
            planner.configured_settings()?
        };
        let mut plan_settings = plan_settings
            .into_iter()
            .map(|(k, v)| format!("* {k}: {v}", k = k.bold()))
            .collect::<Vec<_>>();
        // Stabilize output order
        plan_settings.sort();

        let buf = format!(
            "\
            Nix uninstall plan (v{version})\n\
            \n\
            Planner: {planner}{maybe_default_setting_note}\n\
            \n\
            {maybe_plan_settings}\
            Planned actions:\n\
            {actions}\n\
        ",
            planner = planner.typetag_name(),
            maybe_default_setting_note = if plan_settings.is_empty() {
                String::from(" (with default settings)")
            } else {
                String::new()
            },
            maybe_plan_settings = if plan_settings.is_empty() {
                String::new()
            } else {
                format!(
                    "\
                Configured settings:\n\
                {plan_settings}\n\
                \n\
            ",
                    plan_settings = plan_settings.join("\n")
                )
            },
            actions = actions
                .iter()
                .rev()
                .flat_map(|v| v.describe_revert())
                .map(|desc| {
                    let ActionDescription {
                        description,
                        explanation,
                    } = desc;

                    let mut buf = String::default();
                    buf.push_str(&format!("* {description}"));
                    if explain {
                        for line in explanation {
                            buf.push_str(&format!("\n  {line}"));
                        }
                    }
                    buf
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
        Ok(buf)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn uninstall(
        &mut self,
        cancel_signal: Option<CancelSignal>,
    ) -> Result<(), NixInstallerError> {
        self.check_compatible()?;
        self.pre_uninstall_check()?;

        let Self { actions, .. } = self;
        let mut errors = vec![];

        // This is **deliberately sequential**.
        // Actions which are parallelizable are represented by "group actions" like CreateUsers
        // The plan itself represents the concept of the sequence of stages.
        for action in actions.iter_mut().rev() {
            if let Some(ref signal) = cancel_signal
                && signal.load(Ordering::Relaxed)
            {
                if let Err(err) = self.write_receipt() {
                    tracing::error!("Error saving receipt: {:?}", err);
                }

                return Err(NixInstallerError::Cancelled);
            }

            tracing::info!("Revert: {}", action.tracing_synopsis());
            if let Err(errs) = action.try_revert() {
                errors.push(errs);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            let err = NixInstallerError::ActionRevert(errors);
            Err(err)
        }
    }

    pub fn check_compatible(&self) -> Result<(), NixInstallerError> {
        let self_version_string = self.version.to_string();
        let req = VersionReq::parse(&self_version_string)
            .map_err(|e| NixInstallerError::InvalidVersionRequirement(self_version_string, e))?;
        let nix_installer_version = current_version()?;
        if req.matches(&nix_installer_version) {
            Ok(())
        } else {
            Err(NixInstallerError::IncompatibleVersion {
                binary: nix_installer_version,
                plan: self.version.clone(),
            })
        }
    }

    pub(crate) fn write_receipt(&self) -> Result<(), NixInstallerError> {
        let install_receipt_path = PathBuf::from(RECEIPT_LOCATION);
        write_receipt(self, &install_receipt_path)?;

        Ok(())
    }

    /// Whether the install plan is expected to have started a Nix daemon.
    ///
    /// This checks the planner settings for `start_daemon` (Linux) and
    /// `init` (all platforms).  macOS always starts the daemon so we default
    /// to `true` when the setting is absent.
    /// The self-test execs `sh -lc "nix build ..."`, which only finds `nix`
    /// if we wrote the shell profile hooks. `--no-modify-profile` (and
    /// therefore `--rootless`) leave PATH alone, so the test would just
    /// report "nix: not found" and alarm the user about a perfectly
    /// functional install.
    fn shell_profile_modified(&self) -> bool {
        match self.planner.settings() {
            Ok(s) => s
                .get("modify_profile")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            Err(_) => true,
        }
    }

    fn daemon_expected(&self) -> bool {
        let settings = match self.planner.settings() {
            Ok(s) => s,
            Err(_) => return true,
        };

        // --init none means no daemon at all
        if let Some(init) = settings.get("init")
            && init.as_str() == Some("none")
        {
            return false;
        }

        // --no-start-daemon means the daemon is configured but not started
        if let Some(start_daemon) = settings.get("start_daemon")
            && start_daemon.as_bool() == Some(false)
        {
            return false;
        }

        true
    }

    const DAEMON_SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";

    /// Poll for the Nix daemon socket to appear on disk.
    ///
    /// After the init service starts the daemon, there is a brief window
    /// before the daemon creates its Unix socket.  We poll here so the
    /// self-test doesn't race against daemon startup.
    fn wait_for_daemon_socket() {
        let path = Path::new(Self::DAEMON_SOCKET_PATH);
        if path.exists() {
            return;
        }

        const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
        const INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

        tracing::info!("Waiting up to {}s for Nix daemon socket", TIMEOUT.as_secs());

        let deadline = std::time::Instant::now() + TIMEOUT;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(INTERVAL);
            if path.exists() {
                tracing::debug!("Nix daemon socket appeared");
                return;
            }
        }

        tracing::warn!(
            "Nix daemon socket did not appear at {} within {}s",
            Self::DAEMON_SOCKET_PATH,
            TIMEOUT.as_secs()
        );
    }
}

pub(crate) fn write_receipt(
    plan: &impl serde::Serialize,
    install_receipt_path: &Path,
) -> Result<(), NixInstallerError> {
    let install_receipt_path_tmp = {
        let mut install_receipt_path_tmp = install_receipt_path.to_path_buf();
        install_receipt_path_tmp.set_extension("tmp");
        install_receipt_path_tmp
    };
    let self_json =
        serde_json::to_string_pretty(plan).map_err(NixInstallerError::SerializingReceipt)?;

    std::fs::create_dir_all("/nix")
        .map_err(|e| NixInstallerError::RecordingReceipt(PathBuf::from("/nix"), e))?;
    std::fs::write(&install_receipt_path_tmp, format!("{self_json}\n"))
        .map_err(|e| NixInstallerError::RecordingReceipt(install_receipt_path_tmp.clone(), e))?;
    std::fs::rename(&install_receipt_path_tmp, install_receipt_path)
        .map_err(|e| NixInstallerError::RecordingReceipt(install_receipt_path.to_path_buf(), e))?;

    Ok(())
}

pub fn current_version() -> Result<Version, NixInstallerError> {
    let nix_installer_version_str = env!("CARGO_PKG_VERSION");
    Version::from_str(nix_installer_version_str).map_err(|e| {
        NixInstallerError::InvalidCurrentVersion(nix_installer_version_str.to_string(), e)
    })
}

#[cfg(test)]
mod test {
    use semver::Version;

    use crate::{InstallPlan, NixInstallerError, planner::BuiltinPlanner};

    #[test]
    fn ensure_version_allows_compatible() -> Result<(), NixInstallerError> {
        let planner = BuiltinPlanner::try_default()?;
        let good_version = Version::parse(env!("CARGO_PKG_VERSION"))?;
        let value = serde_json::json!({
            "planner": planner.boxed(),
            "version": good_version,
            "actions": [],
        });
        let maybe_plan: InstallPlan = serde_json::from_value(value)?;
        maybe_plan.check_compatible()?;
        Ok(())
    }

    #[test]
    fn ensure_version_denies_incompatible() -> Result<(), NixInstallerError> {
        let planner = BuiltinPlanner::try_default()?;
        let bad_version = Version::parse("9999999999999.9999999999.99999999")?;
        let value = serde_json::json!({
            "planner": planner.boxed(),
            "version": bad_version,
            "actions": [],
        });
        let maybe_plan: InstallPlan = serde_json::from_value(value)?;
        assert!(maybe_plan.check_compatible().is_err());
        Ok(())
    }
}
