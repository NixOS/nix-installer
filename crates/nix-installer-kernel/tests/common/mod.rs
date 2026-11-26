// Each test binary links this module but uses only part of it.
#![allow(dead_code)]

//! Test-only steps, driven by the shims in `tests/shims` so everything
//! runs unprivileged.

use std::{
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use nix_installer_kernel::{
    ApplyGate, Assessment, Cmd, CmdOutcome, Code, Executor, Failure, HostFacts, Idempotency, Outcome, Probe,
    RevertGate, Session, StepHandler, StepRegistry, fs::atomic_write,
};
use nix_installer_macros::step;

/// `id -u` exits 1 for no such user.
const ID_NO_SUCH_USER: i32 = 1;
/// `useradd` exits 9 when the user exists, and the shim does the same.
const USERADD_EEXIST: i32 = 9;

#[step(name = "create_file", category = "files")]
pub struct CreateFile {
    pub path: PathBuf,
    pub content: String,
    pub mode: u32,
}

impl CreateFile {
    fn read_planned(&self) -> Result<Vec<u8>, Failure> {
        std::fs::read(&self.path).map_err(|err| io_failure("reading", &self.path, err))
    }

    fn content_drifts(&self, actual: &[u8]) -> Option<Assessment> {
        (actual != self.content.as_bytes()).then(|| {
            Assessment::drift(
                Code::DifferentContent,
                format!(
                    "`{}` exists with different content than planned",
                    self.path.display()
                ),
            )
        })
    }

    fn mode_drifts(&self, actual: u32) -> Option<Assessment> {
        (actual != self.mode).then(|| {
            Assessment::drift(
                Code::DifferentContent,
                format!(
                    "`{}` exists with mode {actual:o}, planned {:o}",
                    self.path.display(),
                    self.mode
                ),
            )
        })
    }
}

impl StepHandler for CreateFile {
    fn assess(&self, _session: &Session) -> Result<Assessment, Failure> {
        match std::fs::metadata(&self.path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Assessment::Todo),
            Err(_) => Ok(Assessment::Indeterminate {
                probe: Probe::PathContent {
                    path: self.path.clone(),
                },
            }),
            Ok(metadata) => {
                let actual = self.read_planned()?;
                Ok(self
                    .content_drifts(&actual)
                    .or_else(|| self.mode_drifts(metadata.permissions().mode() & 0o777))
                    .unwrap_or(Assessment::Done))
            },
        }
    }

    fn apply(&self, session: &mut Session) -> Result<Outcome, Failure> {
        if let ApplyGate::Settled(outcome) = self.assess(session)?.apply_gate()? {
            return Ok(outcome);
        }
        atomic_write(&self.path, self.content.as_bytes(), Some(self.mode))
            .map_err(|err| io_failure("writing", &self.path, err))?;
        Ok(Outcome::Applied)
    }

    fn revert(&self, session: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(session)?.revert_gate() {
            RevertGate::Revert => {
                std::fs::remove_file(&self.path)
                    .map_err(|err| io_failure("removing", &self.path, err))?;
                Ok(Outcome::Applied)
            },
            RevertGate::NothingToDo => Ok(Outcome::Applied),
            RevertGate::BestEffort { detail } => {
                tracing::warn!(
                    "leaving drifted `{}` untouched ({detail})",
                    self.path.display()
                );
                Ok(Outcome::Applied)
            },
            RevertGate::Unknown { probe } => Ok(Outcome::Indeterminate { probe }),
        }
    }
}

#[step(name = "create_user", category = "users")]
pub struct CreateUser {
    pub name: String,
    pub uid: u32,
    pub primary_group: String,
}

enum Lookup {
    Known(Option<u32>),
    Unknown,
}

impl CreateUser {
    fn lookup_cmd(&self) -> Cmd {
        Cmd::new("id")
            .arg("-u")
            .arg(self.name.clone())
            .idempotency(Idempotency::SafeToRepeat)
    }

    fn create_cmd(&self) -> Cmd {
        Cmd::new("useradd")
            .arg("-u")
            .arg(self.uid.to_string())
            .arg("-g")
            .arg(self.primary_group.clone())
            .arg(self.name.clone())
            .idempotency(Idempotency::ProbeFirst)
    }

    fn delete_cmd(&self) -> Cmd {
        Cmd::new("userdel")
            .arg(self.name.clone())
            .idempotency(Idempotency::SafeToRepeat)
    }

    fn lookup(&self, session: &Session) -> Result<Lookup, Failure> {
        match self.lookup_cmd().run(session)? {
            CmdOutcome::Success { stdout, .. } => match stdout.trim().parse::<u32>() {
                Ok(uid) => Ok(Lookup::Known(Some(uid))),
                Err(_) => Err(Failure::internal(format!(
                    "`id -u {}` printed unparsable output `{stdout}`",
                    self.name
                ))),
            },
            CmdOutcome::Failure {
                status: ID_NO_SUCH_USER,
                ..
            } => Ok(Lookup::Known(None)),
            CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                Code::Transient,
                format!(
                    "`id -u {}` failed with status {status}: {stderr}",
                    self.name
                ),
            )),
            CmdOutcome::KilledWithoutStatus | CmdOutcome::NotRun => Ok(Lookup::Unknown),
        }
    }

    fn probe(&self) -> Probe {
        Probe::UserExists {
            name: self.name.clone(),
        }
    }

    fn indeterminate(&self) -> Outcome {
        Outcome::Indeterminate {
            probe: self.probe(),
        }
    }

    /// Decides by re-probing and never by retrying.
    fn resolve_existing(&self, session: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(session)?.apply_gate()? {
            ApplyGate::Settled(Outcome::Applied) => Ok(Outcome::Applied),
            other => Err(Failure::internal(format!(
                "user creation reported already-exists, but the re-probe says {other:?}"
            ))),
        }
    }
}

impl StepHandler for CreateUser {
    fn assess(&self, session: &Session) -> Result<Assessment, Failure> {
        match self.lookup(session)? {
            Lookup::Known(None) => Ok(Assessment::Todo),
            Lookup::Known(Some(uid)) if uid == self.uid => Ok(Assessment::Done),
            Lookup::Known(Some(other)) => Ok(Assessment::drift(
                Code::UserUidMismatch,
                format!(
                    "user `{}` exists with uid {other}, but {} was planned",
                    self.name, self.uid
                ),
            )),
            Lookup::Unknown => Ok(Assessment::Indeterminate {
                probe: self.probe(),
            }),
        }
    }

    fn apply(&self, session: &mut Session) -> Result<Outcome, Failure> {
        if let ApplyGate::Settled(outcome) = self.assess(session)?.apply_gate()? {
            return Ok(outcome);
        }
        match self.create_cmd().run(session)? {
            CmdOutcome::Success { .. } => Ok(Outcome::Applied),
            CmdOutcome::Failure {
                status: USERADD_EEXIST,
                ..
            } => self.resolve_existing(session),
            CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                Code::Transient,
                format!(
                    "creating user `{}` failed with status {status}: {stderr}",
                    self.name
                ),
            )),
            CmdOutcome::KilledWithoutStatus | CmdOutcome::NotRun => Ok(self.indeterminate()),
        }
    }

    fn revert(&self, session: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(session)?.revert_gate() {
            RevertGate::Revert => match self.delete_cmd().run(session)? {
                CmdOutcome::Success { .. } => Ok(Outcome::Applied),
                CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                    Code::Transient,
                    format!(
                        "deleting user `{}` failed with status {status}: {stderr}",
                        self.name
                    ),
                )),
                CmdOutcome::KilledWithoutStatus | CmdOutcome::NotRun => Ok(self.indeterminate()),
            },
            RevertGate::NothingToDo => Ok(Outcome::Applied),
            RevertGate::BestEffort { detail } => {
                tracing::warn!("not deleting drifted user `{}` ({detail})", self.name);
                Ok(Outcome::Applied)
            },
            RevertGate::Unknown { .. } => Ok(self.indeterminate()),
        }
    }
}

fn io_failure(what: &str, path: &Path, err: std::io::Error) -> Failure {
    Failure::new(Code::Io, format!("{what} `{}`: {err}", path.display()))
}

pub fn test_registry() -> StepRegistry {
    let mut registry = StepRegistry::new();
    registry.register::<CreateFile>().unwrap();
    registry.register::<CreateUser>().unwrap();
    registry
}

/// A session with the shims on `PATH` and a per-test state file.
pub fn shim_session(executor: Executor) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = Session::new(HostFacts::fabricated(false), executor)
        .with_env("PATH", shim_path())
        .with_env("KERNEL_TEST_MARKER", "marker")
        .with_env(
            "KERNEL_TEST_STATE",
            dir.path().join("state").display().to_string(),
        );
    (dir, session)
}

/// A shim session with an invocation log, for retry tests.
pub fn logged_session(executor: Executor) -> (tempfile::TempDir, Session) {
    let (dir, mut session) = shim_session(executor);
    session.env.insert(
        "KERNEL_TEST_LOG".to_string(),
        dir.path().join("log").display().to_string(),
    );
    (dir, session)
}

fn shim_path() -> String {
    let shims = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/shims");
    let inherited = std::env::var("PATH").unwrap_or_default();
    format!("{}:{}", shims.display(), inherited)
}
