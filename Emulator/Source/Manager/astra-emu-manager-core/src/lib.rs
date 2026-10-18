//! Independent AstraEMU Manager data and host-side configuration.
//!
//! The Manager persists library metadata, play records, and user settings. A
//! Family owns engine execution, native files, media, and native saves; this
//! crate does not depend on the product Runtime, package/save containers, or
//! the former Family service interfaces.

mod appearance;
pub mod extensions;
mod family;
mod family_loader;
mod family_registry;
mod filter_settings;
mod input_mapping;
mod library;
mod metadata;
pub mod model;
mod native_cancel;
mod native_config;
mod native_frame;
mod native_module_owner;
mod play;
mod work_settings;
pub use native_cancel::NativeCancellation;
#[cfg(test)]
extern crate self as astra_emu_manager_core;
#[cfg(test)]
mod test_native;
pub use family_diagnostics::{
    set_emit as set_diagnostic_emit, set_enabled as set_diagnostic_enabled,
    set_flush as set_diagnostic_flush, NativeDiagnostic,
};

pub use appearance::AppearanceSettings;
pub use family::{
    FamilyCapability, FamilyPluginDescriptor, FamilyPluginRegistry, FamilyPolicyError,
    FamilyProbeCandidate, FamilyProbeReport, FamilyProbeSelection,
    INDEPENDENT_FAMILY_ABI_FINGERPRINT,
};
pub use family_loader::{
    no_host_services, FamilyLoadError, LoadedFamilyPlugin, NativeOpenRequest, NativeSession,
};
pub use family_registry::{
    is_family_library_path, FamilyDirectoryLoadError, FamilyDirectoryLoadReport,
    FamilyProviderRegistry,
};
pub use filter_settings::{FilterConfiguration, FilterPreset, FilterSettings};
pub use input_mapping::{
    default_vn_preset, input_key_code, GamepadDeadzone, GamepadInput, InputMapping,
};
pub use library::{GameRecord, Library, LibraryError, LibrarySnapshot};
pub use metadata::{
    DisplayTitle, DisplayTitleSource, ExternalIdentityRecord, MetadataSnapshotRecord,
    MetadataSnapshotState,
};
pub use play::{PlaySessionEndReason, PlaySessionRecord, PlayStats, RecentGameRecord};
pub use work_settings::GameSettings;

mod family_configuration;

mod family_diagnostics;
