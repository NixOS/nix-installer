//! Lifecycle and fault-injection tests for the shim-driven user step.

mod common;

use common::{CreateUser, logged_session, shim_session};
use nix_installer_kernel::{Assessment, Code, Executor, Outcome, Session, StepHandler};

fn user_spec() -> CreateUser {
    CreateUser {
        name: "nixbld1".into(),
        uid: 30001,
        primary_group: "nixbld".into(),
    }
}

/// Invocations the useradd shim recorded, if any.
fn useradd_runs(dir: &std::path::Path) -> usize {
    std::fs::read_to_string(dir.join("log"))
        .map(|log| log.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

#[test]
fn user_lifecycle() {
    let (_dir, mut session) = shim_session(Executor::Real);
    let step = user_spec();

    assert_eq!(step.assess(&session).unwrap(), Assessment::Todo);
    assert_eq!(step.apply(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(step.assess(&session).unwrap(), Assessment::Done);
    assert_eq!(step.revert(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(step.assess(&session).unwrap(), Assessment::Todo);
}

#[test]
fn uid_drift_is_refused_then_reverted_best_effort() {
    let (dir, mut session) = shim_session(Executor::Real);
    std::fs::write(dir.path().join("state"), "nixbld1 29999 nixbld\n").unwrap();

    let step = user_spec();
    match step.assess(&session).unwrap() {
        Assessment::Drift { remedy, .. } => assert_eq!(remedy, Code::UserUidMismatch),
        other => panic!("expected uid drift, got {other:?}"),
    }
    assert_eq!(
        step.apply(&mut session).unwrap_err().code,
        Code::UserUidMismatch
    );
    step.revert(&mut session).unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join("state"))
            .unwrap()
            .contains("nixbld1 29999"),
        "revert must not delete the drifted user"
    );
}

#[test]
fn already_satisfied_apply_is_a_verified_noop() {
    let (dir, mut session) = logged_session(Executor::Real);
    std::fs::write(dir.path().join("state"), "nixbld1 30001 nixbld\n").unwrap();

    let step = user_spec();
    assert_eq!(step.assess(&session).unwrap(), Assessment::Done);
    assert_eq!(step.apply(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(
        useradd_runs(dir.path()),
        0,
        "the gate, not the shim's EEXIST, must produce the no-op"
    );
}

#[test]
fn probe_failure_beyond_no_such_user_is_transient() {
    let (_dir, mut session) = shim_session(Executor::Real);
    session
        .env
        .insert("KERNEL_TEST_ID_STATUS".to_string(), "2".to_string());

    let err = user_spec().assess(&session).unwrap_err();
    assert_eq!(err.code, Code::Transient);
}

/// `Todo` applies, `Indeterminate` re-assesses, `Drift` refuses.
fn drive_to_done(step: &CreateUser, session: &mut Session) -> bool {
    for _ in 0..6 {
        match step.assess(session).unwrap() {
            Assessment::Done => return true,
            Assessment::Todo => {
                assert_eq!(step.apply(session).unwrap(), Outcome::Applied);
            },
            Assessment::Indeterminate { probe } => {
                tracing::debug!("indeterminate: {}", probe.describe());
            },
            Assessment::Drift { detail, .. } => panic!("unexpected drift: {detail}"),
        }
    }
    false
}

#[test]
fn killed_probe_converges_through_indeterminate() {
    let (_dir, mut session) = shim_session(Executor::fault_at(1));
    assert!(
        drive_to_done(&user_spec(), &mut session),
        "install must converge after a killed spawn"
    );
}

#[test]
fn killed_create_converges_next_apply() {
    // Spawn 3 is useradd, after the test's assess and apply's re-assess.
    let (_dir, mut session) = shim_session(Executor::fault_at(3));
    let step = user_spec();

    assert_eq!(step.assess(&session).unwrap(), Assessment::Todo);
    match step.apply(&mut session).unwrap() {
        Outcome::Indeterminate { .. } => {},
        other => panic!("expected indeterminate after killed useradd, got {other:?}"),
    }

    // A fresh session, as after a re-run.
    let (_dir2, mut session2) = shim_session(Executor::Real);
    assert_eq!(step.apply(&mut session2).unwrap(), Outcome::Applied);
    assert_eq!(step.assess(&session2).unwrap(), Assessment::Done);
}
