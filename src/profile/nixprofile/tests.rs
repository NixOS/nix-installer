use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::super::WriteToDefaultProfile;
use super::NixCommandExt;
use super::NixProfile;

fn should_skip() -> bool {
    let cmdret = std::process::Command::new("nix")
        .set_nix_options(Path::new("/dev/null"))
        .unwrap()
        .arg("--version")
        .output();

    if cmdret.is_ok() {
        return false;
    } else {
        println!("Skipping this test because nix isn't in PATH");
        return true;
    }
}

fn sample_tree(dirname: &str, filename: &str, content: &str) -> PathBuf {
    let temp_dir = tempfile::tempdir().unwrap();

    let sub_dir = temp_dir.path().join(dirname);
    std::fs::create_dir(&sub_dir).unwrap();

    let file = sub_dir.join(filename);

    let mut f = std::fs::File::options()
        .create(true)
        .write(true)
        .open(&file)
        .unwrap();

    f.write_all(content.as_bytes()).unwrap();

    let mut cmdret = std::process::Command::new("nix")
        .set_nix_options(Path::new("/dev/null"))
        .unwrap()
        .args(&["store", "add"])
        .arg(&sub_dir)
        .output()
        .unwrap();

    assert!(
        cmdret.status.success(),
        "Running nix-store add failed: {:#?}",
        cmdret,
    );

    if cmdret.stdout.last() == Some(&b'\n') {
        cmdret.stdout.remove(cmdret.stdout.len() - 1);
    }

    let p = PathBuf::from(std::ffi::OsString::from_vec(cmdret.stdout));

    assert!(
        p.exists(),
        "Adding a path to the Nix store failed...: {:#?}",
        cmdret.stderr
    );

    p
}

#[test]
fn test_detect_intersection() {
    if should_skip() {
        return;
    }

    let profile = tempfile::tempdir().unwrap();
    let profile_path = profile.path().join("profile");

    let tree_1 = sample_tree("foo", "foo", "a");
    let tree_2 = sample_tree("bar", "foo", "b");

    (NixProfile {
        nix_store_path: Path::new("/nix/var/nix/profiles/default/"),
        nss_ca_cert_path: Path::new("/nix/var/nix/profiles/default/"),
        profile: &profile_path,
        pkgs: &[&tree_1, &tree_2],
    })
    .install_packages(WriteToDefaultProfile::Isolated)
    .unwrap_err();
}

#[test]
fn test_no_intersection() {
    if should_skip() {
        return;
    }

    let profile = tempfile::tempdir().unwrap();
    let profile_path = profile.path().join("profile");

    let tree_1 = sample_tree("foo", "foo", "a");
    let tree_2 = sample_tree("bar", "bar", "b");

    (NixProfile {
        nix_store_path: Path::new("/nix/var/nix/profiles/default/"),
        nss_ca_cert_path: Path::new("/nix/var/nix/profiles/default/"),
        profile: &profile_path,
        pkgs: &[&tree_1, &tree_2],
    })
    .install_packages(WriteToDefaultProfile::Isolated)
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(profile_path.join("foo")).unwrap(),
        "a"
    );
    assert_eq!(
        std::fs::read_to_string(profile_path.join("bar")).unwrap(),
        "b"
    );

    let tree_3 = sample_tree("baz", "baz", "c");
    let tree_4 = sample_tree("tux", "tux", "d");

    (NixProfile {
        nix_store_path: Path::new("/nix/var/nix/profiles/default/"),
        nss_ca_cert_path: Path::new("/nix/var/nix/profiles/default/"),
        profile: &profile_path,
        pkgs: &[&tree_3, &tree_4],
    })
    .install_packages(WriteToDefaultProfile::Isolated)
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(profile_path.join("baz")).unwrap(),
        "c"
    );
    assert_eq!(
        std::fs::read_to_string(profile_path.join("tux")).unwrap(),
        "d"
    );
}

#[test]
fn test_overlap_replaces() {
    if should_skip() {
        return;
    }

    let profile = tempfile::tempdir().unwrap();
    let profile_path = profile.path().join("profile");

    let tree_base = sample_tree("fizz", "fizz", "fizz");
    let tree_1 = sample_tree("foo", "foo", "a");
    (NixProfile {
        nix_store_path: Path::new("/nix/var/nix/profiles/default/"),
        nss_ca_cert_path: Path::new("/nix/var/nix/profiles/default/"),
        profile: &profile_path,
        pkgs: &[&tree_base, &tree_1],
    })
    .install_packages(WriteToDefaultProfile::Isolated)
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(profile_path.join("fizz")).unwrap(),
        "fizz"
    );
    assert_eq!(
        std::fs::read_to_string(profile_path.join("foo")).unwrap(),
        "a"
    );

    let tree_2 = sample_tree("foo", "foo", "b");
    (NixProfile {
        nix_store_path: Path::new("/nix/var/nix/profiles/default/"),
        nss_ca_cert_path: Path::new("/nix/var/nix/profiles/default/"),
        profile: &profile_path,
        pkgs: &[&tree_2],
    })
    .install_packages(WriteToDefaultProfile::Isolated)
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(profile_path.join("foo")).unwrap(),
        "b"
    );

    let tree_3 = sample_tree("bar", "foo", "c");
    (NixProfile {
        nix_store_path: Path::new("/nix/var/nix/profiles/default/"),
        nss_ca_cert_path: Path::new("/nix/var/nix/profiles/default/"),
        profile: &profile_path,
        pkgs: &[&tree_3],
    })
    .install_packages(WriteToDefaultProfile::Isolated)
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(profile_path.join("foo")).unwrap(),
        "c"
    );

    assert_eq!(
        std::fs::read_to_string(profile_path.join("fizz")).unwrap(),
        "fizz"
    );
}
