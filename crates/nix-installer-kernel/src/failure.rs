//! Stable failure codes.

/// Machine-readable failure class with stable identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Code {
    DifferentContent,
    /// A path exists with attributes (type, mode, ownership) conflicting
    /// with the spec.
    PathConflict,
    UserUidMismatch,
    MissingTool,
    /// A command failed in a way a re-run may fix.
    Transient,
    Io,
    Internal,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::DifferentContent => "E-CONF-003",
            Code::PathConflict => "E-CONF-002",
            Code::UserUidMismatch => "E-GROUP-001",
            Code::MissingTool => "E-TOOL-001",
            Code::Transient => "E-TRANSIENT-001",
            Code::Io => "E-IO-001",
            Code::Internal => "E-INTERNAL",
        }
    }
}

/// A step failure, displayed as `<detail> [<code>]`.
#[derive(Debug, thiserror::Error)]
#[error("{detail} [{}]", .code.as_str())]
pub struct Failure {
    pub code: Code,
    pub detail: String,
}

impl Failure {
    pub fn new(code: Code, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(Code::Internal, detail)
    }
}
