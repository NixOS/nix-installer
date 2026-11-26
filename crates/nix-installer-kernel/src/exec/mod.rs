//! [`Cmd`] is the only way the kernel spawns a process.

mod executor;

pub use executor::Executor;

/// Whether re-running a command after an unknown outcome is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    /// Re-running is always safe, e.g. a read-only probe.
    SafeToRepeat,
    /// Has a definite effect. Re-run only after a re-probe confirms it did
    /// not happen.
    ProbeFirst,
    /// May or may not have taken effect. Never retried. The remedy is a
    /// probe.
    UnknownEffect,
}

impl std::fmt::Display for Idempotency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Idempotency::SafeToRepeat => f.write_str("safe-to-repeat"),
            Idempotency::ProbeFirst => f.write_str("probe-first"),
            Idempotency::UnknownEffect => f.write_str("unknown-effect"),
        }
    }
}

/// The result of running a [`Cmd`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdOutcome {
    Success {
        stdout: String,
        stderr: String,
    },
    Failure {
        status: i32,
        stdout: String,
        stderr: String,
    },
    /// Died without an exit status (signal). Never retried. Maps to
    /// [`Indeterminate`](crate::Outcome::Indeterminate).
    KilledWithoutStatus,
    /// Recorded by a dry run and never spawned.
    NotRun,
}

/// A process invocation. Its idempotency defaults to the most conservative
/// class, [`Idempotency::UnknownEffect`].
#[derive(Debug, Clone)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub idempotency: Idempotency,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            idempotency: Idempotency::UnknownEffect,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, A>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn idempotency(mut self, idempotency: Idempotency) -> Self {
        self.idempotency = idempotency;
        self
    }

    /// The invocation as a single line.
    pub fn display(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }

    /// A spawn failure is `Err`. A non-zero exit is `Ok` with
    /// [`CmdOutcome::Failure`].
    pub fn run(&self, session: &crate::Session) -> Result<CmdOutcome, crate::Failure> {
        tracing::debug!(
            target: "nix_installer_kernel::cmd",
            program = self.program,
            args = self.args.join(" "),
            idempotency = %self.idempotency,
            "spawning"
        );
        session.executor.execute(self, &session.env)
    }
}
