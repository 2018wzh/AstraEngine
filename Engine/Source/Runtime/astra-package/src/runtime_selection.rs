/// Gameplay runtime compiled into the current product family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageRuntimeKind {
    NativeVn,
}

/// Native runtime choice after package authority validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRuntimeSelection {
    kind: PackageRuntimeKind,
    target: String,
    profile: String,
}

impl PackageRuntimeSelection {
    pub(crate) fn native_vn(target: String, profile: String) -> Self {
        Self {
            kind: PackageRuntimeKind::NativeVn,
            target,
            profile,
        }
    }
    pub fn kind(&self) -> PackageRuntimeKind {
        self.kind
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
}
