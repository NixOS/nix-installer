//! Binaries embedded into the installer at build time (see `build.rs`).

/// The nix-mountd helper for the target platform, or empty when this build did
/// not embed one.
pub const NIX_MOUNTD_BINARY: &[u8] = include_bytes!(env!("NIX_MOUNTD_EMBED_PATH"));

/// Whether a usable nix-mountd binary was embedded.
// Const-evaluated against the embedded bytes, which are empty in builds that do
// not set NIX_MOUNTD_BINARY_PATH (e.g. plain `cargo build`/CI on Linux).
#[allow(clippy::const_is_empty)]
pub const NIX_MOUNTD_AVAILABLE: bool = !NIX_MOUNTD_BINARY.is_empty();
