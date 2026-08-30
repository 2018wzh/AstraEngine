//! Stable, renderer-neutral contract between AstraEMU and legacy family providers.

mod ffi;
mod ffi_host;
mod ffi_wire;
mod input_key;
mod provider;
mod scheduler;
mod v9;

pub use ffi::*;
pub use ffi_host::*;
pub use ffi_wire::*;
pub use input_key::*;
pub use provider::*;
pub use scheduler::*;
pub use v9::*;

pub const LEGACY_FAMILY_API_SCHEMA: &str = "astra.emu.family_api.v12";
pub const LEGACY_EFFECT_SCHEMA: &str = "astra.emu.legacy_effect.v2";
/// Host-facing blackboard observation that transfers gameplay-input ownership
/// to family-owned system UI while its value is `"true"`.
pub const LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY: &str = "astra.emu.system_ui_active";

pub fn parse_legacy_system_ui_activity(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_legacy_system_ui_activity;

    #[test]
    fn system_ui_activity_accepts_only_canonical_boolean_values() {
        assert_eq!(parse_legacy_system_ui_activity("true"), Some(true));
        assert_eq!(parse_legacy_system_ui_activity("false"), Some(false));
        assert_eq!(parse_legacy_system_ui_activity("True"), None);
        assert_eq!(parse_legacy_system_ui_activity("1"), None);
    }
}
