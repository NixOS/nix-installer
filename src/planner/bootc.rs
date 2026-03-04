use crate::{
    Action, BuiltinPlanner,
    action::{
        StatefulAction,
        base::{CreateDirectory, CreateFile, RemoveDirectory},
        common::{ConfigureNix, ConfigureUpstreamInitService, CreateUsersAndGroups, ProvisionNix},
        linux::{ProvisionSelinux, provision_selinux::SELINUX_POLICY_PP_CONTENT},
    },
    planner::{Planner, PlannerError},
    settings::{CommonSettings, InitSystem, InstallSettingsError},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use super::{
    ShellProfileLocations,
    linux::{check_nix_not_already_installed, check_not_nixos, check_not_wsl1, detect_selinux},
};

/// A planner for bootc container image builds.
///
/// bootc images are built without a running systemd and with a restricted
/// filesystem layout: `/var`, `/home`, and `/root` do not exist during the
/// build. Nix is installed into `/nix` which becomes part of the image layer.
/// On first boot, `systemd-tmpfiles` copies `/nix` to `/var/lib/nix` (mutable
/// storage), and a bind-mount makes it available at `/nix`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::Parser))]
pub struct Bootc {
    /// Where `/nix` will be bind-mounted from at boot time.
    #[cfg_attr(feature = "cli", clap(long, default_value = "/var/lib/nix"))]
    persistence: PathBuf,
    #[cfg_attr(feature = "cli", clap(flatten))]
    pub settings: CommonSettings,
}

/// Generate a sysusers.d config that recreates the nix build group and users
/// on every boot via `systemd-sysusers`.
fn generate_sysusers_config(settings: &CommonSettings) -> String {
    let mut config = String::new();
    config.push_str(&format!(
        "g {} {}\n",
        settings.nix_build_group_name, settings.nix_build_group_id
    ));
    for i in 1..=settings.nix_build_user_count {
        let uid = settings.nix_build_user_id_base + (i - 1);
        config.push_str(&format!(
            "u {prefix}{i} {uid}:{gid} \"Nix build user {i}\" /var/empty /sbin/nologin\n",
            prefix = settings.nix_build_user_prefix,
            i = i,
            uid = uid,
            gid = settings.nix_build_group_id,
        ));
    }
    config
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
        let has_selinux = detect_selinux()?;
        let mut plan = vec![];

        // Install systemd units — they run after boot, not during the build.
        // No nix-directory.service needed: /nix is already in the image layer
        // and will be present at boot as part of the ostree commit.
        let nix_mount_buf = format!(
            "\
                [Unit]\n\
                Description=Mount `{persistence}` on `/nix`\n\
                PropagatesStopTo=nix-daemon.service\n\
                After=systemd-tmpfiles-setup.service\n\
                ConditionPathIsDirectory=/nix\n\
                DefaultDependencies=no\n\
                \n\
                [Mount]\n\
                What={persistence}\n\
                Where=/nix\n\
                Type=none\n\
                DirectoryMode=0755\n\
                Options=bind\n\
                \n\
                [Install]\n\
                RequiredBy=nix-daemon.service\n\
                RequiredBy=nix-daemon.socket\n\
            ",
            persistence = self.persistence.display(),
        );
        plan.push(
            CreateFile::plan(
                "/etc/systemd/system/nix.mount",
                None,
                None,
                0o0644,
                nix_mount_buf,
                false,
            )
            .map_err(PlannerError::Action)?
            .boxed(),
        );

        // After the bind-mount, reload systemd so it picks up the nix units
        // (which are symlinks into /nix) and start the daemon socket.
        let start_nix_daemon_buf = "\
            [Unit]\n\
            Description=Start Nix daemon after /nix is mounted\n\
            After=nix.mount\n\
            Requires=nix.mount\n\
            DefaultDependencies=no\n\
            \n\
            [Service]\n\
            Type=oneshot\n\
            RemainAfterExit=yes\n\
            ExecStart=/usr/bin/systemctl daemon-reload\n\
            ExecStart=/usr/bin/systemctl restart --no-block nix-daemon.socket\n\
            \n\
            [Install]\n\
            WantedBy=sysinit.target\n\
        "
        .to_string();
        plan.push(
            CreateFile::plan(
                "/etc/systemd/system/nix-daemon-start.service",
                None,
                None,
                0o0644,
                start_nix_daemon_buf,
                false,
            )
            .map_err(PlannerError::Action)?
            .boxed(),
        );

        // Create /nix so the standard install steps work.
        plan.push(
            CreateDirectory::plan("/nix", None, None, 0o0755, true)
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        plan.push(
            ProvisionNix::plan(&self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        plan.push(
            CreateUsersAndGroups::plan(self.settings.clone())
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        // We need to remove this path since it's part of the read-only install.
        let mut shell_profile_locations = ShellProfileLocations::default();
        if let Some(index) = shell_profile_locations
            .fish
            .vendor_confd_prefixes
            .iter()
            .position(|v| v == Path::new("/usr/share/fish/"))
        {
            shell_profile_locations
                .fish
                .vendor_confd_prefixes
                .remove(index);
        }

        plan.push(
            ConfigureNix::plan(shell_profile_locations, &self.settings)
                .map_err(PlannerError::Action)?
                .boxed(),
        );

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

        plan.push(
            CreateDirectory::plan("/etc/tmpfiles.d", None, None, 0o0755, false)
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        // Install unit files but do NOT start the daemon — no systemd at build time.
        plan.push(
            ConfigureUpstreamInitService::plan(InitSystem::Systemd, false)
                .map_err(PlannerError::Action)?
                .boxed(),
        );

        // sysusers.d config — ensures build users are recreated on every boot.
        plan.push(
            CreateDirectory::plan("/etc/sysusers.d", None, None, 0o0755, false)
                .map_err(PlannerError::Action)?
                .boxed(),
        );
        plan.push(
            CreateFile::plan(
                "/etc/sysusers.d/nix-installer.conf",
                None,
                None,
                0o0644,
                generate_sysusers_config(&self.settings),
                false,
            )
            .map_err(PlannerError::Action)?
            .boxed(),
        );

        // tmpfiles.d config — copies /nix (from the image) to /var/lib/nix on
        // first boot. The C directive only copies if the destination doesn't
        // exist yet, so subsequent boots skip it.
        plan.push(
            CreateFile::plan(
                "/etc/tmpfiles.d/nix-installer.conf",
                None,
                None,
                0o0644,
                format!("C {} - - - - /nix\n", self.persistence.display()),
                false,
            )
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
            persistence,
            settings,
        } = self;
        let mut map = HashMap::default();

        map.extend(settings.settings()?);
        map.insert(
            "persistence".to_string(),
            serde_json::to_value(persistence)?,
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
            OperatingSystem::Linux => Ok(()),
            host_os => Err(PlannerError::IncompatibleOperatingSystem {
                planner: self.typetag_name(),
                host_os,
            }),
        }
    }

    fn pre_uninstall_check(&self) -> Result<(), PlannerError> {
        Err(PlannerError::Custom(Box::new(BootcUninstallError)))
    }

    fn pre_install_check(&self) -> Result<(), PlannerError> {
        check_not_nixos()?;
        check_nix_not_already_installed()?;
        check_not_wsl1()?;
        // No systemd check — bootc builds run without systemd.
        Ok(())
    }
}

impl From<Bootc> for BuiltinPlanner {
    fn from(val: Bootc) -> Self {
        BuiltinPlanner::Bootc(val)
    }
}

#[derive(Debug)]
struct BootcUninstallError;

impl std::fmt::Display for BootcUninstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Uninstall is not supported on bootc/immutable systems. \
             To remove Nix, rebuild the container image without the \
             `nix-installer install bootc` step."
        )
    }
}

impl std::error::Error for BootcUninstallError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysusers_config_format() {
        let settings = CommonSettings {
            modify_profile: true,
            nix_build_group_name: "nixbld".to_string(),
            nix_build_group_id: 30000,
            nix_build_user_prefix: "nixbld".to_string(),
            nix_build_user_id_base: 30000,
            nix_build_user_count: 3,
            ssl_cert_file: None,
            extra_conf: vec![],
            force: false,
            skip_nix_conf: false,
            add_channel: false,
        };

        let config = generate_sysusers_config(&settings);
        let expected = "\
g nixbld 30000
u nixbld1 30000:30000 \"Nix build user 1\" /var/empty /sbin/nologin
u nixbld2 30001:30000 \"Nix build user 2\" /var/empty /sbin/nologin
u nixbld3 30002:30000 \"Nix build user 3\" /var/empty /sbin/nologin
";
        assert_eq!(config, expected);
    }
}
