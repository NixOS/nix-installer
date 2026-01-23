use std::{collections::HashMap, io::Cursor, path::PathBuf};

use crate::util::which;
#[cfg(feature = "cli")]
use clap::ArgAction;
use std::process::Command;

use super::ShellProfileLocations;
use crate::action::common::provision_nix::NIX_STORE_LOCATION;
use crate::planner::HasExpectedErrors;

mod profile_queries;
mod profiles;

use crate::{
    Action, BuiltinPlanner,
    action::{
        StatefulAction,
        base::RemoveDirectory,
        common::{ConfigureNix, ConfigureUpstreamInitService, CreateUsersAndGroups, ProvisionNix},
        macos::{
            ConfigureRemoteBuilding, CreateNixHookService, CreateNixVolume, SetTmutilExclusions,
        },
    },
    execute_command,
    os::darwin::DiskUtilInfoOutput,
    planner::{Planner, PlannerError},
    settings::InstallSettingsError,
    settings::{CommonSettings, InitSystem},
};

/// A planner for MacOS (Darwin) systems
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::Parser))]
pub struct Macos {
    #[cfg_attr(feature = "cli", clap(flatten))]
    pub settings: CommonSettings,

    /// Force encryption on the volume
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            action(ArgAction::Set),
            default_value = "false",
            env = "NIX_INSTALLER_ENCRYPT"
        )
    )]
    pub encrypt: Option<bool>,
    /// Use a case sensitive volume
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            action(ArgAction::SetTrue),
            default_value = "false",
            env = "NIX_INSTALLER_CASE_SENSITIVE"
        )
    )]
    pub case_sensitive: bool,
    /// The label for the created APFS volume
    #[cfg_attr(
        feature = "cli",
        clap(long, default_value = "Nix Store", env = "NIX_INSTALLER_VOLUME_LABEL")
    )]
    pub volume_label: String,
    /// The root disk of the target
    #[cfg_attr(feature = "cli", clap(long, env = "NIX_INSTALLER_ROOT_DISK"))]
    pub root_disk: Option<String>,
}

fn default_root_disk() -> Result<String, PlannerError> {
    let buf = execute_command(
        Command::new("/usr/sbin/diskutil")
            .args(["info", "-plist", "/"])
            .stdin(std::process::Stdio::null()),
    )
    .map_err(|e| PlannerError::Custom(Box::new(e)))?
    .stdout;
    let the_plist: DiskUtilInfoOutput = plist::from_reader(Cursor::new(buf))?;

    Ok(the_plist.parent_whole_disk)
}

#[typetag::serde(name = "macos")]
impl Planner for Macos {
    fn default() -> Result<Self, PlannerError> {
        Ok(Self {
            settings: CommonSettings::default()?,
            root_disk: Some(default_root_disk()?),
            case_sensitive: false,
            encrypt: None,
            volume_label: "Nix Store".into(),
        })
    }

    fn plan(&self) -> Result<Vec<StatefulAction<Box<dyn Action>>>, PlannerError> {
        let root_disk = match &self.root_disk {
            root_disk @ Some(_) => root_disk.clone(),
            None => Some(default_root_disk()?),
        };

        let encrypt = match self.encrypt {
            Some(choice) => {
                if let Some(diskutil_info) =
                    crate::action::macos::get_disk_info_for_label(&self.volume_label)
                        .ok()
                        .flatten()
                {
                    if diskutil_info.file_vault {
                        tracing::warn!(
                            "Existing volume was encrypted with FileVault, forcing `encrypt` to true"
                        );
                        true
                    } else {
                        choice
                    }
                } else {
                    choice
                }
            },
            None => {
                let root_disk_is_encrypted = {
                    let output = Command::new("/usr/bin/fdesetup")
                        .arg("isactive")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .map_err(|e| PlannerError::Custom(Box::new(e)))?;

                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stdout_trimmed = stdout.trim();

                    stdout_trimmed == "true"
                };

                let existing_store_volume_is_encrypted = {
                    if let Some(diskutil_info) =
                        crate::action::macos::get_disk_info_for_label(&self.volume_label)
                            .ok()
                            .flatten()
                    {
                        diskutil_info.file_vault
                    } else {
                        false
                    }
                };

                root_disk_is_encrypted || existing_store_volume_is_encrypted
            },
        };

        let mut plan = vec![];

        plan.push(
            CreateNixVolume::plan(
                root_disk.unwrap(), /* We just ensured it was populated */
                self.volume_label.clone(),
                self.case_sensitive,
                encrypt,
            )
            .map_err(PlannerError::Action)?
            .boxed(),
        );

        plan.push(
            ProvisionNix::plan(&self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
        );
        // Auto-allocate uids is broken on Mac. Tools like `whoami` don't work.
        // e.g. https://github.com/NixOS/nix/issues/8444
        plan.push(
            CreateUsersAndGroups::plan(self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
        );
        plan.push(
            SetTmutilExclusions::plan(vec![
                PathBuf::from(NIX_STORE_LOCATION),
                PathBuf::from("/nix/var"),
            ])
            .map_err(PlannerError::Action)?
            .boxed(),
        );
        plan.push(
            ConfigureNix::plan(ShellProfileLocations::default(), &self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
        );
        plan.push(
            ConfigureRemoteBuilding::plan()
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        if self.settings.modify_profile {
            plan.push(
                CreateNixHookService::plan()
                    .map_err(PlannerError::Action)?
                    .boxed(),
            );
        }

        plan.push(
            ConfigureUpstreamInitService::plan(InitSystem::Launchd, true)
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        plan.push(
            RemoveDirectory::plan(crate::settings::SCRATCH_DIR)
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        Ok(plan)
    }

    fn settings(&self) -> Result<HashMap<String, serde_json::Value>, InstallSettingsError> {
        let Self {
            settings,
            encrypt,
            volume_label,
            case_sensitive,
            root_disk,
        } = self;
        let mut map = HashMap::default();

        map.extend(settings.settings()?);
        map.insert("volume_encrypt".into(), serde_json::to_value(encrypt)?);
        map.insert("volume_label".into(), serde_json::to_value(volume_label)?);
        map.insert("root_disk".into(), serde_json::to_value(root_disk)?);
        map.insert(
            "case_sensitive".into(),
            serde_json::to_value(case_sensitive)?,
        );

        Ok(map)
    }

    fn configured_settings(&self) -> Result<HashMap<String, serde_json::Value>, PlannerError> {
        let default = Self::default()?.settings()?;
        let configured = self.settings()?;

        let mut settings: HashMap<String, serde_json::Value> = HashMap::new();
        for (key, value) in configured.iter() {
            if default.get(key) != Some(value) {
                settings.insert(key.clone(), value.clone());
            }
        }

        Ok(settings)
    }

    fn platform_check(&self) -> Result<(), PlannerError> {
        use target_lexicon::OperatingSystem;
        match target_lexicon::OperatingSystem::host() {
            OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => Ok(()),
            host_os => Err(PlannerError::IncompatibleOperatingSystem {
                planner: self.typetag_name(),
                host_os,
            }),
        }
    }

    fn pre_uninstall_check(&self) -> Result<(), PlannerError> {
        check_nix_darwin_not_installed()?;

        Ok(())
    }

    fn pre_install_check(&self) -> Result<(), PlannerError> {
        check_suis()?;
        check_not_running_in_rosetta()?;

        Ok(())
    }
}

impl From<Macos> for BuiltinPlanner {
    fn from(val: Macos) -> Self {
        BuiltinPlanner::Macos(val)
    }
}

fn check_nix_darwin_not_installed() -> Result<(), PlannerError> {
    let has_darwin_rebuild = which("darwin-rebuild").is_some();
    let has_darwin_option = which("darwin-option").is_some();

    let activate_system_present = Command::new("launchctl")
        .arg("print")
        .arg("system/org.nixos.activate-system")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|v| v.success())
        .unwrap_or(false);

    if activate_system_present || has_darwin_rebuild || has_darwin_option {
        return Err(MacosError::UninstallNixDarwin).map_err(|e| PlannerError::Custom(Box::new(e)));
    };

    Ok(())
}

fn check_not_running_in_rosetta() -> Result<(), PlannerError> {
    use sysctl::{Ctl, Sysctl};
    const CTLNAME: &str = "sysctl.proc_translated";

    match Ctl::new(CTLNAME) {
        // This Mac doesn't have Rosetta!
        Err(sysctl::SysctlError::NotFound(_)) => (),
        Err(e) => Err(e)?,
        Ok(ctl) => {
            let str_val = ctl.value_string()?;

            if str_val == "1" {
                return Err(PlannerError::RosettaDetected);
            }
        },
    }

    Ok(())
}

fn check_suis() -> Result<(), PlannerError> {
    let policies: profiles::Policies = match profiles::load() {
        Ok(pol) => pol,
        Err(e) => {
            tracing::warn!(
                "Skipping SystemUIServer checks: failed to load profile data: {:?}",
                e
            );
            return Ok(());
        },
    };

    let blocks: Vec<_> = profile_queries::blocks_internal_mounting(&policies)
        .into_iter()
        .map(|blocking_policy| blocking_policy.display())
        .collect();

    let error: String = match &blocks[..] {
        [] => {
            return Ok(());
        },
        [block] => format!(
            "The following macOS configuration profile includes a 'Restrictions - Media' policy, which interferes with the Nix Store volume:\n\n{}\n\nSee https://dtr.mn/suis-premount-dissented",
            block
        ),
        blocks => {
            format!(
                "The following macOS configuration profiles include a 'Restrictions - Media' policy, which interferes with the Nix Store volume:\n\n{}\n\nSee https://dtr.mn/suis-premount-dissented",
                blocks.join("\n\n")
            )
        },
    };

    Err(MacosError::BlockedBySystemUIServerPolicy(error))
        .map_err(|e| PlannerError::Custom(Box::new(e)))
}

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MacosError {
    #[error(
        "`nix-darwin` installation detected, it must be removed before uninstalling Nix. Please refer to https://github.com/LnL7/nix-darwin#uninstalling for instructions how to uninstall `nix-darwin`."
    )]
    UninstallNixDarwin,

    #[error("{0}")]
    BlockedBySystemUIServerPolicy(String),
}

impl HasExpectedErrors for MacosError {
    fn expected<'a>(&'a self) -> Option<Box<dyn std::error::Error + 'a>> {
        match self {
            this @ MacosError::UninstallNixDarwin => Some(Box::new(this)),
            this @ MacosError::BlockedBySystemUIServerPolicy(_) => Some(Box::new(this)),
        }
    }
}
