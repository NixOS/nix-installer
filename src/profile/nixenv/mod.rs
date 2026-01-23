use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub(crate) struct NixEnv<'a> {
    pub nix_store_path: &'a Path,
    pub nss_ca_cert_path: &'a Path,

    pub profile: &'a Path,
    pub pkgs: &'a [&'a Path],
}

impl NixEnv<'_> {
    pub(crate) fn install_packages(
        &self,
        to_default: super::WriteToDefaultProfile,
    ) -> Result<(), super::Error> {
        self.validate_paths_can_cohabitate()?;

        let tmp = tempfile::tempdir().map_err(super::Error::CreateTempDir)?;
        let temporary_profile = tmp.path().join("profile");

        self.make_empty_profile(&temporary_profile)?;

        if let Ok(canon_profile) = self.profile.canonicalize() {
            self.set_profile_to(Some(&temporary_profile), &canon_profile)?;
        }

        let paths_by_pkg_output = self.collect_paths_by_package_output(&temporary_profile)?;

        for pkg in self.pkgs {
            let pkg_outputs =
                collect_children(pkg).map_err(super::Error::EnumeratingStorePathContent)?;

            for (root_path, children) in &paths_by_pkg_output {
                let conflicts = children
                    .intersection(&pkg_outputs)
                    .collect::<Vec<&PathBuf>>();

                if !conflicts.is_empty() {
                    tracing::debug!(
                        ?temporary_profile,
                        ?root_path,
                        ?conflicts,
                        "Uninstalling path from the scratch profile due to conflicts"
                    );

                    self.uninstall_path(&temporary_profile, root_path)?;
                }
            }

            self.install_path(&temporary_profile, pkg)?;
        }

        self.set_profile_to(
            match to_default {
                #[cfg(test)]
                super::WriteToDefaultProfile::Isolated => Some(self.profile),
                super::WriteToDefaultProfile::WriteToDefault => None,
            },
            &temporary_profile,
        )?;

        Ok(())
    }

    /// Collect all the paths in the new set of packages.
    /// Returns an error if they have paths that will conflict with each other when installed.
    fn validate_paths_can_cohabitate(&self) -> Result<HashSet<PathBuf>, super::Error> {
        let mut all_new_paths = HashSet::<PathBuf>::new();

        for pkg in self.pkgs {
            let candidates =
                collect_children(pkg).map_err(super::Error::EnumeratingStorePathContent)?;

            let intersection = candidates
                .intersection(&all_new_paths)
                .cloned()
                .collect::<Vec<PathBuf>>();
            if !intersection.is_empty() {
                return Err(super::Error::PathConflict(pkg.to_path_buf(), intersection));
            }

            all_new_paths.extend(candidates.into_iter());
        }

        Ok(all_new_paths)
    }

    fn make_empty_profile(&self, profile: &Path) -> Result<(), super::Error> {
        // See: https://github.com/DeterminateSystems/nix-src/blob/f60b21563990ec11d87dd4abe57b8b187d6b6fb3/src/nix-env/buildenv.nix
        let output = std::process::Command::new(self.nix_store_path.join("bin/nix"))
            .set_nix_options(self.nss_ca_cert_path)?
            .args([
                "build",
                "--expr",
                r#"
                    derivation {
                        name = "user-environment";
                        system = "builtin";
                        builder = "builtin:buildenv";
                        derivations = [];
                        manifest = builtins.toFile "env-manifest.nix" "[]";
                    }
                "#,
                "--out-link",
            ])
            .arg(profile)
            .output()
            .map_err(|e| {
                super::Error::StartNixCommand("nix build-ing an empty profile".to_string(), e)
            })?;

        if !output.status.success() {
            return Err(super::Error::NixCommand(
                "nix build-ing an empty profile".to_string(),
                output,
            ));
        }

        Ok(())
    }

    fn set_profile_to(
        &self,
        profile: Option<&Path>,
        canon_profile: &Path,
    ) -> Result<(), super::Error> {
        tracing::debug!("Duplicating the existing profile into the scratch profile");

        let mut cmd = std::process::Command::new(self.nix_store_path.join("bin/nix-env"));

        cmd.set_nix_options(self.nss_ca_cert_path)?;

        if let Some(profile) = profile {
            cmd.arg("--profile");
            cmd.arg(profile);
        }

        let output = cmd.arg("--set").arg(canon_profile).output().map_err(|e| {
            super::Error::StartNixCommand(
                "Duplicating the default profile into the scratch profile".to_string(),
                e,
            )
        })?;

        if !output.status.success() {
            return Err(super::Error::NixCommand(
                "Duplicating the default profile into the scratch profile".to_string(),
                output,
            ));
        }

        Ok(())
    }

    fn collect_paths_by_package_output(
        &self,
        profile: &Path,
    ) -> Result<HashMap<PathBuf, HashSet<PathBuf>>, super::Error> {
        // Query packages that are already installed in the profile.
        // Constructs a map of (store path in the profile) -> (hash set of paths that are inside that store path)
        let mut installed_paths: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
        {
            let output = std::process::Command::new(self.nix_store_path.join("bin/nix-env"))
                .set_nix_options(self.nss_ca_cert_path)?
                .arg("--profile")
                .arg(profile)
                .args(["--query", "--installed", "--out-path", "--json"])
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| {
                    super::Error::StartNixCommand(
                        "nix-env --query'ing installed packages".to_string(),
                        e,
                    )
                })?;

            if !output.status.success() {
                return Err(super::Error::NixCommand(
                    "nix-env --query'ing installed packages".to_string(),
                    output,
                ));
            }

            let installed_pkgs: HashMap<String, PackageInfo> =
                serde_json::from_slice(&output.stdout)?;
            for pkg in installed_pkgs.values() {
                for path in pkg.outputs.values() {
                    installed_paths
                        .insert(path.clone(), collect_children(path).unwrap_or_default());
                }
            }
        }

        Ok(installed_paths)
    }

    fn uninstall_path(&self, profile: &Path, remove: &Path) -> Result<(), super::Error> {
        let output = std::process::Command::new(self.nix_store_path.join("bin/nix-env"))
            .set_nix_options(self.nss_ca_cert_path)?
            .arg("--profile")
            .arg(profile)
            .arg("--uninstall")
            .arg(remove)
            .output()
            .map_err(|e| {
                super::Error::StartNixCommand(
                    format!("nix-env --uninstall'ing conflicting package {:?}", remove),
                    e,
                )
            })?;

        if !output.status.success() {
            return Err(super::Error::NixCommand(
                format!("nix-env --uninstall'ing conflicting package {:?}", remove),
                output,
            ));
        }

        Ok(())
    }

    fn install_path(&self, profile: &Path, add: &Path) -> Result<(), super::Error> {
        let output = std::process::Command::new(self.nix_store_path.join("bin/nix-env"))
            .set_nix_options(self.nss_ca_cert_path)?
            .arg("--profile")
            .arg(profile)
            .arg("--install")
            .arg(add)
            .output()
            .map_err(|e| {
                super::Error::StartNixCommand(
                    format!("Adding the package {:?} to the profile", add),
                    e,
                )
            })?;

        if !output.status.success() {
            return Err(super::Error::AddPackage(add.to_path_buf(), output));
        }

        Ok(())
    }
}

#[derive(Debug, serde::Deserialize)]
struct PackageInfo {
    #[serde(default)]
    outputs: HashMap<String, PathBuf>,
}

fn collect_children<P: AsRef<std::path::Path>>(
    base_path: P,
) -> Result<HashSet<PathBuf>, std::io::Error> {
    let base_path = base_path.as_ref();
    let paths = walkdir::WalkDir::new(base_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|entry| -> Option<walkdir::DirEntry> {
            let entry = entry
                .inspect_err(
                    |e| tracing::debug!(?base_path, %e, "Error walking the file tree, skipping."),
                )
                .ok()?;

            if entry.file_type().is_dir() {
                None
            } else {
                Some(entry)
            }
        })
        .filter_map(|entry| {
            entry.path()
                .strip_prefix(base_path)
                .inspect_err(
                    |e| tracing::debug!(?base_path, path = ?entry.path(), %e, "Error stripping the prefix from the path, skipping."),
                )
                .ok()
                .map(PathBuf::from)
        })
        .collect::<HashSet<PathBuf>>();
    Ok(paths)
}

trait NixCommandExt {
    fn set_nix_options(
        &mut self,
        nss_ca_cert_pkg: &Path,
    ) -> Result<&mut std::process::Command, super::Error>;
}

impl NixCommandExt for std::process::Command {
    fn set_nix_options(
        &mut self,
        nss_ca_cert_pkg: &Path,
    ) -> Result<&mut std::process::Command, super::Error> {
        Ok(self
            .args(["--option", "substitute", "false"])
            .args(["--option", "post-build-hook", ""])
            .env("HOME", dirs::home_dir().ok_or(super::Error::NoRootHome)?)
            .env(
                "NIX_SSL_CERT_FILE",
                nss_ca_cert_pkg.join("etc/ssl/certs/ca-bundle.crt"),
            ))
    }
}
