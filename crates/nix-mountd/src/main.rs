//! Mount the Nix Store volume, then keep it mounted against AWS EC2's macOS
//! deployment, which periodically unmounts internal disks.

use std::path::PathBuf;

use clap::Parser;

#[cfg(target_os = "macos")]
mod disk_arbitration;

#[derive(Parser)]
#[command(name = "nix-mountd", about, version)]
struct Cli {
    /// APFS volume label to mount.
    #[arg(long, default_value = "Nix Store", env = "NIX_MOUNTD_VOLUME_LABEL")]
    volume_label: String,

    /// Mount point to keep mounted.
    #[arg(long, default_value = "/nix", env = "NIX_MOUNTD_MOUNT_POINT")]
    mount_point: PathBuf,
}

fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    #[cfg(not(target_os = "macos"))]
    {
        let _ = cli;
        Err(std::io::Error::other("nix-mountd only supports macOS"))
    }

    #[cfg(target_os = "macos")]
    {
        if !is_mounted(&cli.mount_point)? {
            mount(&cli.volume_label, &cli.mount_point)?;
        }
        disk_arbitration::run(&cli.mount_point)
    }
}

#[cfg(target_os = "macos")]
fn is_mounted(mount_point: &std::path::Path) -> std::io::Result<bool> {
    let output = std::process::Command::new("/sbin/mount").output()?;
    let needle = format!(" on {} (", mount_point.display());
    Ok(String::from_utf8_lossy(&output.stdout).contains(&needle))
}

#[cfg(target_os = "macos")]
fn mount(volume_label: &str, mount_point: &std::path::Path) -> std::io::Result<()> {
    tracing::info!(volume_label, mount_point = %mount_point.display(), "Mounting");
    let status = std::process::Command::new("/usr/sbin/diskutil")
        .arg("mount")
        .arg("-mountPoint")
        .arg(mount_point)
        .arg(volume_label)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "diskutil mount failed with {status}"
        )));
    }
    Ok(())
}
