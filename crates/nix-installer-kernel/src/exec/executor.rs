use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    os::unix::fs::PermissionsExt as _,
    path::PathBuf,
    process::{Command, Stdio},
};

use super::{Cmd, CmdOutcome};
use crate::{Code, Failure};

#[derive(Debug)]
pub enum Executor {
    Real,
    /// Records invocations instead of executing them.
    Record(RefCell<Vec<String>>),
    /// Reports [`CmdOutcome::KilledWithoutStatus`] for the `fail_at_nth`
    /// spawn (1-based). All others run for real.
    FaultInject {
        fail_at_nth: usize,
        spawned: Cell<usize>,
    },
}

impl Executor {
    pub fn record() -> Self {
        Executor::Record(RefCell::new(Vec::new()))
    }

    pub fn fault_at(nth: usize) -> Self {
        Executor::FaultInject {
            fail_at_nth: nth,
            spawned: Cell::new(0),
        }
    }

    /// Empty unless this is the `Record` executor.
    pub fn recorded(&self) -> Vec<String> {
        match self {
            Executor::Record(log) => log.borrow().clone(),
            _ => Vec::new(),
        }
    }

    pub(super) fn execute(
        &self,
        cmd: &Cmd,
        env: &BTreeMap<String, String>,
    ) -> Result<CmdOutcome, Failure> {
        match self {
            Executor::Record(log) => {
                log.borrow_mut().push(cmd.display());
                Ok(CmdOutcome::NotRun)
            },
            Executor::FaultInject {
                fail_at_nth,
                spawned,
            } => {
                let n = spawned.get() + 1;
                spawned.set(n);
                if n == *fail_at_nth {
                    Ok(CmdOutcome::KilledWithoutStatus)
                } else {
                    spawn(cmd, env)
                }
            },
            Executor::Real => spawn(cmd, env),
        }
    }
}

fn spawn(cmd: &Cmd, env: &BTreeMap<String, String>) -> Result<CmdOutcome, Failure> {
    let program = resolve_program(&cmd.program, env)?;
    let mut command = Command::new(&program);
    command.args(&cmd.args).envs(env).stdin(Stdio::null());
    command
        .output()
        .map(outcome_of)
        .map_err(|err| spawn_failure(cmd, &err))
}

/// Only `NotFound` means the tool is absent. `EACCES`, `ENOEXEC` and the
/// rest call for different remedies.
fn spawn_failure(cmd: &Cmd, err: &std::io::Error) -> Failure {
    let code = if err.kind() == std::io::ErrorKind::NotFound {
        Code::MissingTool
    } else {
        Code::Io
    };
    Failure::new(code, format!("could not execute `{}`: {err}", cmd.program))
}

fn outcome_of(output: std::process::Output) -> CmdOutcome {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    match output.status.code() {
        Some(0) => CmdOutcome::Success { stdout, stderr },
        Some(status) => CmdOutcome::Failure {
            status,
            stdout,
            stderr,
        },
        None => CmdOutcome::KilledWithoutStatus,
    }
}

/// Names containing `/` are used as given. Bare names resolve against the
/// session's `PATH`, or the process's own only when the session has none.
fn resolve_program(program: &str, env: &BTreeMap<String, String>) -> Result<PathBuf, Failure> {
    if program.contains('/') {
        return Ok(PathBuf::from(program));
    }
    let path = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    path_split(&path)
        .map(|dir| dir.join(program))
        .find(is_executable_file)
        .ok_or_else(|| Failure::new(Code::MissingTool, format!("`{program}` not found in PATH")))
}

/// Like `execvp`, a non-executable file is not a match.
fn is_executable_file(path: &PathBuf) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn path_split(path: &str) -> impl Iterator<Item = PathBuf> + '_ {
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}
