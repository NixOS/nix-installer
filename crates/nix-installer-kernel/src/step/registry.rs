use super::{ErasedStep, StepDecl, StepMeta, StepSpec};

/// A step name was registered twice.
#[derive(Debug, thiserror::Error)]
#[error("step `{0}` registered twice")]
pub struct DuplicateStep(pub &'static str);

#[derive(Debug, Clone, Copy)]
pub struct StepVTable {
    pub name: &'static str,
    pub meta: StepMeta,
    pub deserialize: fn(serde_json::Value) -> Result<Box<dyn ErasedStep>, serde_json::Error>,
}

impl StepVTable {
    pub fn of<T: StepSpec + 'static>() -> Self {
        StepVTable {
            name: <T as StepDecl>::META.name,
            meta: <T as StepDecl>::META,
            deserialize: deserialize::<T>,
        }
    }
}

fn deserialize<T: StepSpec + 'static>(
    value: serde_json::Value,
) -> Result<Box<dyn ErasedStep>, serde_json::Error> {
    Ok(Box::new(serde_json::from_value::<T>(value)?) as Box<dyn ErasedStep>)
}

/// Declared steps by name.
#[derive(Debug, Clone, Default)]
pub struct StepRegistry {
    entries: Vec<StepVTable>,
}

impl StepRegistry {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn register<T: StepSpec + 'static>(&mut self) -> Result<(), DuplicateStep> {
        let vtable = StepVTable::of::<T>();
        if self.entries.iter().any(|e| e.name == vtable.name) {
            return Err(DuplicateStep(vtable.name));
        }
        self.entries.push(vtable);
        Ok(())
    }

    pub fn with<T: StepSpec + 'static>(mut self) -> Result<Self, DuplicateStep> {
        self.register::<T>()?;
        Ok(self)
    }

    pub fn get(&self, name: &str) -> Option<StepVTable> {
        self.entries.iter().copied().find(|e| e.name == name)
    }

    /// `None` for an unregistered name. `Some(Err)` when the value does not
    /// fit that step.
    pub fn deserialize_name(
        &self,
        name: &str,
        value: serde_json::Value,
    ) -> Option<Result<Box<dyn ErasedStep>, serde_json::Error>> {
        self.get(name).map(|vtable| (vtable.deserialize)(value))
    }

    pub fn iter(&self) -> impl Iterator<Item = StepVTable> + '_ {
        self.entries.iter().copied()
    }

    /// In registration order.
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|e| e.name).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
