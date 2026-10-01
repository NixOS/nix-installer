use std::{io::IsTerminal, process::ExitCode};

use clap::{CommandFactory, FromArgMatches};
use nix_installer::{
    cli::{CommandExecute, NixInstallerCli},
    payload::PayloadError,
    settings::nix_version,
};

fn main() -> eyre::Result<ExitCode> {
    color_eyre::config::HookBuilder::default()
        .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new"))
        .add_issue_metadata("version", env!("CARGO_PKG_VERSION"))
        .add_issue_metadata("os", std::env::consts::OS)
        .add_issue_metadata("arch", std::env::consts::ARCH)
        .theme(if !std::io::stderr().is_terminal() {
            color_eyre::config::Theme::new()
        } else {
            color_eyre::config::Theme::dark()
        })
        .install()?;

    let embedded_nix_version = match nix_version() {
        Ok(v) => v,
        Err(PayloadError::Missing) => "<none>",
        Err(e) => return Err(e.into()),
    };

    let version = format!(
        "{} (Nix {})",
        env!("CARGO_PKG_VERSION"),
        embedded_nix_version
    );
    let mut cmd = NixInstallerCli::command().version(&version);
    let matches = cmd.get_matches_mut();
    let cli = NixInstallerCli::from_arg_matches(&matches)
        .map_err(|e| e.exit())
        .unwrap();

    cli.instrumentation.setup()?;

    tracing::info!("nix-installer {version}");

    cli.execute()
}
