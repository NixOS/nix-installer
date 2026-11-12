/// Detected from the real host or fabricated in tests.
#[derive(Debug, Clone)]
pub struct HostFacts {
    pub is_darwin: bool,
}

impl HostFacts {
    #[cfg(target_os = "macos")]
    pub fn detect() -> Self {
        Self { is_darwin: true }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn detect() -> Self {
        Self { is_darwin: false }
    }

    pub fn fabricated(is_darwin: bool) -> Self {
        Self { is_darwin }
    }
}
