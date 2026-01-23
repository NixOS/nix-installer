/*! CLI argument structures and utilities

*/

pub(crate) mod arg;
mod interaction;
pub(crate) mod subcommand;

use clap::Parser;
use eyre::WrapErr;
use owo_colors::OwoColorize;
use std::{
    ffi::CString,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use url::Url;

use self::subcommand::NixInstallerSubcommand;

pub use crate::plan::{CancelSignal, cancel_signal};

pub trait CommandExecute {
    fn execute(self) -> eyre::Result<ExitCode>;
}

/**
Experimental Nix Installer

A WIP replacement for the shell-based Nix installer (TODO: better description)
*/
#[derive(Debug, Parser)]
#[clap(version)]
pub struct NixInstallerCli {
    /// The proxy to use (if any); valid proxy bases are `https://$URL`, `http://$URL` and `socks5://$URL`
    #[cfg_attr(
        feature = "cli",
        clap(long, env = "NIX_INSTALLER_PROXY", global = true)
    )]
    pub proxy: Option<Url>,

    /// An SSL cert to use (if any); used for fetching Nix and sets `ssl-cert-file` in `/etc/nix/nix.conf`
    #[cfg_attr(
        feature = "cli",
        clap(long, env = "NIX_INSTALLER_SSL_CERT_FILE", global = true)
    )]
    pub ssl_cert_file: Option<PathBuf>,

    #[clap(flatten)]
    pub instrumentation: arg::Instrumentation,

    #[clap(subcommand)]
    pub subcommand: NixInstallerSubcommand,
}

impl CommandExecute for NixInstallerCli {
    #[tracing::instrument(level = "trace", skip_all)]
    fn execute(self) -> eyre::Result<ExitCode> {
        let is_install_subcommand = matches!(self.subcommand, NixInstallerSubcommand::Install(_));

        let ret = match self.subcommand {
            NixInstallerSubcommand::Plan(plan) => plan.execute(),
            NixInstallerSubcommand::SelfTest(self_test) => self_test.execute(),
            NixInstallerSubcommand::Install(install) => install.execute(),
            NixInstallerSubcommand::Repair(repair) => repair.execute(),
            NixInstallerSubcommand::Uninstall(revert) => revert.execute(),
            NixInstallerSubcommand::SplitReceipt(split_receipt) => split_receipt.execute(),
        };

        let maybe_cancelled = ret.as_ref().err().and_then(|err| {
            err.root_cause()
                .downcast_ref::<crate::NixInstallerError>()
                .and_then(|err| {
                    if matches!(err, crate::NixInstallerError::Cancelled) {
                        return Some(err);
                    }
                    None
                })
        });

        if let Some(cancelled) = maybe_cancelled {
            eprintln!("{}", cancelled.red());
            return Ok(ExitCode::FAILURE);
        }

        let is_macos = matches!(
            target_lexicon::OperatingSystem::host(),
            target_lexicon::OperatingSystem::MacOSX(_) | target_lexicon::OperatingSystem::Darwin(_)
        );

        if is_install_subcommand && is_macos {
            let is_ok_but_failed = ret.as_ref().is_ok_and(|code| code == &ExitCode::FAILURE);
            let is_error = ret.as_ref().is_err();

            if is_error || is_ok_but_failed {
                // NOTE: If the error bubbled up, print it before we log the pkg suggestion
                if let Err(ref err) = ret {
                    eprintln!("{err:?}\n");
                }

                return Ok(ExitCode::FAILURE);
            }
        }

        ret
    }
}

/// Set up a signal handler that sets the cancel flag when SIGINT or SIGTERM is received
pub fn setup_signal_handler() -> CancelSignal {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel.clone();

    ctrlc::set_handler(move || {
        tracing::warn!("Received interrupt signal");
        cancel_clone.store(true, Ordering::Relaxed);
    })
    .expect("Error setting signal handler");

    cancel
}

pub fn is_root() -> bool {
    let euid = nix::unistd::Uid::effective();
    tracing::trace!("Running as EUID {euid}");
    euid.is_root()
}

pub fn ensure_root() -> eyre::Result<()> {
    if !is_root() {
        eprintln!(
            "{}",
            "`nix-installer` needs to run as `root`, attempting to escalate now via `sudo`..."
                .yellow()
                .dimmed()
        );
        let sudo_cstring = CString::new("sudo").wrap_err("Making C string of `sudo`")?;
        let set_home_cstring =
            CString::new("--set-home").wrap_err("Making C string of `--set-home`")?;

        let args = std::env::args();
        let mut arg_vec_cstring = vec![];
        arg_vec_cstring.push(sudo_cstring.clone());
        arg_vec_cstring.push(set_home_cstring);

        let mut env_list = vec![];
        for (key, value) in std::env::vars() {
            let preserve = match key.as_str() {
                // Rust logging/backtrace bits we use
                "RUST_LOG" | "RUST_BACKTRACE" => true,
                // CI
                "GITHUB_PATH" => true,
                // Used for detecting what command to suggest for sourcing Nix
                "SHELL" => true,
                // Proxy settings (automatically picked up by Reqwest)
                "HTTP_PROXY" | "http_proxy" | "HTTPS_PROXY" | "https_proxy" => true,
                // Our own environments
                key if key.starts_with("NIX_INSTALLER") => true,
                // Kept for backward compatibility with existing installations
                key if key.starts_with("DETSYS_") => true,
                _ => false,
            };
            if preserve {
                env_list.push(format!("{key}={value}"));
            }
        }

        if !env_list.is_empty() {
            arg_vec_cstring
                .push(CString::new("env").wrap_err("Building a `env` argument for `sudo`")?);
            for env in env_list {
                arg_vec_cstring.push(
                    CString::new(env.clone())
                        .wrap_err_with(|| format!("Building a `{}` argument for `sudo`", env))?,
                );
            }
        }

        for arg in args {
            arg_vec_cstring.push(CString::new(arg).wrap_err("Making arg into C string")?);
        }

        tracing::trace!("Execvp'ing `{sudo_cstring:?}` with args `{arg_vec_cstring:?}`");
        nix::unistd::execvp(&sudo_cstring, &arg_vec_cstring)
            .wrap_err("Executing `nix-installer` as `root` via `sudo`")?;
    }
    Ok(())
}
