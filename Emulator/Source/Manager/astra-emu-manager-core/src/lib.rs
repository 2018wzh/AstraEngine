mod desktop_source;
mod evidence;
mod family_loader;
mod filter;
mod library;
mod patch;
mod probe;
mod runtime_provider;
mod scanner;

pub use desktop_source::{DesktopGrantedSource, DesktopVfsRegistry};
pub use evidence::*;
pub use family_loader::*;
pub use filter::{FilterBinding, FilterGraph, FilterLayer, FilterValidation};
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
    GrantedSourceEntry, GrantedSourceReader, LibraryScanner, ScanLimits, SourceScanError,
};
