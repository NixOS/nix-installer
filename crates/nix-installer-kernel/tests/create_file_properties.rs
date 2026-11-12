//! Properties of a filesystem step.

mod common;

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use common::{CreateFile, shim_session};
use nix_installer_kernel::{Assessment, Code, Executor, Outcome, StepHandler};

fn spec(dir: &std::path::Path) -> CreateFile {
    CreateFile {
        path: dir.join("nix.conf"),
        content: "experimental-features = nix-command flakes\n".into(),
        mode: 0o644,
    }
}

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn done_exactly_when_content_and_mode_match() {
    let (dir, mut session) = shim_session(Executor::Real);
    let step = spec(dir.path());

    step.apply(&mut session).unwrap();
    assert_eq!(step.assess(&session).unwrap(), Assessment::Done);

    std::fs::set_permissions(
        dir.path().join("nix.conf"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    match step.assess(&session).unwrap() {
        Assessment::Drift { detail, .. } => assert!(detail.contains("mode")),
        other => panic!("wrong mode must drift, got {other:?}"),
    }
}

#[test]
fn apply_twice_equals_apply_once() {
    let (dir, mut session) = shim_session(Executor::Real);
    let step = spec(dir.path());

    step.apply(&mut session).unwrap();
    let after_first = std::fs::read(dir.path().join("nix.conf")).unwrap();
    let inode_first = std::fs::metadata(dir.path().join("nix.conf"))
        .unwrap()
        .ino();
    assert_eq!(
        step.apply(&mut session).unwrap(),
        Outcome::Applied,
        "second apply is a verified no-op"
    );
    let after_second = std::fs::read(dir.path().join("nix.conf")).unwrap();
    assert_eq!(after_first, after_second);
    assert_eq!(mode_of(&dir.path().join("nix.conf")), 0o644);
    assert_eq!(
        std::fs::metadata(dir.path().join("nix.conf"))
            .unwrap()
            .ino(),
        inode_first,
        "a gated-off apply must not rewrite the file"
    );
}

#[test]
fn revert_restores_the_pre_state() {
    let (dir, mut session) = shim_session(Executor::Real);
    let step = spec(dir.path());
    let bystander = dir.path().join("bystander");
    std::fs::write(&bystander, "untouched").unwrap();

    step.apply(&mut session).unwrap();
    step.revert(&mut session).unwrap();

    assert!(!dir.path().join("nix.conf").exists());
    assert_eq!(
        std::fs::read_to_string(&bystander).unwrap(),
        "untouched",
        "revert must not disturb neighbors"
    );
    assert_eq!(step.assess(&session).unwrap(), Assessment::Todo);
}

#[test]
fn drift_is_refused_on_apply_and_kept_on_revert() {
    let (dir, mut session) = shim_session(Executor::Real);
    let step = spec(dir.path());
    std::fs::write(dir.path().join("nix.conf"), "hand-edited\n").unwrap();

    match step.assess(&session).unwrap() {
        Assessment::Drift { remedy, .. } => assert_eq!(remedy, Code::DifferentContent),
        other => panic!("edited file must drift, got {other:?}"),
    }
    let err = step.apply(&mut session).unwrap_err();
    assert_eq!(err.code, Code::DifferentContent);

    step.revert(&mut session).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("nix.conf")).unwrap(),
        "hand-edited\n",
        "revert must not delete the drifted file"
    );
}

#[test]
fn missing_parent_directory_is_an_io_failure() {
    let (dir, mut session) = shim_session(Executor::Real);
    let step = CreateFile {
        path: dir.path().join("no-such-dir").join("nix.conf"),
        content: String::new(),
        mode: 0o644,
    };

    assert_eq!(step.assess(&session).unwrap(), Assessment::Todo);
    let err = step.apply(&mut session).unwrap_err();
    assert_eq!(err.code, Code::Io);
}
