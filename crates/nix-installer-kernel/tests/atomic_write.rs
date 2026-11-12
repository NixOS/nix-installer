//! Failure paths of `atomic_write`.

use std::{io, path::Path};

use nix_installer_kernel::fs::atomic_write;

#[test]
fn failed_rename_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("occupied");
    // An occupied target makes the rename fail after the temp file
    // exists.
    std::fs::create_dir(&target).unwrap();

    atomic_write(&target, b"contents", Some(0o644)).unwrap_err();

    let litter = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".nix-installer-tmp")
        })
        .count();
    assert_eq!(litter, 0);
    assert!(target.is_dir(), "the occupant is untouched");
}

#[test]
fn relative_paths_are_rejected_before_anything_is_written() {
    let err = atomic_write(Path::new("receipt.json"), b"{}", Some(0o644)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("absolute"));
}
