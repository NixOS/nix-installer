//! Executors, `PATH` resolution, and environment pass-through.

mod common;

use common::shim_session;
use nix_installer_kernel::{Cmd, CmdOutcome, Code, Executor, Idempotency};

#[test]
fn recorded_cmd_is_not_run() {
    let (dir, session) = shim_session(Executor::record());
    let cmd = Cmd::new("useradd")
        .arg("-u")
        .arg("30001")
        .arg("nixbld1")
        .idempotency(Idempotency::ProbeFirst);

    let outcome = cmd.run(&session).unwrap();
    assert!(
        matches!(outcome, CmdOutcome::NotRun),
        "a recorded command claims no result"
    );
    assert_eq!(session.executor.recorded(), vec![cmd.display()]);
    assert!(
        !dir.path().join("state").exists(),
        "the shim must never have run"
    );
}

#[test]
fn fault_injection_kills_only_the_nth_spawn() {
    let (_dir, session) = shim_session(Executor::fault_at(1));
    let cmd = || Cmd::new("print-marker").idempotency(Idempotency::SafeToRepeat);

    assert!(matches!(
        cmd().run(&session).unwrap(),
        CmdOutcome::KilledWithoutStatus
    ));
    match cmd().run(&session).unwrap() {
        CmdOutcome::Success { stdout, .. } => assert_eq!(stdout.trim(), "marker"),
        other => panic!("second spawn should run for real, got {other:?}"),
    }
}

#[test]
fn session_path_resolves_bare_names() {
    let (_dir, session) = shim_session(Executor::Real);
    let cmd = Cmd::new("print-marker").idempotency(Idempotency::SafeToRepeat);

    match cmd.run(&session).unwrap() {
        CmdOutcome::Success { stdout, .. } => assert_eq!(stdout.trim(), "marker"),
        other => panic!("session PATH should resolve the shim, got {other:?}"),
    }
}

#[test]
fn absolute_program_bypasses_path() {
    let (_dir, session) = shim_session(Executor::Real);
    let cmd = Cmd::new("/bin/sh")
        .args(["-c", "echo absolute"])
        .idempotency(Idempotency::SafeToRepeat);

    match cmd.run(&session).unwrap() {
        CmdOutcome::Success { stdout, .. } => assert_eq!(stdout.trim(), "absolute"),
        other => panic!("absolute program should run, got {other:?}"),
    }
}

#[test]
fn child_sees_the_session_environment() {
    let (_dir, session) = shim_session(Executor::Real);
    let session = session.with_env("KERNEL_TEST_MARKER", "carried-through");
    let cmd = Cmd::new("print-marker").idempotency(Idempotency::SafeToRepeat);

    match cmd.run(&session).unwrap() {
        CmdOutcome::Success { stdout, .. } => assert_eq!(stdout.trim(), "carried-through"),
        other => panic!("env should reach the child, got {other:?}"),
    }
}

#[test]
fn nonzero_status_is_a_failure_outcome() {
    let (_dir, session) = shim_session(Executor::Real);
    let cmd = Cmd::new("exit-42").idempotency(Idempotency::SafeToRepeat);

    match cmd.run(&session).unwrap() {
        CmdOutcome::Failure { status: 42, .. } => {},
        other => panic!("expected status 42, got {other:?}"),
    }
}

#[test]
fn missing_program_is_a_missing_tool_failure() {
    let (_dir, session) = shim_session(Executor::Real);
    let cmd = Cmd::new("kernel-test-no-such-tool").idempotency(Idempotency::SafeToRepeat);

    let err = cmd.run(&session).unwrap_err();
    assert_eq!(err.code, Code::MissingTool);
}

#[test]
fn bare_names_fall_back_to_the_process_path() {
    let (_dir, mut session) = shim_session(Executor::Real);
    session.env.remove("PATH");
    let cmd = Cmd::new("sh")
        .args(["-c", "echo fallback"])
        .idempotency(Idempotency::SafeToRepeat);

    match cmd.run(&session).unwrap() {
        CmdOutcome::Success { stdout, .. } => assert_eq!(stdout.trim(), "fallback"),
        other => panic!("process PATH should resolve `sh`, got {other:?}"),
    }
}

#[test]
fn non_executable_candidates_are_skipped() {
    let shadow = tempfile::tempdir().unwrap();
    std::fs::write(shadow.path().join("sh"), "not an executable\n").unwrap();
    let inherited = std::env::var("PATH").unwrap_or_default();

    let (_dir, mut session) = shim_session(Executor::Real);
    session.env.insert(
        "PATH".to_string(),
        format!("{}:{inherited}", shadow.path().display()),
    );
    let cmd = Cmd::new("sh")
        .args(["-c", "echo real"])
        .idempotency(Idempotency::SafeToRepeat);

    match cmd.run(&session).unwrap() {
        CmdOutcome::Success { stdout, .. } => assert_eq!(stdout.trim(), "real"),
        other => panic!("a non-executable file must not shadow the real tool, got {other:?}"),
    }
}
