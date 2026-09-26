/// Validated package target and profile. Gameplay is composed by the product binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageProductContext {
    target: String,
    profile: String,
}

impl PackageProductContext {
    pub(crate) fn new(target: String, profile: String) -> Self {
        Self { target, profile }
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
}
