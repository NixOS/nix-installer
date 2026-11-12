//! What `assess` and `apply` return.

use std::path::PathBuf;

use crate::{Cmd, CmdOutcome, Code, Failure, Idempotency, Session};

/// `id -u` exit status for an unknown user.
const ID_NO_SUCH_USER: i32 = 1;
/// `systemctl is-active` exit status for an inactive unit. Any other
/// non-zero status is an error.
const SYSTEMCTL_INACTIVE: i32 = 3;

/// How the world compares to a step's spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    Done,
    Todo,
    /// A human likely changed the world. Apply refuses to overwrite it and
    /// `remedy` names the way out.
    Drift {
        detail: String,
        remedy: Code,
    },
    /// Cannot tell, usually because a spawn died without an exit status.
    /// Re-assess with the probe instead of repeating the spawn.
    Indeterminate {
        probe: Probe,
    },
}

impl Assessment {
    pub fn drift(remedy: Code, detail: impl Into<String>) -> Self {
        Assessment::Drift {
            detail: detail.into(),
            remedy,
        }
    }

    /// Drift is refused with its remedy code.
    pub fn apply_gate(self) -> Result<ApplyGate, Failure> {
        match self {
            Assessment::Done => Ok(ApplyGate::Settled(Outcome::Applied)),
            Assessment::Todo => Ok(ApplyGate::Proceed),
            Assessment::Drift { detail, remedy } => Err(Failure::new(remedy, detail)),
            Assessment::Indeterminate { probe } => {
                Ok(ApplyGate::Settled(Outcome::Indeterminate { probe }))
            },
        }
    }

    pub fn revert_gate(self) -> RevertGate {
        match self {
            Assessment::Done => RevertGate::Revert,
            Assessment::Todo => RevertGate::NothingToDo,
            Assessment::Drift { detail, .. } => RevertGate::BestEffort { detail },
            Assessment::Indeterminate { probe } => RevertGate::Unknown { probe },
        }
    }
}

/// What `apply` does after assessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyGate {
    /// Run the effect.
    Proceed,
    /// Skip the effect and return this outcome.
    Settled(Outcome),
}

/// What `revert` does for each assessment. Handlers implement only the
/// `Revert` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertGate {
    Revert,
    NothingToDo,
    /// The world drifted from the spec. Revert must not fail the uninstall
    /// over it.
    BestEffort {
        detail: String,
    },
    Unknown {
        probe: Probe,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The world was verified to match the intended end state.
    Applied,
    /// The effect possibly happened. Re-assess to resolve.
    Indeterminate { probe: Probe },
}

/// How to re-check an indeterminate effect. A probe is a fresh check, not
/// a retry of a dead spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    UserExists { name: String },
    UnitActive { unit: String },
    PathContent { path: PathBuf },
}

/// What a [`Probe`] saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// Carries the uid, unit state or file content.
    Present(String),
    Absent,
    /// The check itself failed or did not run.
    Unavailable,
}

impl Probe {
    pub fn describe(&self) -> String {
        match self {
            Probe::UserExists { name } => format!("does user `{name}` exist, and with which uid"),
            Probe::UnitActive { unit } => format!("is unit `{unit}` active"),
            Probe::PathContent { path } => {
                format!("does `{}` exist, and with which content", path.display())
            },
        }
    }

    /// Command-backed probes report [`Observation::Unavailable`] when the
    /// executor only records.
    pub fn observe(&self, session: &Session) -> Observation {
        match self {
            Probe::UserExists { name } => observe_user(session, name),
            Probe::UnitActive { unit } => observe_unit(session, unit),
            Probe::PathContent { path } => observe_path(path),
        }
    }
}

fn user_lookup_cmd(name: &str) -> Cmd {
    Cmd::new("id")
        .arg("-u")
        .arg(name.to_string())
        .idempotency(Idempotency::SafeToRepeat)
}

fn unit_check_cmd(unit: &str) -> Cmd {
    Cmd::new("systemctl")
        .arg("is-active")
        .arg(unit.to_string())
        .idempotency(Idempotency::SafeToRepeat)
}

fn observe_user(session: &Session, name: &str) -> Observation {
    match user_lookup_cmd(name).run(session) {
        Ok(CmdOutcome::Success { stdout, .. }) => Observation::Present(stdout.trim().to_string()),
        Ok(CmdOutcome::Failure {
            status: ID_NO_SUCH_USER,
            ..
        }) => Observation::Absent,
        _ => Observation::Unavailable,
    }
}

fn observe_unit(session: &Session, unit: &str) -> Observation {
    match unit_check_cmd(unit).run(session) {
        Ok(CmdOutcome::Success { stdout, .. }) => Observation::Present(stdout.trim().to_string()),
        Ok(CmdOutcome::Failure {
            status: SYSTEMCTL_INACTIVE,
            ..
        }) => Observation::Absent,
        _ => Observation::Unavailable,
    }
}

fn observe_path(path: &PathBuf) -> Observation {
    match std::fs::read_to_string(path) {
        Ok(content) => Observation::Present(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Observation::Absent,
        Err(_) => Observation::Unavailable,
    }
}
