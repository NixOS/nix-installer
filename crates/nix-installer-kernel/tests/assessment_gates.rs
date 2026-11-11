//! The apply/revert gates and the probes.

mod common;

use common::shim_session;
use nix_installer_kernel::{
    ApplyGate, Assessment, Code, Executor, Failure, Observation, Outcome, Probe, RevertGate,
};

fn probe() -> Probe {
    Probe::UserExists {
        name: "nixbld1".into(),
    }
}

#[test]
fn apply_gate_maps_each_assessment() {
    assert_eq!(
        Assessment::Done.apply_gate().unwrap(),
        ApplyGate::Settled(Outcome::Applied)
    );
    assert_eq!(Assessment::Todo.apply_gate().unwrap(), ApplyGate::Proceed);
    assert_eq!(
        Assessment::Indeterminate { probe: probe() }
            .apply_gate()
            .unwrap(),
        ApplyGate::Settled(Outcome::Indeterminate { probe: probe() })
    );

    let err = Assessment::drift(Code::DifferentContent, "`/etc/nix/nix.conf` differs")
        .apply_gate()
        .unwrap_err();
    assert_eq!(err.code, Code::DifferentContent);
    assert!(err.detail.contains("nix.conf"));
}

#[test]
fn revert_gate_maps_each_assessment() {
    assert_eq!(Assessment::Done.revert_gate(), RevertGate::Revert);
    assert_eq!(Assessment::Todo.revert_gate(), RevertGate::NothingToDo);
    assert_eq!(
        Assessment::drift(Code::DifferentContent, "detail").revert_gate(),
        RevertGate::BestEffort {
            detail: "detail".into()
        }
    );
    assert_eq!(
        Assessment::Indeterminate { probe: probe() }.revert_gate(),
        RevertGate::Unknown { probe: probe() }
    );
}

#[test]
fn failure_codes_are_stable_strings() {
    assert_eq!(Code::DifferentContent.as_str(), "E-CONF-003");
    assert_eq!(Code::PathConflict.as_str(), "E-CONF-002");
    assert_eq!(Code::UserUidMismatch.as_str(), "E-GROUP-001");
    assert_eq!(Code::MissingTool.as_str(), "E-TOOL-001");
    assert_eq!(Code::Transient.as_str(), "E-TRANSIENT-001");
    assert_eq!(Code::Io.as_str(), "E-IO-001");
    assert_eq!(Code::Internal.as_str(), "E-INTERNAL");
    assert!(
        Failure::internal("boom")
            .to_string()
            .contains("[E-INTERNAL]")
    );
}

#[test]
fn probes_describe_their_subject() {
    assert!(
        Probe::UserExists {
            name: "nixbld1".into()
        }
        .describe()
        .contains("nixbld1")
    );
    assert!(
        Probe::UnitActive {
            unit: "nix-daemon.socket".into()
        }
        .describe()
        .contains("nix-daemon.socket")
    );
    assert!(
        Probe::PathContent {
            path: "/etc/nix/nix.conf".into()
        }
        .describe()
        .contains("/etc/nix/nix.conf")
    );
}

#[test]
fn path_probe_observes_content_absence_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let session = plain_session();
    let present = Probe::PathContent {
        path: dir.path().join("present"),
    };
    std::fs::write(dir.path().join("present"), "contents").unwrap();
    assert_eq!(
        present.observe(&session),
        Observation::Present("contents".into())
    );

    let absent = Probe::PathContent {
        path: dir.path().join("absent"),
    };
    assert_eq!(absent.observe(&session), Observation::Absent);

    // Reading a directory errors, which is Unavailable and not Absent.
    let dir_probe = Probe::PathContent {
        path: dir.path().to_path_buf(),
    };
    assert_eq!(dir_probe.observe(&session), Observation::Unavailable);
}

fn plain_session() -> nix_installer_kernel::Session {
    nix_installer_kernel::Session::new(
        nix_installer_kernel::HostFacts::fabricated(false),
        Executor::Real,
    )
}

#[test]
fn user_probe_observes_through_shims() {
    let (dir, session) = shim_session(Executor::Real);
    std::fs::write(dir.path().join("state"), "nixbld1 30001 nixbld\n").unwrap();

    let present = Probe::UserExists {
        name: "nixbld1".into(),
    };
    assert_eq!(
        present.observe(&session),
        Observation::Present("30001".into())
    );

    let absent = Probe::UserExists {
        name: "nobody-else".into(),
    };
    assert_eq!(absent.observe(&session), Observation::Absent);
}

#[test]
fn user_probe_under_recording_is_unavailable() {
    let (_dir, session) = shim_session(Executor::record());
    let probe = Probe::UserExists {
        name: "nixbld1".into(),
    };
    assert_eq!(probe.observe(&session), Observation::Unavailable);
}
