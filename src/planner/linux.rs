use std::{collections::HashMap, path::Path, process::Command};

use super::{LinuxDistro, ShellProfileLocations};
use crate::{
    Action, BuiltinPlanner,
    action::{
        StatefulAction,
        base::{
            CreateDirectory, CreateOrInsertIntoFile, RemoveDirectory, create_or_insert_into_file,
        },
        common::{ConfigureNix, ConfigureUpstreamInitService, CreateUsersAndGroups, ProvisionNix},
        linux::{ProvisionSelinux, provision_selinux::SELINUX_POLICY_PP_CONTENT},
    },
    error::HasExpectedErrors,
    planner::{Planner, PlannerError},
    settings::{CommonSettings, InitSettings, InitSystem, InstallSettingsError},
    util::which,
};

pub const FHS_SELINUX_POLICY_PATH: &str = "/usr/share/selinux/packages/nix.pp";

/// A planner for traditional, mutable Linux systems like Debian, RHEL, or Arch
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::Parser))]
pub struct Linux {
    #[cfg_attr(feature = "cli", clap(flatten))]
    pub settings: CommonSettings,
    #[cfg_attr(feature = "cli", clap(flatten))]
    pub init: InitSettings,
}

#[typetag::serde(name = "linux")]
impl Planner for Linux {
    fn try_default() -> Result<Self, PlannerError> {
        Ok(Self {
            settings: CommonSettings::try_default()?,
            init: InitSettings::try_default()?,
        })
    }

    fn plan(&self) -> Result<Vec<StatefulAction<Box<dyn Action>>>, PlannerError> {
        let has_selinux = detect_selinux()?;

        let mut shell_profile_locations = ShellProfileLocations::default();
        match LinuxDistro::detect() {
            LinuxDistro::Suse => {
                // On SUSE, /etc/bash.bashrc sources /etc/profile for SSH sessions and
                // rebuilds PATH from scratch. Writing the Nix snippet to /etc/bash.bashrc
                // causes PATH to lose Nix directories because the idempotency guard in
                // nix-daemon.sh prevents re-sourcing via /etc/profile.d/nix.sh after PATH
                // is rebuilt.
                shell_profile_locations
                    .bash
                    .retain(|p| p != std::path::Path::new("/etc/bash.bashrc"));
                shell_profile_locations
                    .bash
                    .push("/etc/bash.bashrc.local".into());
            },
            LinuxDistro::Arch => {
                // On Arch Linux, /etc/bash.bashrc starts with `[[ $- != *i* ]] && return`
                // which prevents the Nix snippet from running in non-interactive shells
                // (e.g. `ssh host 'command'`). Writing to /etc/bash.bashrc still works for
                // interactive sessions, but we also need /etc/profile.d/nix.sh (already in
                // defaults) for login shells. For non-interactive non-login shells (SSH
                // command mode), we set BASH_ENV in /etc/environment.d/ so bash sources the
                // Nix profile automatically.
                shell_profile_locations
                    .bash
                    .retain(|p| p != std::path::Path::new("/etc/bash.bashrc"));
            },
            _ => {},
        }

        let mut plan = vec![
            CreateDirectory::plan("/nix", None, None, 0o0755, true)
                .map_err(PlannerError::Action)?
                .boxed(),
            ProvisionNix::plan(&self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
            CreateUsersAndGroups::plan(self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
            ConfigureNix::plan(shell_profile_locations, &self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
        ];

        if LinuxDistro::detect() == LinuxDistro::Arch {
            // On Arch, /etc/bash.bashrc guards non-interactive shells with
            // `[[ $- != *i* ]] && return`, so the Nix snippet we prepend there
            // never runs for `ssh host 'command'` (non-interactive, non-login).
            // Setting BASH_ENV in /etc/environment (read by PAM for all sessions)
            // makes bash source the Nix profile in every session type.
            plan.push(
                CreateOrInsertIntoFile::plan(
                    "/etc/environment",
                    None,
                    None,
                    0o644,
                    "\n# Nix\nBASH_ENV=/nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh\n# End Nix\n".to_string(),
                    create_or_insert_into_file::Position::End,
                )
                .map_err(PlannerError::Action)?
                .boxed(),
            );
        }

        if has_selinux {
            plan.push(
                ProvisionSelinux::plan(FHS_SELINUX_POLICY_PATH.into(), SELINUX_POLICY_PP_CONTENT)
                    .map_err(PlannerError::Action)?
                    .boxed(),
            );
        }

        plan.extend([
            CreateDirectory::plan("/etc/tmpfiles.d", None, None, 0o0755, false)
                .map_err(PlannerError::Action)?
                .boxed(),
            ConfigureUpstreamInitService::plan(self.init.init, self.init.start_daemon)
                .map_err(PlannerError::Action)?
                .boxed(),
            RemoveDirectory::plan(crate::settings::SCRATCH_DIR)
                .map_err(PlannerError::Action)?
                .boxed(),
        ]);

        Ok(plan)
    }

    fn settings(&self) -> Result<HashMap<String, serde_json::Value>, InstallSettingsError> {
        let Self { settings, init } = self;
        let mut map = HashMap::default();

        map.extend(settings.settings()?);
        map.extend(init.settings()?);

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
            OperatingSystem::Linux => Ok(()),
            host_os => Err(PlannerError::IncompatibleOperatingSystem {
                planner: self.typetag_name(),
                host_os,
            }),
        }
    }

    fn pre_uninstall_check(&self) -> Result<(), PlannerError> {
        check_not_wsl1()?;

        if self.init.init == InitSystem::Systemd && self.init.start_daemon {
            check_systemd_active()?;
        }

        Ok(())
    }

    fn pre_install_check(&self) -> Result<(), PlannerError> {
        check_not_nixos()?;

        check_nix_not_already_installed()?;

        check_not_wsl1()?;

        if self.init.init == InitSystem::Systemd && self.init.start_daemon {
            check_systemd_active()?;
        }

        Ok(())
    }
}

impl From<Linux> for BuiltinPlanner {
    fn from(val: Linux) -> Self {
        BuiltinPlanner::Linux(val)
    }
}

// If on NixOS, running `nix_installer` is pointless
pub(crate) fn check_not_nixos() -> Result<(), PlannerError> {
    // NixOS always sets up this file as part of setting up /etc itself: https://github.com/NixOS/nixpkgs/blob/bdd39e5757d858bd6ea58ed65b4a2e52c8ed11ca/nixos/modules/system/etc/setup-etc.pl#L145
    if Path::new("/etc/NIXOS").exists() {
        return Err(PlannerError::NixOs);
    }
    Ok(())
}

pub(crate) fn check_not_wsl1() -> Result<(), PlannerError> {
    // Detection strategies: https://patrickwu.space/wslconf/
    if std::env::var("WSL_DISTRO_NAME").is_ok() && std::env::var("WSL_INTEROP").is_err() {
        return Err(PlannerError::Wsl1);
    }
    Ok(())
}

pub(crate) fn detect_selinux() -> Result<bool, PlannerError> {
    if Path::new("/sys/fs/selinux").exists() && which("sestatus").is_some() {
        // We expect systems with SELinux to have the normal SELinux tools.
        let has_semodule = which("semodule").is_some();
        let has_restorecon = which("restorecon").is_some();
        if !(has_semodule && has_restorecon) {
            Err(PlannerError::SelinuxRequirements)
        } else {
            Ok(true)
        }
    } else {
        Ok(false)
    }
}

pub(crate) fn check_nix_not_already_installed() -> Result<(), PlannerError> {
    // For now, we don't try to repair the user's Nix install or anything special.
    if Command::new("nix-env")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Err(PlannerError::NixExists);
    }

    Ok(())
}

pub(crate) fn check_systemd_active() -> Result<(), PlannerError> {
    if !Path::new("/run/systemd/system").exists() {
        if std::env::var("WSL_DISTRO_NAME").is_ok() {
            return Err(LinuxErrorKind::Wsl2SystemdNotActive.into());
        } else {
            return Err(LinuxErrorKind::SystemdNotActive.into());
        }
    }

    Ok(())
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum LinuxErrorKind {
    #[error(
        "\
        systemd was not active.\n\
        \n\
        If it will be started later consider, passing `--no-start-daemon`.\n\
        \n\
        To use a `root`-only Nix install, consider passing `--init none`."
    )]
    SystemdNotActive,
    #[error(
        "\
        systemd was not active.\n\
        \n\
        On WSL2, systemd is not enabled by default. Consider enabling it by adding it to your `/etc/wsl.conf` with `echo -e '[boot]\\nsystemd=true'` then restarting WSL2 with `wsl.exe --shutdown` and re-entering the WSL shell. For more information, see https://devblogs.microsoft.com/commandline/systemd-support-is-now-available-in-wsl/.\n\
        \n\
        If it will be started later consider, passing `--no-start-daemon`.\n\
        \n\
        To use a `root`-only Nix install, consider passing `--init none`."
    )]
    Wsl2SystemdNotActive,
}

impl HasExpectedErrors for LinuxErrorKind {
    fn expected<'a>(&'a self) -> Option<Box<dyn std::error::Error + 'a>> {
        match self {
            LinuxErrorKind::SystemdNotActive => Some(Box::new(self)),
            LinuxErrorKind::Wsl2SystemdNotActive => Some(Box::new(self)),
        }
    }
}

impl From<LinuxErrorKind> for PlannerError {
    fn from(v: LinuxErrorKind) -> PlannerError {
        PlannerError::Custom(Box::new(v))
    }
}
