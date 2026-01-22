// .filter_map() predicates returns Some/None, which is more clear than .filter()'s -> bool predicates.
#![allow(clippy::unnecessary_filter_map)]

/*! The [Nix](https://github.com/NixOS/nix) installer

`nix-installer` breaks down into three main concepts:

* [`Action`]: An executable or revertable step, possibly orchestrating sub-[`Action`]s.
* [`InstallPlan`]: A set of [`Action`]s, along with some metadata, which can be carried out to
  drive an install or revert.
* [`Planner`](planner::Planner): Something which can be used to plan out an [`InstallPlan`].

It is possible to create custom [`Action`]s and [`Planner`](planner::Planner)s to suit the needs of your project, team, or organization.

In the simplest case, `nix-installer` can be asked to determine a default plan for the platform and install
it, uninstalling if anything goes wrong:

```rust,no_run
use std::error::Error;
use nix_installer::InstallPlan;
# fn default_install() -> color_eyre::Result<()> {
let mut plan = InstallPlan::default()?;
match plan.install(None) {
    Ok(()) => tracing::info!("Done"),
    Err(e) => {
        match e.source() {
            Some(source) => tracing::error!("{e}: {}", source),
            None => tracing::error!("{e}"),
        };
        plan.uninstall(None)?;
    },
};
#
# Ok(())
# }
```
Sometimes choosing a specific planner is desired:
```rust,no_run
use std::error::Error;
use nix_installer::{InstallPlan, planner::Planner};

# fn chosen_planner_install() -> color_eyre::Result<()> {
#[cfg(target_os = "linux")]
let planner = nix_installer::planner::steam_deck::SteamDeck::default()?;
#[cfg(target_os = "macos")]
let planner = nix_installer::planner::macos::Macos::default()?;

// Or call `crate::planner::BuiltinPlanner::default()`
// Match on the result to customize.

// Customize any settings...

let mut plan = InstallPlan::plan(planner)?;
match plan.install(None) {
    Ok(()) => tracing::info!("Done"),
    Err(e) => {
        match e.source() {
            Some(source) => tracing::error!("{e}: {}", source),
            None => tracing::error!("{e}"),
        };
        plan.uninstall(None)?;
    },
};
#
# Ok(())
# }
```

*/

pub mod action;
#[cfg(feature = "cli")]
pub mod cli;
mod error;
mod os;
mod plan;
pub mod planner;
mod profile;
pub mod self_test;
pub mod settings;
mod util;

use std::{ffi::OsStr, path::Path, process::Output};

pub use error::NixInstallerError;
pub use plan::InstallPlan;
use planner::BuiltinPlanner;

use std::process::Command;

use crate::action::{Action, ActionErrorKind};

#[tracing::instrument(level = "debug", skip_all, fields(command = %format!("{:?}", command)))]
fn execute_command(command: &mut Command) -> Result<Output, ActionErrorKind> {
    tracing::trace!("Executing");
    let output = command
        .output()
        .map_err(|e| ActionErrorKind::command(command, e))?;
    match output.status.success() {
        true => {
            tracing::trace!(
                stderr = %String::from_utf8_lossy(&output.stderr),
                stdout = %String::from_utf8_lossy(&output.stdout),
                "Command success"
            );
            Ok(output)
        },
        false => Err(ActionErrorKind::command_output(command, output)),
    }
}

#[tracing::instrument(level = "debug", skip_all, fields(
    k = %k.as_ref().to_string_lossy(),
    v = %v.as_ref().to_string_lossy(),
))]
fn set_env(k: impl AsRef<OsStr>, v: impl AsRef<OsStr>) {
    tracing::trace!("Setting env");
    std::env::set_var(k.as_ref(), v.as_ref());
}

fn parse_ssl_cert(
    ssl_cert_file: &Path,
) -> Result<rustls::pki_types::CertificateDer<'static>, CertificateError> {
    let cert_buf = std::fs::read(ssl_cert_file)
        .map_err(|e| CertificateError::Read(ssl_cert_file.to_path_buf(), e))?;

    // Try PEM format first
    if let Ok(pem) = pem::parse(&cert_buf) {
        return Ok(rustls::pki_types::CertificateDer::from(pem.into_contents()));
    }

    // Try DER format
    Ok(rustls::pki_types::CertificateDer::from(cert_buf))
}

#[derive(Debug, thiserror::Error)]
pub enum CertificateError {
    #[error("Read path `{0}`")]
    Read(std::path::PathBuf, #[source] std::io::Error),
    #[error("Unknown certificate format, `der` and `pem` supported")]
    UnknownCertFormat,
}
