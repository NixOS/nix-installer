mod facts;

pub use facts::HostFacts;

use std::collections::BTreeMap;

use crate::{Cmd, CmdOutcome, Executor, Failure, Idempotency};

/// Owns every side-effecting facility a handler may use. Built after any
/// privilege transition, so no session state ever crosses an exec.
#[derive(Debug)]
pub struct Session {
    pub facts: HostFacts,
    /// Overlaid on the inherited environment for spawned commands. `PATH`
    /// here also drives bare-name resolution.
    pub env: BTreeMap<String, String>,
    pub executor: Executor,
    /// Total runs allowed per retried command, counting the first.
    pub max_spawn_attempts: u32,
}

impl Session {
    pub fn new(facts: HostFacts, executor: Executor) -> Self {
        Self {
            facts,
            env: BTreeMap::new(),
            executor,
            max_spawn_attempts: 3,
        }
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Retries transient failures. Only
    /// [`SafeToRepeat`](Idempotency::SafeToRepeat) commands may be re-run,
    /// the other classes need a probe.
    pub fn run_with_retry(
        &self,
        cmd: Cmd,
        is_transient: fn(&CmdOutcome) -> bool,
    ) -> Result<CmdOutcome, Failure> {
        if cmd.idempotency != Idempotency::SafeToRepeat {
            return Err(Failure::internal(format!(
                "refusing to retry `{}` classified {}; remediation is a probe",
                cmd.display(),
                cmd.idempotency
            )));
        }
        let mut outcome = cmd.run(self)?;
        for _ in 1..self.max_spawn_attempts.max(1) {
            if matches!(
                outcome,
                CmdOutcome::KilledWithoutStatus | CmdOutcome::NotRun
            ) || !is_transient(&outcome)
            {
                break;
            }
            outcome = cmd.run(self)?;
        }
        Ok(outcome)
    }
}
