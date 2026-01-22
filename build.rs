use std::env;
use std::path::Path;

fn main() {
    // Get the tarball path from environment (set by flake.nix)
    let tarball_path = env::var("NIX_TARBALL_PATH")
        .expect("NIX_TARBALL_PATH must be set - build with `nix build` or `nix develop`");

    // Verify the tarball exists
    if !Path::new(&tarball_path).exists() {
        panic!("NIX_TARBALL_PATH points to non-existent file: {tarball_path}");
    }

    // Verify other required env vars are set
    env::var("NIX_STORE_PATH").expect("NIX_STORE_PATH must be set");
    env::var("NSS_CACERT_STORE_PATH").expect("NSS_CACERT_STORE_PATH must be set");
    env::var("NIX_VERSION").expect("NIX_VERSION must be set");

    // Tell cargo to rerun if any of these change
    println!("cargo:rerun-if-env-changed=NIX_TARBALL_PATH");
    println!("cargo:rerun-if-env-changed=NIX_STORE_PATH");
    println!("cargo:rerun-if-env-changed=NSS_CACERT_STORE_PATH");
    println!("cargo:rerun-if-env-changed=NIX_VERSION");
    println!("cargo:rerun-if-changed={tarball_path}");
}
