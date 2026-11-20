//! Declared step specs and their behavior.

mod assessment;
mod registry;

pub use assessment::{ApplyGate, Assessment, Observation, Outcome, Probe, RevertGate};
pub use registry::{DuplicateStep, StepRegistry, StepVTable};

/// Static description of a step, emitted by `#[step]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepMeta {
    /// The serde tag, the registry name, and the journal identity.
    pub name: &'static str,
    pub category: &'static str,
    /// Paths this step may modify.
    pub touches: &'static [&'static str],
    pub revert: RevertPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevertPolicy {
    Revert,
    /// The effects survive uninstall. Reverting them would destroy state
    /// the installer did not create.
    Keep,
}

/// A step whose metadata was declared by `#[step]`.
pub trait StepDecl {
    const META: StepMeta;
}

/// `assess` and `apply` must change together. A disagreement between them
/// breaks crash convergence.
pub trait StepHandler {
    /// Compare the spec against the world.
    fn assess(&self, session: &crate::Session) -> Result<Assessment, crate::Failure>;
    /// Bring the world to the spec. Implementations re-assess first.
    fn apply(&self, session: &mut crate::Session) -> Result<Outcome, crate::Failure>;
    /// Undo the spec's effects, best-effort on drift.
    fn revert(&self, session: &mut crate::Session) -> Result<Outcome, crate::Failure>;
}

/// A step that is declared, serializable and handled, so the registry can
/// dispatch it.
pub trait StepSpec:
    StepHandler + StepDecl + serde::Serialize + serde::de::DeserializeOwned + Send + Sync
{
}

impl<T> StepSpec for T where
    T: StepHandler + StepDecl + serde::Serialize + serde::de::DeserializeOwned + Send + Sync
{
}

/// A step usable without its concrete type.
pub trait ErasedStep {
    fn name(&self) -> &'static str;
    fn meta(&self) -> StepMeta;
    fn assess(&self, session: &crate::Session) -> Result<Assessment, crate::Failure>;
    fn apply(&self, session: &mut crate::Session) -> Result<Outcome, crate::Failure>;
    fn revert(&self, session: &mut crate::Session) -> Result<Outcome, crate::Failure>;
    fn to_value(&self) -> Result<serde_json::Value, serde_json::Error>;
}

impl<T: StepSpec + 'static> ErasedStep for T {
    fn name(&self) -> &'static str {
        T::META.name
    }

    fn meta(&self) -> StepMeta {
        T::META
    }

    fn assess(&self, session: &crate::Session) -> Result<Assessment, crate::Failure> {
        StepHandler::assess(self, session)
    }

    fn apply(&self, session: &mut crate::Session) -> Result<Outcome, crate::Failure> {
        StepHandler::apply(self, session)
    }

    fn revert(&self, session: &mut crate::Session) -> Result<Outcome, crate::Failure> {
        StepHandler::revert(self, session)
    }

    fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }
}
