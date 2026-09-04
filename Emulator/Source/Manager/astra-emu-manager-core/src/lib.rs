mod compatibility_cache;
mod config_schema;
mod desktop_source;
mod evidence;
mod extension_loader;
mod family_loader;
mod filter;
mod host_services;
mod identity;
mod input_mapping;
mod library;
mod live_mapping;
mod patch;
mod play_record;
mod probe;
mod runtime_provider;
mod scanner;
mod work_settings;

pub use compatibility_cache::{
    CompatibilityCacheEntry, CompatibilityMatch, CompatibilitySyncState, VnReleaseRecord,
};
pub use config_schema::{
    extension_config_schema, family_config_schema, filter_config_schema, ConfigFieldDescriptor,
    ConfigFieldKind, ConfigSchema,
};
pub use desktop_source::{
    DesktopGrantedSource, DesktopVfsRegistry, VfsAccessMetrics, VfsAuditSummary, VfsResourceInfo,
};
pub use evidence::*;
pub use extension_loader::*;
pub use family_loader::*;
pub use filter::{FilterBinding, FilterGraph, FilterLayer, FilterValidation};
pub use host_services::*;
pub use identity::{
    BangumiPlayStateRecord, DisplayTitle, ExternalIdentityRecord, InstallationRecord,
    MatchCandidateRecord, MatchDecisionRecord, MetadataSnapshotRecord, ProviderConsentRecord,
    ScanRunRecord, WorkRecord, MATCHER_VERSION,
};
pub use input_mapping::{default_vn_preset, GamepadDeadzone, GamepadInput, InputMapping};
pub use library::{
    CancellationToken, CaseRecord, CaseRuntimeProfileRecord, CoverCacheRecord, Library,
    LibraryError, ScanCandidate, ScanReport, SourceDiagnosticRecord, SourceGrant,
    TranslationConsent, TranslationProfileRecord,
};
pub use live_mapping::{
    legacy_live_audio_command, legacy_live_audio_packet, legacy_live_video_command,
    legacy_texture_format, live_wait_condition, PendingLiveWait,
};
pub use patch::{
    PatchContext, PatchDiagnostic, PatchEffectIntent, PatchExecution, PatchHostAction,
    PatchVfsReader, TrustedPatchRuntime,
};
pub use play_record::{PlaySessionRecord, PlayStats, RecentWorkRecord};
pub use probe::{AutoProbe, ProbeBinding, ProbeError, DEFAULT_PROBE_ORDER};
pub use runtime_provider::{
    evidence_vm_coverage_ids, AstraEmuRuntimeProvider, AstraEmuRuntimeProviderFactory,
    EmuCaseProfile, QueuedPatchEffect,
};
pub use scanner::{
    DiscoveryMarker, FamilyDiscoveryDescriptor, GrantedSourceEntry, GrantedSourceReader,
    LibraryScanner, ScanLimits, SourceScanError, DEFAULT_DISCOVERY_DESCRIPTORS,
};
pub use work_settings::WorkSettings;
