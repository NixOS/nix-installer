/*! The nix-installer kernel.

A step is a serializable spec declared with `#[step]` (in
`nix-installer-macros`), plus a [`StepHandler`] impl.

- [`assess`](StepHandler::assess) compares the spec to the world. Drift is
  refused and never silently overwritten. An indeterminate result carries a
  [`Probe`] for re-checking.
- [`apply`](StepHandler::apply) re-assesses first, so applying twice is a
  no-op.
- [`revert`](StepHandler::revert) is best-effort on drift, since refusing to
  uninstall over a user's edit would strand the machine.

Every process launch goes through [`Cmd`] and carries an [`Idempotency`]
class. Dry-run and fault injection are modes of [`Executor`].
*/

mod exec;
mod failure;
pub mod fs;
pub mod paths;
mod session;
mod step;

pub use exec::{Cmd, CmdOutcome, Executor, Idempotency};
pub use failure::{Code, Failure};
pub use session::{HostFacts, Session};
pub use step::{
    ApplyGate, Assessment, DuplicateStep, ErasedStep, Observation, Outcome, Probe, RevertGate, RevertPolicy,
    StepDecl, StepHandler, StepMeta, StepRegistry, StepSpec, StepVTable,
};
