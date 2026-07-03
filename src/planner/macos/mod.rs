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
            ConfigureRemoteBuilding, CreateNixHookService, CreateNixVolume, ProvisionNixMountd,
            SetTmutilExclusions,
        },
    },
    execute_command,
    os::darwin::{DiskUtilInfoOutput, diskutil::DiskUtilList},
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

    /// On AWS EC2, put the Nix Store on the instance's internal disk to avoid the
    /// interactive Full Disk Access prompt macOS requires for removable volumes.
    ///
    /// The internal disk is erased on Stop (but survives reboots), so the install
    /// breaks if the instance is Stopped. A launchd watchdog remounts the volume,
    /// which AWS's macOS deployment periodically unmounts. Auto-detects the root
    /// disk when --root-disk is unset.
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            default_value = "false",
            env = "NIX_INSTALLER_USE_EC2_INSTANCE_STORE"
        )
    )]
    pub use_ec2_instance_store: bool,
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

fn default_internal_root_disk() -> Result<Option<String>, PlannerError> {
    let buf = execute_command(
        Command::new("/usr/sbin/diskutil")
            .args(["list", "-plist", "internal", "virtual"])
            .stdin(std::process::Stdio::null()),
    )
    .map_err(|e| PlannerError::Custom(Box::new(e)))?
    .stdout;
    let the_plist: DiskUtilList = plist::from_reader(Cursor::new(buf))?;

    let mut disks = the_plist
        .all_disks_and_partitions
        .into_iter()
        .filter(|disk| !disk.os_internal)
        .collect::<Vec<_>>();

    disks.sort_by_key(|d| d.size_bytes);

    Ok(disks.pop().map(|d| d.device_identifier))
}

#[typetag::serde(name = "macos")]
impl Planner for Macos {
    fn try_default() -> Result<Self, PlannerError> {
        Ok(Self {
            settings: CommonSettings::try_default()?,
            use_ec2_instance_store: false,
            root_disk: Some(default_root_disk()?),
            case_sensitive: false,
            encrypt: None,
            volume_label: "Nix Store".into(),
        })
    }

    fn plan(&self) -> Result<Vec<StatefulAction<Box<dyn Action>>>, PlannerError> {
        if self.use_ec2_instance_store && !crate::distribution::NIX_MOUNTD_AVAILABLE {
            return Err(PlannerError::Ec2InstanceStoreUnavailable);
        }

        let root_disk = match &self.root_disk {
            root_disk @ Some(_) => root_disk.clone(),
            None if self.use_ec2_instance_store => {
                Some(default_internal_root_disk()?.ok_or(PlannerError::NoInternalDisk)?)
            },
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

        // nix-mountd must exist before the volume service that runs it is
        // bootstrapped by CreateNixVolume.
        if self.use_ec2_instance_store {
            plan.push(
                ProvisionNixMountd::plan()
                    .map_err(PlannerError::Action)?
                    .boxed(),
            );
        }

        plan.extend([
            CreateNixVolume::plan(
                root_disk.unwrap(), /* We just ensured it was populated */
                self.volume_label.clone(),
                self.case_sensitive,
                encrypt,
                self.use_ec2_instance_store,
            )
            .map_err(PlannerError::Action)?
            .boxed(),
            ProvisionNix::plan(&self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
            // Auto-allocate uids is broken on Mac. Tools like `whoami` don't work.
            // e.g. https://github.com/NixOS/nix/issues/8444
            CreateUsersAndGroups::plan(self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
            SetTmutilExclusions::plan(vec![
                PathBuf::from(NIX_STORE_LOCATION),
                PathBuf::from("/nix/var"),
            ])
            .map_err(PlannerError::Action)?
            .boxed(),
            ConfigureNix::plan(ShellProfileLocations::default(), &self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
            ConfigureRemoteBuilding::plan()
                .map_err(PlannerError::Action)?
                .boxed(),
        ]);

        if self.settings.modify_profile {
            plan.push(
                CreateNixHookService::plan()
                    .map_err(PlannerError::Action)?
                    .boxed(),
            );
        }

        plan.extend([
            ConfigureUpstreamInitService::plan(InitSystem::Launchd, true)
                .map_err(PlannerError::Action)?
                .boxed(),
            RemoveDirectory::plan(crate::settings::SCRATCH_DIR)
                .map_err(PlannerError::Action)?
                .boxed(),
        ]);

        Ok(plan)
    }

    fn settings(&self) -> Result<HashMap<String, serde_json::Value>, InstallSettingsError> {
        let Self {
            settings,
            encrypt,
            volume_label,
            case_sensitive,
            root_disk,
            use_ec2_instance_store,
        } = self;
        let mut map = HashMap::default();

        map.extend(settings.settings()?);
        map.insert("volume_encrypt".into(), serde_json::to_value(encrypt)?);
        map.insert("volume_label".into(), serde_json::to_value(volume_label)?);
        map.insert("root_disk".into(), serde_json::to_value(root_disk)?);
        map.insert(
            "use_ec2_instance_store".into(),
            serde_json::to_value(use_ec2_instance_store)?,
        );
        map.insert(
            "case_sensitive".into(),
            serde_json::to_value(case_sensitive)?,
        );

        Ok(map)
    }

    fn configured_settings(&self) -> Result<HashMap<String, serde_json::Value>, PlannerError> {
        let default = Self::try_default()?.settings()?;
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
        check_macos_version()?;
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

/// The minimum macOS version required to run the bundled Nix.
///
/// Nix links against the system libc++ (`/usr/lib/libc++.1.dylib`).
/// Since Nix 2.34, libnixexpr uses `std::pmr::memory_resource`, which
/// was introduced in LLVM 16's libc++ and first shipped in macOS 14
/// (Sonoma).  On older systems the dynamic linker aborts with
/// "Symbol not found: __ZNSt3__13pmr15memory_resourceD2Ev".
const MIN_MACOS_VERSION: (u64, u64) = (14, 0);

fn check_macos_version() -> Result<(), PlannerError> {
    use sysctl::{Ctl, Sysctl};

    let ctl = Ctl::new("kern.osproductversion")?;
    let version_string = ctl.value_string()?;

    let parts: Vec<u64> = version_string
        .trim()
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    let (major, minor) = (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
    );

    if (major, minor) < MIN_MACOS_VERSION {
        return Err(MacosError::UnsupportedMacosVersion {
            got: version_string.trim().to_owned(),
            min: format!("{}.{}", MIN_MACOS_VERSION.0, MIN_MACOS_VERSION.1),
        })
        .map_err(|e| PlannerError::Custom(Box::new(e)));
    }

    Ok(())
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

    #[error(
        "macOS {got} is too old — Nix requires macOS {min} or later (the bundled Nix links against \
         system libc++ which only provides std::pmr since macOS 14)"
    )]
    UnsupportedMacosVersion { got: String, min: String },
}

impl HasExpectedErrors for MacosError {
    fn expected<'a>(&'a self) -> Option<Box<dyn std::error::Error + 'a>> {
        match self {
            this @ MacosError::UninstallNixDarwin => Some(Box::new(this)),
            this @ MacosError::BlockedBySystemUIServerPolicy(_) => Some(Box::new(this)),
            this @ MacosError::UnsupportedMacosVersion { .. } => Some(Box::new(this)),
        }
    }
}
