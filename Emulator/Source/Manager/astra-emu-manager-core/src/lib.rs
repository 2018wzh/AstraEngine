mod desktop_source;
mod evidence;
mod family_loader;
mod filter;
mod identity;
mod library;
mod patch;
mod probe;
mod runtime_provider;
mod scanner;

pub use desktop_source::{
    DesktopGrantedSource, DesktopVfsRegistry, VfsAccessMetrics, VfsAuditSummary,
};
pub use evidence::*;
pub use family_loader::*;
pub use filter::{FilterBinding, FilterGraph, FilterLayer, FilterValidation};
pub use identity::{
    BangumiPlayStateRecord, DisplayTitle, ExternalIdentityRecord, InstallationRecord,
    MatchCandidateRecord, MatchDecisionRecord, MetadataSnapshotRecord, ProviderConsentRecord,
    ScanRunRecord, WorkRecord, MATCHER_VERSION,
};
pub use library::{
    CancellationToken, CaseRecord, CaseRuntimeProfileRecord, CoverCacheRecord, Library,
    LibraryError, ScanCandidate, ScanReport, SourceDiagnosticRecord, SourceGrant,
    TranslationCacheRecord, TranslationConsent, TranslationProfileRecord,
};
pub use patch::{
    PatchContext, PatchDiagnostic, PatchEffectIntent, PatchExecution, PatchHostAction,
    PatchVfsReader, TrustedPatchRuntime,
};
pub use probe::{AutoProbe, ProbeBinding, ProbeError, DEFAULT_PROBE_ORDER};
pub use runtime_provider::{AstraEmuRuntimeProvider, EmuCaseProfile, EmuStepPayload};
pub use scanner::{
    DiscoveryMarker, FamilyDiscoveryDescriptor, GrantedSourceEntry, GrantedSourceReader,
    LibraryScanner, ScanLimits, SourceScanError, DEFAULT_DISCOVERY_DESCRIPTORS,
};
