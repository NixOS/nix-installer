//! The recording executor runs nothing and reports no results, so
//! handlers degrade to `Indeterminate`.

mod common;

use common::{CreateUser, shim_session};
use nix_installer_kernel::{Assessment, Executor, Outcome, StepHandler};

fn user_spec() -> CreateUser {
    CreateUser {
        name: "nixbld1".into(),
        uid: 30001,
        primary_group: "nixbld".into(),
    }
}

#[test]
fn dry_run_assess_is_indeterminate() {
    let (_dir, session) = shim_session(Executor::record());
    match user_spec().assess(&session).unwrap() {
        Assessment::Indeterminate { .. } => {},
        other => panic!("dry-run assess must be indeterminate, got {other:?}"),
    }
}

#[test]
fn dry_run_apply_is_indeterminate_and_effect_free() {
    let (dir, mut session) = shim_session(Executor::record());

    match user_spec().apply(&mut session).unwrap() {
        Outcome::Indeterminate { .. } => {},
        other => panic!("dry-run apply must be indeterminate, got {other:?}"),
    }
    assert!(
        !dir.path().join("state").exists(),
        "dry-run must not create users"
    );
    assert_eq!(
        session.executor.recorded(),
        vec!["id -u nixbld1".to_string()],
        "only the probe was attempted"
    );
}

#[test]
fn dry_run_revert_is_indeterminate() {
    let (_dir, mut session) = shim_session(Executor::record());

    match user_spec().revert(&mut session).unwrap() {
        Outcome::Indeterminate { .. } => {},
        other => panic!("dry-run revert must be indeterminate, got {other:?}"),
    }
}
