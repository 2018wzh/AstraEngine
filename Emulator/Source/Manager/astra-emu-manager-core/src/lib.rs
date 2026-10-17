//! Independent AstraEMU Manager data and host-side configuration.
//!
//! The Manager persists library metadata, play records, and user settings. A
//! Family owns engine execution, native files, media, and native saves; this
//! crate does not depend on the product Runtime, package/save containers, or
//! the former Family service interfaces.

mod appearance;
mod family;
mod family_loader;
mod family_registry;
mod filter_settings;
mod input_mapping;
mod library;
mod metadata;
mod play;
mod work_settings;
pub mod model;
mod native_config;
mod native_frame;
pub mod extensions;
mod native_cancel;
mod native_module_owner;
pub use native_cancel::NativeCancellation;
#[cfg(test)]
extern crate self as astra_emu_manager_core;
#[cfg(test)]
mod test_native;
pub use family_diagnostics::{NativeDiagnostic, set_emit as set_diagnostic_emit, set_flush as set_diagnostic_flush, set_enabled as set_diagnostic_enabled};

pub use appearance::AppearanceSettings;
pub use family::{
    FamilyCapability, FamilyPluginDescriptor, FamilyPluginRegistry, FamilyPolicyError,
    FamilyProbeCandidate, FamilyProbeReport, FamilyProbeSelection,
    INDEPENDENT_FAMILY_ABI_FINGERPRINT,
};
pub use family_loader::{FamilyLoadError, LoadedFamilyPlugin, NativeOpenRequest, NativeSession, no_host_services};
pub use family_registry::{
    is_family_library_path, FamilyDirectoryLoadError, FamilyDirectoryLoadReport,
    FamilyProviderRegistry,
};
pub use filter_settings::{FilterConfiguration, FilterPreset, FilterSettings};
pub use input_mapping::{
    default_vn_preset, input_key_code, GamepadDeadzone, GamepadInput, InputMapping,
};
pub use library::{GameRecord, Library, LibraryError};
pub use metadata::{
    DisplayTitle, DisplayTitleSource, ExternalIdentityRecord, MetadataSnapshotRecord,
    MetadataSnapshotState,
};
pub use play::{PlaySessionEndReason, PlaySessionRecord, PlayStats, RecentGameRecord};
pub use work_settings::GameSettings;

mod family_configuration;

mod family_diagnostics;
