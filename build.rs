use std::{env, fs, path::PathBuf};

fn main() {
    // Embed the nix-mountd binary from NIX_MOUNTD_BINARY_PATH (set by the
    // flake), falling back to an empty placeholder so plain `cargo build` works.
    println!("cargo:rerun-if-env-changed=NIX_MOUNTD_BINARY_PATH");
    let path = match env::var_os("NIX_MOUNTD_BINARY_PATH") {
        Some(path) => PathBuf::from(path),
        None => {
            let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR unset"));
            let placeholder = out_dir.join("nix-mountd.placeholder");
            fs::write(&placeholder, b"").expect("write nix-mountd placeholder");
            placeholder
        },
    };
    println!("cargo:rustc-env=NIX_MOUNTD_EMBED_PATH={}", path.display());
}
