//! The well-known `/nix`-relative paths.

use std::path::PathBuf;

pub fn store_root() -> PathBuf {
    PathBuf::from("/nix")
}

pub fn store() -> PathBuf {
    store_root().join("store")
}

pub fn state_dir() -> PathBuf {
    store_root().join("var").join("nix")
}

pub fn nix_db() -> PathBuf {
    state_dir().join("db")
}

pub fn daemon_socket_dir() -> PathBuf {
    state_dir().join("daemon-socket")
}

pub fn userpool() -> PathBuf {
    state_dir().join("userpool")
}

pub fn gcroots() -> PathBuf {
    state_dir().join("gcroots")
}

pub fn per_user_gcroots() -> PathBuf {
    gcroots().join("per-user")
}

pub fn profiles() -> PathBuf {
    state_dir().join("profiles")
}

pub fn default_profile() -> PathBuf {
    profiles().join("default")
}

pub fn per_user_profiles() -> PathBuf {
    profiles().join("per-user")
}

pub fn nix_conf_folder() -> PathBuf {
    PathBuf::from("/etc/nix")
}

pub fn nix_conf() -> PathBuf {
    nix_conf_folder().join("nix.conf")
}

pub fn receipt() -> PathBuf {
    store_root().join("receipt.json")
}

/// The installer self-copy that uninstall re-executes.
pub fn self_copy() -> PathBuf {
    store_root().join("nix-installer")
}
