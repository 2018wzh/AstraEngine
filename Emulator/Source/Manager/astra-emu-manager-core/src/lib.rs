//! Independent AstraEMU Manager data and host-side configuration.
//!
//! The Manager persists library metadata, play records, and user settings. A
//! Family owns engine execution, native files, media, and native saves; this
//! crate does not depend on the product Runtime, package/save containers, or
//! the former Family service interfaces.

mod config_schema;
mod family;
mod family_loader;
mod family_registry;
mod input_mapping;
mod library;
mod metadata;
mod play;
mod work_settings;

pub use config_schema::{
    translation_config_schema, ConfigFieldDescriptor, ConfigFieldKind, ConfigSchema,
};
pub use family::{
    FamilyCapability, FamilyPluginDescriptor, FamilyPluginRegistry, FamilyPolicyError,
    FamilyProbeCandidate, FamilyProbeReport, FamilyProbeSelection,
    INDEPENDENT_FAMILY_ABI_FINGERPRINT,
};
pub use family_loader::{FamilyLoadError, LoadedFamilyPlugin};
pub use family_registry::FamilyProviderRegistry;
pub use input_mapping::{default_vn_preset, GamepadDeadzone, GamepadInput, InputMapping};
pub use library::{GameRecord, Library, LibraryError, PluginInstallRecord, VerifiedPluginInstall};
pub use metadata::{
    DisplayTitle, DisplayTitleSource, ExternalIdentityRecord, MetadataSnapshotRecord,
    MetadataSnapshotState,
};
pub use play::{PlaySessionEndReason, PlaySessionRecord, PlayStats, RecentGameRecord};
pub use work_settings::GameSettings;
