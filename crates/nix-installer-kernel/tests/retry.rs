//! Unknown effects refused, dead spawns never retried, transients
//! bounded.

mod common;

use std::path::PathBuf;

use common::logged_session;
use nix_installer_kernel::{Cmd, CmdOutcome, Code, Executor, Idempotency};

fn invocations(dir: &tempfile::TempDir) -> usize {
    std::fs::read_to_string(log_path(dir))
        .map(|log| log.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

fn log_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("log")
}

fn is_transient(outcome: &CmdOutcome) -> bool {
    matches!(outcome, CmdOutcome::Failure { .. })
}

#[test]
fn refuses_unknown_effect() {
    let (_dir, session) = logged_session(Executor::Real);
    let cmd = Cmd::new("useradd")
        .arg("oops")
        .idempotency(Idempotency::UnknownEffect);

    let err = session
        .run_with_retry(cmd, is_transient)
        .expect_err("UnknownEffect must not be retried");
    assert_eq!(err.code, Code::Internal);
    assert!(err.detail.contains("probe"));
}

#[test]
fn refuses_probe_first_without_a_probe() {
    let (_dir, session) = logged_session(Executor::Real);
    let cmd = Cmd::new("useradd")
        .arg("nixbld1")
        .idempotency(Idempotency::ProbeFirst);

    let err = session
        .run_with_retry(cmd, is_transient)
        .expect_err("a ProbeFirst re-run must follow a re-probe, not the retry loop");
    assert_eq!(err.code, Code::Internal);
    assert_eq!(invocations(&_dir), 0);
}

#[test]
fn killed_spawn_is_never_retried() {
    let (dir, session) = logged_session(Executor::fault_at(1));
    let cmd = Cmd::new("counting-fail").idempotency(Idempotency::SafeToRepeat);

    let outcome = session.run_with_retry(cmd, is_transient).unwrap();
    assert!(matches!(outcome, CmdOutcome::KilledWithoutStatus));
    assert_eq!(
        invocations(&dir),
        0,
        "the killed spawn must not be retried for real"
    );
}

#[test]
fn recorded_spawn_is_never_retried() {
    let (dir, session) = logged_session(Executor::record());
    let cmd = Cmd::new("counting-fail").idempotency(Idempotency::SafeToRepeat);

    let outcome = session.run_with_retry(cmd, is_transient).unwrap();
    assert!(matches!(outcome, CmdOutcome::NotRun));
    assert_eq!(session.executor.recorded().len(), 1);
    assert_eq!(invocations(&dir), 0);
}

#[test]
fn transient_failures_retry_to_the_bound() {
    let (dir, mut session) = logged_session(Executor::Real);
    session.max_spawn_attempts = 3;
    let cmd = Cmd::new("counting-fail").idempotency(Idempotency::SafeToRepeat);

    let outcome = session.run_with_retry(cmd, is_transient).unwrap();
    assert!(matches!(outcome, CmdOutcome::Failure { status: 1, .. }));
    assert_eq!(invocations(&dir), 3, "the budget bounds the attempts");
}

#[test]
fn transient_failure_recovers_within_the_bound() {
    let (dir, session) = logged_session(Executor::Real);
    let cmd = Cmd::new("succeed-second-try").idempotency(Idempotency::SafeToRepeat);

    let outcome = session.run_with_retry(cmd, is_transient).unwrap();
    assert!(matches!(outcome, CmdOutcome::Success { .. }));
    assert_eq!(invocations(&dir), 2);
}
