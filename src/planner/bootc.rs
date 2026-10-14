use super::linux::{check_nix_not_already_installed, check_not_nixos, check_not_wsl1};
use crate::{
    Action, BuiltinPlanner,
    action::{
        StatefulAction,
        base::{CreateDirectory, CreateFile, RemoveDirectory},
        common::{ConfigureNix, CreateUsersAndGroups, ProvisionNix},
        linux::{
            ConfigureBootcInitService, ProvisionSelinux,
            provision_selinux::SELINUX_POLICY_PP_CONTENT,
        },
    },
    planner::{Planner, PlannerError, ShellProfileLocations},
    settings::{CommonSettings, InstallSettingsError},
};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

/// Install Nix while building a bootc image, without starting services.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::Parser))]
pub struct Bootc {
    /// Persistent directory to bind mount at /nix after boot.
    #[cfg_attr(feature = "cli", clap(long, default_value = "/var/lib/nix"))]
    persistence: PathBuf,
    #[cfg_attr(feature = "cli", clap(flatten))]
    pub settings: CommonSettings,
}

impl Bootc {
    fn plan_boot_files(
        &self,
        selinux: bool,
    ) -> Result<Vec<StatefulAction<Box<dyn Action>>>, PlannerError> {
        let persistence = self
            .persistence
            .to_str()
            .ok_or(BootcError::InvalidPersistence)?;
        let mut files = vec![
            (
                "/usr/lib/systemd/system/nix-store-setup.service",
                include_str!("bootc/nix-store-setup.service").replace("@persistence@", persistence),
            ),
            (
                "/usr/lib/systemd/system/nix.mount",
                include_str!("bootc/nix.mount").replace("@persistence@", persistence),
            ),
            (
                "/usr/lib/sysusers.d/nix-installer.conf",
                generate_sysusers_config(&self.settings),
            ),
            (
                "/usr/lib/tmpfiles.d/nix-installer.conf",
                format!("C {persistence} - - - - /nix\n"),
            ),
        ];
        if selinux {
            files.push((
                "/usr/lib/systemd/system/nix-store-label.service",
                include_str!("bootc/nix-store-label.service").into(),
            ));
        }

        let mut actions = Vec::new();
        for directory in [
            "/usr/lib/systemd/system",
            "/usr/lib/sysusers.d",
            "/usr/lib/tmpfiles.d",
        ] {
            actions.push(
                CreateDirectory::plan(directory, None, None, 0o0755, false)
                    .map_err(PlannerError::Action)?
                    .boxed(),
            );
        }
        for (path, contents) in files {
            actions.push(
                CreateFile::plan(path, None, None, 0o0644, contents, false)
                    .map_err(PlannerError::Action)?
                    .boxed(),
            );
        }
        Ok(actions)
    }
}

fn generate_sysusers_config(settings: &CommonSettings) -> String {
    let mut config = format!(
        "g {} {}\n",
        settings.nix_build_group_name, settings.nix_build_group_id
    );
    for i in 1..=settings.nix_build_user_count {
        config.push_str(&format!(
            "u {prefix}{i} {uid}:{gid} \"Nix build user {i}\" /var/empty /sbin/nologin\n",
            prefix = settings.nix_build_user_prefix,
            uid = settings.nix_build_user_id_base + i,
            gid = settings.nix_build_group_id,
        ));
    }
    config
}

fn validate_persistence(path: &Path) -> Result<(), BootcError> {
    let Some(value) = path.to_str() else {
        return Err(BootcError::InvalidPersistence);
    };
    if !path.starts_with("/var")
        || path == Path::new("/var")
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "\"'\\%".contains(c))
    {
        return Err(BootcError::InvalidPersistence);
    }
    Ok(())
}

#[typetag::serde(name = "bootc")]
impl Planner for Bootc {
    fn try_default() -> Result<Self, PlannerError> {
        Ok(Self {
            persistence: PathBuf::from("/var/lib/nix"),
            settings: CommonSettings::try_default()?,
        })
    }

    fn plan(&self) -> Result<Vec<StatefulAction<Box<dyn Action>>>, PlannerError> {
        validate_persistence(&self.persistence)?;
        let mut shell_profiles = ShellProfileLocations::default();
        shell_profiles
            .fish
            .vendor_confd_prefixes
            .retain(|p| p != Path::new("/usr/share/fish/"));

        let mut plan = vec![
            CreateDirectory::plan("/nix", None, None, 0o0755, true)
                .map_err(PlannerError::Action)?
                .boxed(),
            ProvisionNix::plan(&self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
            CreateUsersAndGroups::plan(self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
            ConfigureNix::plan(shell_profiles, &self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
        ];
        // The container host need not enable SELinux for the image to use it.
        let has_selinux = Path::new("/etc/selinux/config").exists();
        if has_selinux {
            plan.push(
                ProvisionSelinux::plan(
                    "/etc/nix-installer/selinux/packages/nix.pp".into(),
                    SELINUX_POLICY_PP_CONTENT,
                )
                .map_err(PlannerError::Action)?
                .boxed(),
            );
        }
        plan.extend(self.plan_boot_files(has_selinux)?);
        plan.push(ConfigureBootcInitService::plan(has_selinux).boxed());
        plan.push(
            RemoveDirectory::plan(crate::settings::SCRATCH_DIR)
                .map_err(PlannerError::Action)?
                .boxed(),
        );
        Ok(plan)
    }

    fn settings(&self) -> Result<HashMap<String, serde_json::Value>, InstallSettingsError> {
        let mut map = self.settings.settings()?;
        map.insert(
            "persistence".into(),
            serde_json::to_value(&self.persistence)?,
        );
        map.insert("start_daemon".into(), serde_json::Value::Bool(false));
        Ok(map)
    }

    fn configured_settings(&self) -> Result<HashMap<String, serde_json::Value>, PlannerError> {
        let default = Self::try_default()?.settings()?;
        Ok(self
            .settings()?
            .into_iter()
            .filter(|(k, v)| default.get(k) != Some(v))
            .collect())
    }

    fn platform_check(&self) -> Result<(), PlannerError> {
        use target_lexicon::OperatingSystem;
        match OperatingSystem::host() {
            OperatingSystem::Linux => Ok(()),
            host_os => Err(PlannerError::IncompatibleOperatingSystem {
                planner: self.typetag_name(),
                host_os,
            }),
        }
    }

    fn pre_install_check(&self) -> Result<(), PlannerError> {
        check_not_nixos()?;
        check_nix_not_already_installed()?;
        check_not_wsl1()?;
        validate_persistence(&self.persistence)?;
        if Path::new("/run/ostree-booted").exists() {
            return Err(BootcError::DeployedSystem.into());
        }
        Ok(())
    }

    fn pre_uninstall_check(&self) -> Result<(), PlannerError> {
        Err(BootcError::Uninstall.into())
    }

    fn should_run_self_test(&self) -> bool {
        false
    }
}

impl From<Bootc> for BuiltinPlanner {
    fn from(val: Bootc) -> Self {
        Self::Bootc(val)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BootcError {
    #[error(
        "The bootc planner is for container image builds. Use the ostree planner on a deployed system."
    )]
    DeployedSystem,
    #[error(
        "The bootc persistence path must be below /var and contain no parent traversal, whitespace, quotes, backslashes or systemd specifiers."
    )]
    InvalidPersistence,
    #[error(
        "Uninstall is not supported on bootc systems. Rebuild the image without Nix; the persistent store in /var is retained."
    )]
    Uninstall,
}

impl From<BootcError> for PlannerError {
    fn from(error: BootcError) -> Self {
        Self::Custom(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysusers_ids_match_created_users() {
        let mut settings = CommonSettings::try_default().unwrap();
        settings.nix_build_user_id_base = 42000;
        settings.nix_build_group_id = 41000;
        settings.nix_build_group_name = format!("bootc-test-{}", std::process::id());
        settings.nix_build_user_prefix = format!("bootc-builder-{}-", std::process::id());
        settings.nix_build_user_count = 3;
        let users = CreateUsersAndGroups::plan(settings.clone()).unwrap();
        let config = generate_sysusers_config(&settings);
        for (index, line) in config.lines().skip(1).enumerate() {
            let user = serde_json::to_value(&users.action.create_users[index]).unwrap();
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields[1], user["action"]["name"].as_str().unwrap());
            assert_eq!(
                fields[2],
                format!("{}:{}", user["action"]["uid"], settings.nix_build_group_id)
            );
        }
    }

    #[test]
    fn persistence_is_a_literal_path_below_var() {
        for path in ["/var/lib/nix", "/var/lib/custom-nix"] {
            assert!(validate_persistence(Path::new(path)).is_ok());
        }
        for path in [
            "/var",
            "/nix",
            "var/lib/nix",
            "/variable/nix",
            "/var/../etc",
            "/var/lib/nix\nExecStart=bad",
            "/var/lib/%n",
            "/var/lib/nix store",
            "/var/lib/\\nix",
        ] {
            assert!(
                validate_persistence(Path::new(path)).is_err(),
                "accepted {path:?}"
            );
        }
    }

    #[test]
    fn image_build_does_not_expect_a_daemon() {
        let planner = Bootc::try_default().unwrap();
        assert!(!planner.should_run_self_test());
        assert_eq!(planner.settings().unwrap()["start_daemon"], false);
    }
}
