use std::collections::BTreeMap;

use abi_stable::{
    std_types::{ROption, RString, RVec},
    StableAbi,
};
use astra_byte_source::{FfiOwnedByteBuffer, FfiOwnedF32Buffer, FfiOwnedI16Buffer};
use astra_core::{Hash256, SchemaVersion};

use crate::{
    FamilyId, LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7,
    LegacyAudioSampleFormat, LegacyAwaitResult, LegacyBlackboardMutation, LegacyControlTransaction,
    LegacyCoverageDelta, LegacyDiagnostic, LegacyDirtySection, LegacyEphemeralText, LegacyEvent,
    LegacyFamilyCoreKind, LegacyFamilyPluginDescriptor, LegacyFamilyPresentationMode,
    LegacyInputEdge, LegacyLiveOutput, LegacyOpenRequest, LegacyPcmBufferV7, LegacyProbeReport,
    LegacyProbeRequest, LegacyProviderError, LegacyProviderResult, LegacyReplayMode,
    LegacyRestoreReport, LegacyRuntimeHostCtx, LegacyRuntimeSessionId, LegacyRuntimeStatus,
    LegacySequenced, LegacyShutdownReport, LegacySnapshotEnvelope, LegacySnapshotSection,
    LegacyStepInput, LegacyStepOutput, LegacyTraceEntry, LegacyVfsListedFile, LegacyVideoCommandV1,
    LegacyVideoMode, LegacyVmTraceRecord, LegacyWaitRequest,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiHash256(pub [u8; 32]);

impl From<Hash256> for FfiHash256 {
    fn from(value: Hash256) -> Self {
        Self(*value.as_bytes())
    }
}

impl From<FfiHash256> for Hash256 {
    fn from(value: FfiHash256) -> Self {
        Self::from_bytes(value.0)
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiOwnedBytes {
    pub bytes: RVec<u8>,
}

impl FfiOwnedBytes {
    pub fn empty() -> Self {
        Self { bytes: RVec::new() }
    }

    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes.into_vec()
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiSchemaVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl From<SchemaVersion> for FfiSchemaVersion {
    fn from(value: SchemaVersion) -> Self {
        Self {
            major: value.major,
            minor: value.minor,
            patch: value.patch,
        }
    }
}

impl From<FfiSchemaVersion> for SchemaVersion {
    fn from(value: FfiSchemaVersion) -> Self {
        Self::new(value.major, value.minor, value.patch)
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiStringPair {
    pub key: RString,
    pub value: RString,
}

fn strings_to_ffi(values: Vec<String>) -> RVec<RString> {
    values
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>()
        .into()
}

fn strings_from_ffi(values: RVec<RString>) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn map_to_ffi(values: BTreeMap<String, String>) -> RVec<FfiStringPair> {
    values
        .into_iter()
        .map(|(key, value)| FfiStringPair {
            key: key.into(),
            value: value.into(),
        })
        .collect::<Vec<_>>()
        .into()
}

fn map_from_ffi(
    values: RVec<FfiStringPair>,
) -> Result<BTreeMap<String, String>, LegacyProviderError> {
    let mut result = BTreeMap::new();
    for pair in values.iter() {
        if result
            .insert(pair.key.to_string(), pair.value.to_string())
            .is_some()
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_MAP_DUPLICATE",
                "ABI pair list contains a duplicate key",
            ));
        }
    }
    Ok(result)
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiFamilyPluginDescriptor {
    pub family_id: RString,
    pub plugin_id: RString,
    pub provider_id: RString,
    pub core_kind: FfiFamilyCoreKind,
    pub presentation_mode: FfiFamilyPresentationMode,
    pub engine_version: RString,
    pub rustc_fingerprint: RString,
    pub feature_fingerprint: RString,
    pub abi_fingerprint: RString,
    pub supported_formats: RVec<RString>,
    pub permissions: RVec<RString>,
    pub report_redaction: RString,
    pub license: RString,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiFamilyCoreKind {
    Native,
    Ported,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiFamilyPresentationMode {
    SingleLayer,
    MultiLayer,
}

impl From<LegacyFamilyPluginDescriptor> for FfiFamilyPluginDescriptor {
    fn from(value: LegacyFamilyPluginDescriptor) -> Self {
        Self {
            family_id: value.family_id.0.into(),
            plugin_id: value.plugin_id.into(),
            provider_id: value.provider_id.into(),
            core_kind: match value.core_kind {
                LegacyFamilyCoreKind::Native => FfiFamilyCoreKind::Native,
                LegacyFamilyCoreKind::Ported => FfiFamilyCoreKind::Ported,
            },
            presentation_mode: match value.presentation_mode {
                LegacyFamilyPresentationMode::SingleLayer => FfiFamilyPresentationMode::SingleLayer,
                LegacyFamilyPresentationMode::MultiLayer => FfiFamilyPresentationMode::MultiLayer,
            },
            engine_version: value.engine_version.into(),
            rustc_fingerprint: value.rustc_fingerprint.into(),
            feature_fingerprint: value.feature_fingerprint.into(),
            abi_fingerprint: value.abi_fingerprint.into(),
            supported_formats: strings_to_ffi(value.supported_formats),
            permissions: strings_to_ffi(value.permissions),
            report_redaction: value.report_redaction.into(),
            license: value.license.into(),
        }
    }
}

impl From<FfiFamilyPluginDescriptor> for LegacyFamilyPluginDescriptor {
    fn from(value: FfiFamilyPluginDescriptor) -> Self {
        Self {
            family_id: FamilyId(value.family_id.to_string()),
            plugin_id: value.plugin_id.to_string(),
            provider_id: value.provider_id.to_string(),
            core_kind: match value.core_kind {
                FfiFamilyCoreKind::Native => LegacyFamilyCoreKind::Native,
                FfiFamilyCoreKind::Ported => LegacyFamilyCoreKind::Ported,
            },
            presentation_mode: match value.presentation_mode {
                FfiFamilyPresentationMode::SingleLayer => LegacyFamilyPresentationMode::SingleLayer,
                FfiFamilyPresentationMode::MultiLayer => LegacyFamilyPresentationMode::MultiLayer,
            },
            engine_version: value.engine_version.to_string(),
            rustc_fingerprint: value.rustc_fingerprint.to_string(),
            feature_fingerprint: value.feature_fingerprint.to_string(),
            abi_fingerprint: value.abi_fingerprint.to_string(),
            supported_formats: strings_from_ffi(value.supported_formats),
            permissions: strings_from_ffi(value.permissions),
            report_redaction: value.report_redaction.to_string(),
            license: value.license.to_string(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiRuntimeHostCtx {
    pub case_id: RString,
    pub package_id: RString,
    pub package_hash: FfiHash256,
    pub mount_set_id: RString,
    pub media_service_ids: RVec<RString>,
    pub permission_policy_id: RString,
    pub report_sink_id: RString,
    pub target: RString,
    pub profile: RString,
}

impl From<LegacyRuntimeHostCtx> for FfiRuntimeHostCtx {
    fn from(value: LegacyRuntimeHostCtx) -> Self {
        Self {
            case_id: value.case_id.into(),
            package_id: value.package_id.into(),
            package_hash: value.package_hash.into(),
            mount_set_id: value.mount_set_id.into(),
            media_service_ids: strings_to_ffi(value.media_service_ids),
            permission_policy_id: value.permission_policy_id.into(),
            report_sink_id: value.report_sink_id.into(),
            target: value.target.into(),
            profile: value.profile.into(),
        }
    }
}

impl From<FfiRuntimeHostCtx> for LegacyRuntimeHostCtx {
    fn from(value: FfiRuntimeHostCtx) -> Self {
        Self {
            case_id: value.case_id.to_string(),
            package_id: value.package_id.to_string(),
            package_hash: value.package_hash.into(),
            mount_set_id: value.mount_set_id.to_string(),
            media_service_ids: strings_from_ffi(value.media_service_ids),
            permission_policy_id: value.permission_policy_id.to_string(),
            report_sink_id: value.report_sink_id.to_string(),
            target: value.target.to_string(),
            profile: value.profile.to_string(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiProbeRequest {
    pub root_mount_id: RString,
    pub candidate_uris: RVec<RString>,
    pub marker_hashes: RVec<FfiHash256>,
    pub max_entries: u32,
    pub max_metadata_bytes: u64,
}

impl From<LegacyProbeRequest> for FfiProbeRequest {
    fn from(value: LegacyProbeRequest) -> Self {
        Self {
            root_mount_id: value.root_mount_id.into(),
            candidate_uris: strings_to_ffi(value.candidate_uris),
            marker_hashes: value
                .marker_hashes
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            max_entries: value.max_entries,
            max_metadata_bytes: value.max_metadata_bytes,
        }
    }
}

impl From<FfiProbeRequest> for LegacyProbeRequest {
    fn from(value: FfiProbeRequest) -> Self {
        Self {
            root_mount_id: value.root_mount_id.to_string(),
            candidate_uris: strings_from_ffi(value.candidate_uris),
            marker_hashes: value
                .marker_hashes
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            max_entries: value.max_entries,
            max_metadata_bytes: value.max_metadata_bytes,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiDiagnostic {
    pub code: RString,
    pub severity: RString,
    pub subject: RString,
    pub message: RString,
}

impl From<LegacyDiagnostic> for FfiDiagnostic {
    fn from(value: LegacyDiagnostic) -> Self {
        Self {
            code: value.code.into(),
            severity: value.severity.into(),
            subject: value.subject.into(),
            message: value.message.into(),
        }
    }
}

impl From<FfiDiagnostic> for LegacyDiagnostic {
    fn from(value: FfiDiagnostic) -> Self {
        Self {
            code: value.code.to_string(),
            severity: value.severity.to_string(),
            subject: value.subject.to_string(),
            message: value.message.to_string(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiProbeReport {
    pub family_id: RString,
    pub confidence_permyriad: u16,
    pub markers: RVec<RString>,
    pub blockers: RVec<FfiDiagnostic>,
    pub content_identity: FfiHash256,
}

impl From<LegacyProbeReport> for FfiProbeReport {
    fn from(value: LegacyProbeReport) -> Self {
        Self {
            family_id: value.family_id.0.into(),
            confidence_permyriad: value.confidence_permyriad,
            markers: strings_to_ffi(value.markers),
            blockers: value
                .blockers
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            content_identity: value.content_identity.into(),
        }
    }
}

impl From<FfiProbeReport> for LegacyProbeReport {
    fn from(value: FfiProbeReport) -> Self {
        Self {
            family_id: FamilyId(value.family_id.to_string()),
            confidence_permyriad: value.confidence_permyriad,
            markers: strings_from_ffi(value.markers),
            blockers: value.blockers.iter().cloned().map(Into::into).collect(),
            content_identity: value.content_identity.into(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiOpenRequest {
    pub requested_session_id: RString,
    pub case_fingerprint: FfiHash256,
    pub script_uri: RString,
    pub fixed_delta_ns: u64,
    pub session_seed: u64,
    pub compatibility_profile: RString,
    pub family_options: RVec<FfiStringPair>,
}

impl From<LegacyOpenRequest> for FfiOpenRequest {
    fn from(value: LegacyOpenRequest) -> Self {
        Self {
            requested_session_id: value.requested_session_id.0.into(),
            case_fingerprint: value.case_fingerprint.into(),
            script_uri: value.script_uri.into(),
            fixed_delta_ns: value.fixed_delta_ns,
            session_seed: value.session_seed,
            compatibility_profile: value.compatibility_profile.into(),
            family_options: map_to_ffi(value.family_options),
        }
    }
}

impl TryFrom<FfiOpenRequest> for LegacyOpenRequest {
    type Error = LegacyProviderError;

    fn try_from(value: FfiOpenRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            requested_session_id: LegacyRuntimeSessionId(value.requested_session_id.to_string()),
            case_fingerprint: value.case_fingerprint.into(),
            script_uri: value.script_uri.to_string(),
            fixed_delta_ns: value.fixed_delta_ns,
            session_seed: value.session_seed,
            compatibility_profile: value.compatibility_profile.to_string(),
            family_options: map_from_ffi(value.family_options)?,
        })
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiReplayMode {
    Live,
    RestoreContinuation,
}

impl From<LegacyReplayMode> for FfiReplayMode {
    fn from(value: LegacyReplayMode) -> Self {
        match value {
            LegacyReplayMode::Live => Self::Live,
            LegacyReplayMode::RestoreContinuation => Self::RestoreContinuation,
        }
    }
}

impl From<FfiReplayMode> for LegacyReplayMode {
    fn from(value: FfiReplayMode) -> Self {
        match value {
            FfiReplayMode::Live => Self::Live,
            FfiReplayMode::RestoreContinuation => Self::RestoreContinuation,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiInputEdge {
    pub control: RString,
    pub pressed: bool,
    pub value: f32,
    pub sequence: u64,
}

impl From<LegacyInputEdge> for FfiInputEdge {
    fn from(value: LegacyInputEdge) -> Self {
        Self {
            control: value.control.into(),
            pressed: value.pressed,
            value: value.value,
            sequence: value.sequence,
        }
    }
}

impl From<FfiInputEdge> for LegacyInputEdge {
    fn from(value: FfiInputEdge) -> Self {
        Self {
            control: value.control.to_string(),
            pressed: value.pressed,
            value: value.value,
            sequence: value.sequence,
        }
    }
}

macro_rules! ffi_result_item {
    ($ffi:ident, $native:ident, $($field:ident),+) => {
        #[repr(C)]
        #[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
        pub struct $ffi { $(pub $field: RString,)+ pub payload_len: u64, pub sequence: u64 }
    };
}

ffi_result_item!(FfiAwaitResult, LegacyAwaitResult, token_id, status);
ffi_result_item!(
    FfiProviderResult,
    LegacyProviderResult,
    request_id,
    provider_id,
    status
);

impl From<LegacyAwaitResult> for FfiAwaitResult {
    fn from(value: LegacyAwaitResult) -> Self {
        Self {
            token_id: value.token_id.into(),
            status: value.status.into(),
            payload_len: value.payload_len,
            sequence: value.sequence,
        }
    }
}
impl From<FfiAwaitResult> for LegacyAwaitResult {
    fn from(value: FfiAwaitResult) -> Self {
        Self {
            token_id: value.token_id.to_string(),
            status: value.status.to_string(),
            payload_len: value.payload_len,
            sequence: value.sequence,
        }
    }
}
impl From<LegacyProviderResult> for FfiProviderResult {
    fn from(value: LegacyProviderResult) -> Self {
        Self {
            request_id: value.request_id.into(),
            provider_id: value.provider_id.into(),
            status: value.status.into(),
            payload_len: value.payload_len,
            sequence: value.sequence,
        }
    }
}
impl From<FfiProviderResult> for LegacyProviderResult {
    fn from(value: FfiProviderResult) -> Self {
        Self {
            request_id: value.request_id.to_string(),
            provider_id: value.provider_id.to_string(),
            status: value.status.to_string(),
            payload_len: value.payload_len,
            sequence: value.sequence,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiStepInput {
    pub tick_index: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: FfiReplayMode,
    pub input_edges: RVec<FfiInputEdge>,
    pub await_results: RVec<FfiAwaitResult>,
    pub provider_results: RVec<FfiProviderResult>,
}

impl From<LegacyStepInput> for FfiStepInput {
    fn from(value: LegacyStepInput) -> Self {
        Self {
            tick_index: value.tick_index,
            delta_ns: value.delta_ns,
            session_seed: value.session_seed,
            mode: value.mode.into(),
            input_edges: value
                .input_edges
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            await_results: value
                .await_results
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            provider_results: value
                .provider_results
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

impl From<FfiStepInput> for LegacyStepInput {
    fn from(value: FfiStepInput) -> Self {
        Self {
            tick_index: value.tick_index,
            delta_ns: value.delta_ns,
            session_seed: value.session_seed,
            mode: value.mode.into(),
            input_edges: value.input_edges.iter().cloned().map(Into::into).collect(),
            await_results: value
                .await_results
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            provider_results: value
                .provider_results
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiRuntimeStatus {
    Active,
    Awaiting,
    Terminal,
    Faulted,
}

impl From<LegacyRuntimeStatus> for FfiRuntimeStatus {
    fn from(value: LegacyRuntimeStatus) -> Self {
        match value {
            LegacyRuntimeStatus::Active => Self::Active,
            LegacyRuntimeStatus::Awaiting => Self::Awaiting,
            LegacyRuntimeStatus::Terminal => Self::Terminal,
            LegacyRuntimeStatus::Faulted => Self::Faulted,
        }
    }
}
impl From<FfiRuntimeStatus> for LegacyRuntimeStatus {
    fn from(value: FfiRuntimeStatus) -> Self {
        match value {
            FfiRuntimeStatus::Active => Self::Active,
            FfiRuntimeStatus::Awaiting => Self::Awaiting,
            FfiRuntimeStatus::Terminal => Self::Terminal,
            FfiRuntimeStatus::Faulted => Self::Faulted,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub enum FfiWaitKind {
    Frame { frames: u32 },
    Time { milliseconds: u32 },
    Input { keys: RVec<RString> },
    MediaFence { media_id: RString },
    PresentationFence { fence_id: RString },
    ProviderCompletion { request_id: RString },
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiWaitRequest {
    pub token_id: RString,
    pub kind: FfiWaitKind,
}

impl From<LegacyWaitRequest> for FfiWaitRequest {
    fn from(value: LegacyWaitRequest) -> Self {
        match value {
            LegacyWaitRequest::Frame { token_id, frames } => Self {
                token_id: token_id.into(),
                kind: FfiWaitKind::Frame { frames },
            },
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => Self {
                token_id: token_id.into(),
                kind: FfiWaitKind::Time { milliseconds },
            },
            LegacyWaitRequest::Input { token_id, keys } => Self {
                token_id: token_id.into(),
                kind: FfiWaitKind::Input {
                    keys: strings_to_ffi(keys),
                },
            },
            LegacyWaitRequest::MediaFence { token_id, media_id } => Self {
                token_id: token_id.into(),
                kind: FfiWaitKind::MediaFence {
                    media_id: media_id.into(),
                },
            },
            LegacyWaitRequest::PresentationFence { token_id, fence_id } => Self {
                token_id: token_id.into(),
                kind: FfiWaitKind::PresentationFence {
                    fence_id: fence_id.into(),
                },
            },
            LegacyWaitRequest::ProviderCompletion {
                token_id,
                request_id,
            } => Self {
                token_id: token_id.into(),
                kind: FfiWaitKind::ProviderCompletion {
                    request_id: request_id.into(),
                },
            },
        }
    }
}

impl TryFrom<FfiWaitRequest> for LegacyWaitRequest {
    type Error = LegacyProviderError;
    fn try_from(value: FfiWaitRequest) -> Result<Self, Self::Error> {
        let token_id = value.token_id.to_string();
        Ok(match value.kind {
            FfiWaitKind::Frame { frames } => Self::Frame { token_id, frames },
            FfiWaitKind::Time { milliseconds } => Self::Time {
                token_id,
                milliseconds,
            },
            FfiWaitKind::Input { keys } => Self::Input {
                token_id,
                keys: strings_from_ffi(keys),
            },
            FfiWaitKind::MediaFence { media_id } => Self::MediaFence {
                token_id,
                media_id: media_id.to_string(),
            },
            FfiWaitKind::PresentationFence { fence_id } => Self::PresentationFence {
                token_id,
                fence_id: fence_id.to_string(),
            },
            FfiWaitKind::ProviderCompletion { request_id } => Self::ProviderCompletion {
                token_id,
                request_id: request_id.to_string(),
            },
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiTraceEntry {
    pub sequence: u64,
    pub context_id: u32,
    pub pc: u64,
    pub opcode: RString,
    pub action: ROption<RString>,
    pub yield_reason: ROption<RString>,
}

impl From<LegacyTraceEntry> for FfiTraceEntry {
    fn from(value: LegacyTraceEntry) -> Self {
        Self {
            sequence: value.sequence,
            context_id: value.context_id,
            pc: value.pc,
            opcode: value.opcode.into(),
            action: value.action.map(Into::into).into(),
            yield_reason: value.yield_reason.map(Into::into).into(),
        }
    }
}
impl From<FfiTraceEntry> for LegacyTraceEntry {
    fn from(value: FfiTraceEntry) -> Self {
        Self {
            sequence: value.sequence,
            context_id: value.context_id,
            pc: value.pc,
            opcode: value.opcode.to_string(),
            action: value.action.into_option().map(|v| v.to_string()),
            yield_reason: value.yield_reason.into_option().map(|v| v.to_string()),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiCoverageDelta {
    pub instructions: u64,
    pub syscalls: u64,
    pub contexts: RVec<u32>,
    pub presentation_commands: u64,
    pub audio_commands: u64,
    pub text_events: u64,
    pub capture_bytes: u64,
    pub operation_bytes: u64,
    pub scene_moved_bytes: u64,
    pub scene_copied_bytes: u64,
    pub pcm_moved_bytes: u64,
    pub pcm_copied_bytes: u64,
}

impl From<LegacyCoverageDelta> for FfiCoverageDelta {
    fn from(value: LegacyCoverageDelta) -> Self {
        Self {
            instructions: value.instructions,
            syscalls: value.syscalls,
            contexts: value.contexts.into(),
            presentation_commands: value.presentation_commands,
            audio_commands: value.audio_commands,
            text_events: value.text_events,
            capture_bytes: value.capture_bytes,
            operation_bytes: value.operation_bytes,
            scene_moved_bytes: value.scene_moved_bytes,
            scene_copied_bytes: value.scene_copied_bytes,
            pcm_moved_bytes: value.pcm_moved_bytes,
            pcm_copied_bytes: value.pcm_copied_bytes,
        }
    }
}
impl From<FfiCoverageDelta> for LegacyCoverageDelta {
    fn from(value: FfiCoverageDelta) -> Self {
        Self {
            instructions: value.instructions,
            syscalls: value.syscalls,
            contexts: value.contexts.iter().copied().collect(),
            presentation_commands: value.presentation_commands,
            audio_commands: value.audio_commands,
            text_events: value.text_events,
            capture_bytes: value.capture_bytes,
            operation_bytes: value.operation_bytes,
            scene_moved_bytes: value.scene_moved_bytes,
            scene_copied_bytes: value.scene_copied_bytes,
            pcm_moved_bytes: value.pcm_moved_bytes,
            pcm_copied_bytes: value.pcm_copied_bytes,
        }
    }
}

// Family ABI v7 live values own the allocation that moves through the
// provider boundary without a bytes envelope or an application-level copy.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveTextureFormat {
    Rgba8,
    LumaAlpha8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, StableAbi)]
pub struct FfiLiveVertex {
    pub position: [f32; 2],
    pub tex_coord: [f32; 2],
    pub color: [f32; 4],
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveBlendMode {
    Alpha,
    Additive,
    Opaque,
    Multiply,
    Screen,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveTextureFilter {
    Nearest,
    Linear,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiLiveScissor {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiLiveDraw {
    pub texture_id: u32,
    pub vertices: [FfiLiveVertex; 4],
    pub blend: FfiLiveBlendMode,
    pub texture_filter: FfiLiveTextureFilter,
    pub scissor: ROption<FfiLiveScissor>,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiLiveTextureCreate {
    pub texture_id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub format: FfiLiveTextureFormat,
    pub pixels: FfiOwnedByteBuffer,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiLiveTextureUpdate {
    pub texture_id: u32,
    pub generation: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub format: FfiLiveTextureFormat,
    pub pixels: FfiOwnedByteBuffer,
}

#[repr(u8)]
#[derive(Debug, StableAbi)]
pub enum FfiLiveSceneResourceOperation {
    Create(FfiLiveTextureCreate),
    Update(FfiLiveTextureUpdate),
    Destroy { texture_id: u32, generation: u64 },
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiLiveSceneTransaction {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub compositing: FfiLiveSceneCompositing,
    pub resources: RVec<FfiLiveSceneResourceOperation>,
    pub draws: RVec<FfiLiveDraw>,
    pub reset_resources: bool,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiLiveSceneCompositing {
    LinearSrgb,
    EncodedSrgb,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiLiveResourceTexture {
    pub texture_id: u32,
    pub resource_uri: RString,
    pub codec: RString,
    pub revision: u64,
    pub decoded_width: u32,
    pub decoded_height: u32,
    pub decoded_format: FfiLiveTextureFormat,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveResourceScene {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub textures: RVec<FfiLiveResourceTexture>,
    pub draws: RVec<FfiLiveDraw>,
}

#[repr(u8)]
#[derive(Debug, StableAbi)]
pub enum FfiLivePcmBuffer {
    I16(FfiOwnedI16Buffer),
    F32(FfiOwnedF32Buffer),
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiLiveAudioPacket {
    pub sequence: u64,
    pub stream_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: FfiLivePcmBuffer,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveAudioEncoding {
    Unknown,
    Wav,
    Ogg,
    Mp3,
    Flac,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveAudioSampleFormat {
    I16,
    F32,
}

#[repr(u8)]
#[derive(Debug, StableAbi)]
pub enum FfiLiveAudioCommand {
    LoadResource {
        sequence: u64,
        stream_id: u32,
        encoding: FfiLiveAudioEncoding,
        resource_uri: RString,
    },
    CreateStream {
        sequence: u64,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        sample_format: FfiLiveAudioSampleFormat,
    },
    SubmitI16 {
        sequence: u64,
        stream_id: u32,
        samples: FfiOwnedI16Buffer,
    },
    SubmitF32 {
        sequence: u64,
        stream_id: u32,
        samples: FfiOwnedF32Buffer,
    },
    Play {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    },
    Stop {
        sequence: u64,
        stream_id: u32,
        fade_ms: u32,
    },
    Pause {
        sequence: u64,
        stream_id: u32,
    },
    Resume {
        sequence: u64,
        stream_id: u32,
    },
    SetParams {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
    },
    DestroyStream {
        sequence: u64,
        stream_id: u32,
    },
    MasterVolume {
        sequence: u64,
        volume: f32,
    },
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, StableAbi)]
pub struct FfiLiveTextRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub line_height: f32,
    pub max_lines: u32,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveTextPresentation {
    pub sequence: u64,
    pub lease_id: RString,
    pub layout_id: RString,
    pub language: RString,
    pub font_families: RVec<RString>,
    pub body: FfiLiveTextRegion,
    pub speaker: ROption<FfiLiveTextRegion>,
    pub rgba: [u8; 4],
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveVideoMode {
    ModalWithAudio,
    LayerNoAudio,
}

#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiLiveVideoCommand {
    Play {
        sequence: u64,
        playback_id: RString,
        resource_uri: RString,
        mode: FfiLiveVideoMode,
        stage_width: u32,
        stage_height: u32,
    },
    Stop {
        sequence: u64,
        playback_id: RString,
    },
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveEvent {
    pub sequence: u64,
    pub event: RString,
    pub value: RString,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveTextLease {
    pub sequence: u64,
    pub lease_id: RString,
    pub byte_len: u32,
    pub source_ref: RString,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiLiveOutput {
    pub layers: RVec<crate::FfiLayerTransactionV9>,
    pub audio: RVec<FfiLiveAudioPacket>,
    pub audio_commands: RVec<FfiLiveAudioCommand>,
    pub video: RVec<FfiLiveVideoCommand>,
}

fn ffi_live_audio(value: LegacyAudioPacketV7) -> FfiLiveAudioPacket {
    FfiLiveAudioPacket {
        sequence: value.sequence,
        stream_id: value.stream_id,
        sample_rate: value.sample_rate,
        channels: value.channels,
        pcm: match value.pcm {
            LegacyPcmBufferV7::I16(samples) => FfiLivePcmBuffer::I16(samples.into_ffi()),
            LegacyPcmBufferV7::F32(samples) => FfiLivePcmBuffer::F32(samples.into_ffi()),
        },
    }
}

fn legacy_live_audio(value: FfiLiveAudioPacket) -> LegacyAudioPacketV7 {
    LegacyAudioPacketV7 {
        sequence: value.sequence,
        stream_id: value.stream_id,
        sample_rate: value.sample_rate,
        channels: value.channels,
        pcm: match value.pcm {
            FfiLivePcmBuffer::I16(samples) => LegacyPcmBufferV7::I16(samples.into_owned()),
            FfiLivePcmBuffer::F32(samples) => LegacyPcmBufferV7::F32(samples.into_owned()),
        },
    }
}

fn ffi_live_audio_command(sequence: u64, value: LegacyAudioCommandV1) -> FfiLiveAudioCommand {
    match value {
        LegacyAudioCommandV1::LoadResource {
            stream_id,
            encoding,
            resource_uri,
        } => FfiLiveAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding: match encoding {
                LegacyAudioEncoding::Unknown => FfiLiveAudioEncoding::Unknown,
                LegacyAudioEncoding::Wav => FfiLiveAudioEncoding::Wav,
                LegacyAudioEncoding::Ogg => FfiLiveAudioEncoding::Ogg,
                LegacyAudioEncoding::Mp3 => FfiLiveAudioEncoding::Mp3,
                LegacyAudioEncoding::Flac => FfiLiveAudioEncoding::Flac,
            },
            resource_uri: resource_uri.into(),
        },
        LegacyAudioCommandV1::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format,
        } => FfiLiveAudioCommand::CreateStream {
            sequence,
            stream_id,
            sample_rate,
            channels,
            sample_format: match sample_format {
                LegacyAudioSampleFormat::I16 => FfiLiveAudioSampleFormat::I16,
                LegacyAudioSampleFormat::F32 => FfiLiveAudioSampleFormat::F32,
            },
        },
        LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => FfiLiveAudioCommand::SubmitI16 {
            sequence,
            stream_id,
            samples: samples.into_ffi(),
        },
        LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => FfiLiveAudioCommand::SubmitF32 {
            sequence,
            stream_id,
            samples: samples.into_ffi(),
        },
        LegacyAudioCommandV1::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => FfiLiveAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        },
        LegacyAudioCommandV1::Stop { stream_id, fade_ms } => FfiLiveAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        },
        LegacyAudioCommandV1::Pause { stream_id } => FfiLiveAudioCommand::Pause {
            sequence,
            stream_id,
        },
        LegacyAudioCommandV1::Resume { stream_id } => FfiLiveAudioCommand::Resume {
            sequence,
            stream_id,
        },
        LegacyAudioCommandV1::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
        } => FfiLiveAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        },
        LegacyAudioCommandV1::DestroyStream { stream_id } => FfiLiveAudioCommand::DestroyStream {
            sequence,
            stream_id,
        },
        LegacyAudioCommandV1::MasterVolume { volume } => {
            FfiLiveAudioCommand::MasterVolume { sequence, volume }
        }
    }
}

fn legacy_live_audio_command(value: FfiLiveAudioCommand) -> LegacySequenced<LegacyAudioCommandV1> {
    let (sequence, value) = match value {
        FfiLiveAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding,
            resource_uri,
        } => (
            sequence,
            LegacyAudioCommandV1::LoadResource {
                stream_id,
                encoding: match encoding {
                    FfiLiveAudioEncoding::Unknown => LegacyAudioEncoding::Unknown,
                    FfiLiveAudioEncoding::Wav => LegacyAudioEncoding::Wav,
                    FfiLiveAudioEncoding::Ogg => LegacyAudioEncoding::Ogg,
                    FfiLiveAudioEncoding::Mp3 => LegacyAudioEncoding::Mp3,
                    FfiLiveAudioEncoding::Flac => LegacyAudioEncoding::Flac,
                },
                resource_uri: resource_uri.to_string(),
            },
        ),
        FfiLiveAudioCommand::CreateStream {
            sequence,
            stream_id,
            sample_rate,
            channels,
            sample_format,
        } => (
            sequence,
            LegacyAudioCommandV1::CreateStream {
                stream_id,
                sample_rate,
                channels,
                sample_format: match sample_format {
                    FfiLiveAudioSampleFormat::I16 => LegacyAudioSampleFormat::I16,
                    FfiLiveAudioSampleFormat::F32 => LegacyAudioSampleFormat::F32,
                },
            },
        ),
        FfiLiveAudioCommand::SubmitI16 {
            sequence,
            stream_id,
            samples,
        } => (
            sequence,
            LegacyAudioCommandV1::SubmitI16 {
                stream_id,
                samples: samples.into_owned(),
            },
        ),
        FfiLiveAudioCommand::SubmitF32 {
            sequence,
            stream_id,
            samples,
        } => (
            sequence,
            LegacyAudioCommandV1::SubmitF32 {
                stream_id,
                samples: samples.into_owned(),
            },
        ),
        FfiLiveAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => (
            sequence,
            LegacyAudioCommandV1::Play {
                stream_id,
                volume,
                pan,
                repeat,
                fade_in_ms,
            },
        ),
        FfiLiveAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        } => (sequence, LegacyAudioCommandV1::Stop { stream_id, fade_ms }),
        FfiLiveAudioCommand::Pause {
            sequence,
            stream_id,
        } => (sequence, LegacyAudioCommandV1::Pause { stream_id }),
        FfiLiveAudioCommand::Resume {
            sequence,
            stream_id,
        } => (sequence, LegacyAudioCommandV1::Resume { stream_id }),
        FfiLiveAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        } => (
            sequence,
            LegacyAudioCommandV1::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
            },
        ),
        FfiLiveAudioCommand::DestroyStream {
            sequence,
            stream_id,
        } => (sequence, LegacyAudioCommandV1::DestroyStream { stream_id }),
        FfiLiveAudioCommand::MasterVolume { sequence, volume } => {
            (sequence, LegacyAudioCommandV1::MasterVolume { volume })
        }
    };
    LegacySequenced { sequence, value }
}

fn ffi_live_output(value: LegacyLiveOutput) -> FfiLiveOutput {
    FfiLiveOutput {
        layers: value
            .layers
            .into_iter()
            .map(crate::v9::ffi_layer_transaction)
            .collect::<Vec<_>>()
            .into(),
        audio: value
            .audio
            .into_iter()
            .map(ffi_live_audio)
            .collect::<Vec<_>>()
            .into(),
        audio_commands: value
            .audio_commands
            .into_iter()
            .map(|command| ffi_live_audio_command(command.sequence, command.value))
            .collect::<Vec<_>>()
            .into(),
        video: value
            .video
            .into_iter()
            .map(|video| {
                let sequence = video.sequence;
                match video.value {
                    LegacyVideoCommandV1::Play {
                        playback_id,
                        resource_uri,
                        mode,
                        stage_width,
                        stage_height,
                    } => FfiLiveVideoCommand::Play {
                        sequence,
                        playback_id: playback_id.into(),
                        resource_uri: resource_uri.into(),
                        mode: match mode {
                            LegacyVideoMode::ModalWithAudio => FfiLiveVideoMode::ModalWithAudio,
                            LegacyVideoMode::LayerNoAudio => FfiLiveVideoMode::LayerNoAudio,
                        },
                        stage_width,
                        stage_height,
                    },
                    LegacyVideoCommandV1::Stop { playback_id } => FfiLiveVideoCommand::Stop {
                        sequence,
                        playback_id: playback_id.into(),
                    },
                }
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

fn legacy_live_output(value: FfiLiveOutput) -> LegacyLiveOutput {
    LegacyLiveOutput {
        layers: value
            .layers
            .into_iter()
            .map(crate::v9::legacy_layer_transaction)
            .collect(),
        audio: value.audio.into_iter().map(legacy_live_audio).collect(),
        audio_commands: value
            .audio_commands
            .into_iter()
            .map(legacy_live_audio_command)
            .collect(),
        video: value
            .video
            .into_iter()
            .map(|video| match video {
                FfiLiveVideoCommand::Play {
                    sequence,
                    playback_id,
                    resource_uri,
                    mode,
                    stage_width,
                    stage_height,
                } => LegacySequenced {
                    sequence,
                    value: LegacyVideoCommandV1::Play {
                        playback_id: playback_id.to_string(),
                        resource_uri: resource_uri.to_string(),
                        mode: match mode {
                            FfiLiveVideoMode::ModalWithAudio => LegacyVideoMode::ModalWithAudio,
                            FfiLiveVideoMode::LayerNoAudio => LegacyVideoMode::LayerNoAudio,
                        },
                        stage_width,
                        stage_height,
                    },
                },
                FfiLiveVideoCommand::Stop {
                    sequence,
                    playback_id,
                } => LegacySequenced {
                    sequence,
                    value: LegacyVideoCommandV1::Stop {
                        playback_id: playback_id.to_string(),
                    },
                },
            })
            .collect(),
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiBlackboardMutation {
    pub sequence: u64,
    pub key: RString,
    pub value: RString,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiDirtySection {
    pub sequence: u64,
    pub section_id: RString,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiControlTransaction {
    pub events: RVec<FfiLiveEvent>,
    pub blackboard: RVec<FfiBlackboardMutation>,
    pub dirty_sections: RVec<FfiDirtySection>,
    pub waits: RVec<FfiWaitRequest>,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiStepOutput {
    pub status: FfiRuntimeStatus,
    pub live: FfiLiveOutput,
    pub control: FfiControlTransaction,
    pub trace: RVec<FfiTraceEntry>,
    pub diagnostics: RVec<FfiDiagnostic>,
    pub coverage: FfiCoverageDelta,
    pub state_revision: u64,
}

impl TryFrom<LegacyStepOutput> for FfiStepOutput {
    type Error = LegacyProviderError;

    fn try_from(value: LegacyStepOutput) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status.into(),
            live: ffi_live_output(value.live),
            control: FfiControlTransaction {
                events: value
                    .control
                    .events
                    .into_iter()
                    .map(|event| FfiLiveEvent {
                        sequence: event.sequence,
                        event: event.event.into(),
                        value: event.value.into(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                blackboard: value
                    .control
                    .blackboard
                    .into_iter()
                    .map(|mutation| FfiBlackboardMutation {
                        sequence: mutation.sequence,
                        key: mutation.key.into(),
                        value: mutation.value.into(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                dirty_sections: value
                    .control
                    .dirty_sections
                    .into_iter()
                    .map(|dirty| FfiDirtySection {
                        sequence: dirty.sequence,
                        section_id: dirty.section_id.into(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                waits: value
                    .control
                    .waits
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>()
                    .into(),
            },
            trace: value
                .trace
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            diagnostics: value
                .diagnostics
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            coverage: value.coverage.into(),
            state_revision: value.state_revision,
        })
    }
}

impl TryFrom<FfiStepOutput> for LegacyStepOutput {
    type Error = LegacyProviderError;
    fn try_from(value: FfiStepOutput) -> Result<Self, Self::Error> {
        let FfiStepOutput {
            status,
            live,
            control,
            trace,
            diagnostics,
            coverage,
            state_revision,
        } = value;
        Ok(Self {
            status: status.into(),
            live: legacy_live_output(live),
            control: LegacyControlTransaction {
                events: control
                    .events
                    .into_iter()
                    .map(|event| LegacyEvent {
                        sequence: event.sequence,
                        event: event.event.to_string(),
                        value: event.value.to_string(),
                    })
                    .collect(),
                blackboard: control
                    .blackboard
                    .into_iter()
                    .map(|mutation| LegacyBlackboardMutation {
                        sequence: mutation.sequence,
                        key: mutation.key.to_string(),
                        value: mutation.value.to_string(),
                    })
                    .collect(),
                dirty_sections: control
                    .dirty_sections
                    .into_iter()
                    .map(|dirty| LegacyDirtySection {
                        sequence: dirty.sequence,
                        section_id: dirty.section_id.to_string(),
                    })
                    .collect(),
                waits: control
                    .waits
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?,
            },
            trace: trace.into_iter().map(Into::into).collect(),
            diagnostics: diagnostics.into_iter().map(Into::into).collect(),
            coverage: coverage.into(),
            state_revision,
        })
    }
}

#[cfg(test)]
mod live_zero_copy_tests {
    use super::*;

    #[test]
    fn pcm_i16_allocation_moves_across_family_ffi_wire() {
        let samples = vec![-3_i16, 0, 17, 4096];
        let source_ptr = samples.as_ptr();
        let packet = LegacyAudioPacketV7 {
            sequence: 9,
            stream_id: 2,
            sample_rate: 48_000,
            channels: 2,
            pcm: LegacyPcmBufferV7::I16(samples.into()),
        };

        let ffi = ffi_live_audio(packet);
        let ffi_ptr = match &ffi.pcm {
            FfiLivePcmBuffer::I16(samples) => samples.as_slice().as_ptr(),
            FfiLivePcmBuffer::F32(_) => panic!("expected i16 PCM"),
        };
        assert_eq!(ffi_ptr, source_ptr);

        let legacy = legacy_live_audio(ffi);
        let returned_ptr = match legacy.pcm {
            LegacyPcmBufferV7::I16(samples) => samples.as_ptr(),
            LegacyPcmBufferV7::F32(_) => panic!("expected i16 PCM"),
        };
        assert_eq!(returned_ptr, source_ptr);
    }

    #[test]
    fn pcm_submit_command_moves_across_family_ffi_wire() {
        let samples = vec![-0.25_f32, 0.0, 0.25, 1.0];
        let source_ptr = samples.as_ptr();
        let ffi = ffi_live_audio_command(
            12,
            LegacyAudioCommandV1::SubmitF32 {
                stream_id: 5,
                samples: samples.into(),
            },
        );
        let ffi_ptr = match &ffi {
            FfiLiveAudioCommand::SubmitF32 { samples, .. } => samples.as_ptr(),
            _ => panic!("expected f32 submit"),
        };
        assert_eq!(ffi_ptr, source_ptr);

        let command = legacy_live_audio_command(ffi);
        match command.value {
            LegacyAudioCommandV1::SubmitF32 { samples, .. } => {
                assert_eq!(samples.as_ptr(), source_ptr);
            }
            _ => panic!("expected f32 submit"),
        }
    }

    #[test]
    fn vfs_range_allocation_moves_across_family_ffi_wire() {
        let bytes = vec![1_u8, 2, 3, 4, 5, 6];
        let source_ptr = bytes.as_ptr();
        let range = astra_byte_source::RangeReadResult {
            range: astra_byte_source::ByteRange {
                offset: 11,
                len: bytes.len() as u64,
            },
            revision: astra_byte_source::SourceRevision(7),
            bytes: bytes.into(),
        };

        let ffi = FfiRangeReadResult::from(range);
        assert_eq!(ffi.bytes.as_ptr(), source_ptr);
        let returned = astra_byte_source::RangeReadResult::from(ffi);
        assert_eq!(returned.bytes.as_ptr(), source_ptr);
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSnapshotSection {
    pub section_id: RString,
    pub schema: RString,
    pub version: FfiSchemaVersion,
    pub bytes: FfiOwnedBytes,
}

impl From<LegacySnapshotSection> for FfiSnapshotSection {
    fn from(value: LegacySnapshotSection) -> Self {
        Self {
            section_id: value.section_id.into(),
            schema: value.schema.into(),
            version: value.version.into(),
            bytes: FfiOwnedBytes::new(value.bytes),
        }
    }
}
impl From<FfiSnapshotSection> for LegacySnapshotSection {
    fn from(value: FfiSnapshotSection) -> Self {
        Self {
            section_id: value.section_id.to_string(),
            schema: value.schema.to_string(),
            version: value.version.into(),
            bytes: value.bytes.into_bytes(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSnapshotEnvelope {
    pub family_id: RString,
    pub session_id: RString,
    pub schema_version: FfiSchemaVersion,
    pub case_fingerprint: FfiHash256,
    pub fixed_step: u64,
    pub session_seed: u64,
    pub runtime_cursor: u64,
    pub family_sections: RVec<FfiSnapshotSection>,
    pub redaction_status: RString,
}

impl From<LegacySnapshotEnvelope> for FfiSnapshotEnvelope {
    fn from(value: LegacySnapshotEnvelope) -> Self {
        Self {
            family_id: value.family_id.0.into(),
            session_id: value.session_id.0.into(),
            schema_version: value.schema_version.into(),
            case_fingerprint: value.case_fingerprint.into(),
            fixed_step: value.fixed_step,
            session_seed: value.session_seed,
            runtime_cursor: value.runtime_cursor,
            family_sections: value
                .family_sections
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            redaction_status: value.redaction_status.into(),
        }
    }
}
impl From<FfiSnapshotEnvelope> for LegacySnapshotEnvelope {
    fn from(value: FfiSnapshotEnvelope) -> Self {
        Self {
            family_id: FamilyId(value.family_id.to_string()),
            session_id: LegacyRuntimeSessionId(value.session_id.to_string()),
            schema_version: value.schema_version.into(),
            case_fingerprint: value.case_fingerprint.into(),
            fixed_step: value.fixed_step,
            session_seed: value.session_seed,
            runtime_cursor: value.runtime_cursor,
            family_sections: value
                .family_sections
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            redaction_status: value.redaction_status.to_string(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiRestoreReport {
    pub restored_fixed_step: u64,
    pub session_seed: u64,
    pub state_revision: u64,
    pub diagnostics: RVec<FfiDiagnostic>,
}
impl From<LegacyRestoreReport> for FfiRestoreReport {
    fn from(value: LegacyRestoreReport) -> Self {
        Self {
            restored_fixed_step: value.restored_fixed_step,
            session_seed: value.session_seed,
            state_revision: value.state_revision,
            diagnostics: value
                .diagnostics
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}
impl From<FfiRestoreReport> for LegacyRestoreReport {
    fn from(value: FfiRestoreReport) -> Self {
        Self {
            restored_fixed_step: value.restored_fixed_step,
            session_seed: value.session_seed,
            state_revision: value.state_revision,
            diagnostics: value.diagnostics.iter().cloned().map(Into::into).collect(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiShutdownReport {
    pub final_state_revision: u64,
    pub instruction_count: u64,
    pub syscall_count: u64,
    pub evidence_vm_trace: RVec<FfiVmTraceRecord>,
    pub diagnostics: RVec<FfiDiagnostic>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiVmTraceRecord {
    pub context_id: u32,
    pub program_counter: u32,
    pub opcode: u8,
}

impl From<LegacyVmTraceRecord> for FfiVmTraceRecord {
    fn from(value: LegacyVmTraceRecord) -> Self {
        Self {
            context_id: value.context_id,
            program_counter: value.program_counter,
            opcode: value.opcode,
        }
    }
}

impl From<FfiVmTraceRecord> for LegacyVmTraceRecord {
    fn from(value: FfiVmTraceRecord) -> Self {
        Self {
            context_id: value.context_id,
            program_counter: value.program_counter,
            opcode: value.opcode,
        }
    }
}

impl From<LegacyShutdownReport> for FfiShutdownReport {
    fn from(value: LegacyShutdownReport) -> Self {
        Self {
            final_state_revision: value.final_state_revision,
            instruction_count: value.instruction_count,
            syscall_count: value.syscall_count,
            evidence_vm_trace: value
                .evidence_vm_trace
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            diagnostics: value
                .diagnostics
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}
impl From<FfiShutdownReport> for LegacyShutdownReport {
    fn from(value: FfiShutdownReport) -> Self {
        Self {
            final_state_revision: value.final_state_revision,
            instruction_count: value.instruction_count,
            syscall_count: value.syscall_count,
            evidence_vm_trace: value
                .evidence_vm_trace
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>(),
            diagnostics: value.diagnostics.iter().cloned().map(Into::into).collect(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiEphemeralText {
    pub lease_id: RString,
    pub text: RString,
    pub speaker: ROption<RString>,
}
impl From<LegacyEphemeralText> for FfiEphemeralText {
    fn from(value: LegacyEphemeralText) -> Self {
        Self {
            lease_id: value.lease_id.into(),
            text: value.text.into(),
            speaker: value.speaker.map(Into::into).into(),
        }
    }
}
impl From<FfiEphemeralText> for LegacyEphemeralText {
    fn from(value: FfiEphemeralText) -> Self {
        Self {
            lease_id: value.lease_id.to_string(),
            text: value.text.to_string(),
            speaker: value.speaker.into_option().map(|v| v.to_string()),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiByteRange {
    pub offset: u64,
    pub len: u64,
}
impl From<astra_byte_source::ByteRange> for FfiByteRange {
    fn from(value: astra_byte_source::ByteRange) -> Self {
        Self {
            offset: value.offset,
            len: value.len,
        }
    }
}
impl From<FfiByteRange> for astra_byte_source::ByteRange {
    fn from(value: FfiByteRange) -> Self {
        Self {
            offset: value.offset,
            len: value.len,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiByteSourceStat {
    pub len: u64,
    pub revision: u64,
}
impl From<astra_byte_source::ByteSourceStat> for FfiByteSourceStat {
    fn from(value: astra_byte_source::ByteSourceStat) -> Self {
        Self {
            len: value.len,
            revision: value.revision.0,
        }
    }
}
impl From<FfiByteSourceStat> for astra_byte_source::ByteSourceStat {
    fn from(value: FfiByteSourceStat) -> Self {
        Self {
            len: value.len,
            revision: astra_byte_source::SourceRevision(value.revision),
        }
    }
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiRangeReadResult {
    pub range: FfiByteRange,
    pub revision: u64,
    pub bytes: FfiOwnedByteBuffer,
}
impl From<astra_byte_source::RangeReadResult> for FfiRangeReadResult {
    fn from(value: astra_byte_source::RangeReadResult) -> Self {
        Self {
            range: value.range.into(),
            revision: value.revision.0,
            bytes: value.bytes.into_ffi(),
        }
    }
}
impl From<FfiRangeReadResult> for astra_byte_source::RangeReadResult {
    fn from(value: FfiRangeReadResult) -> Self {
        Self {
            range: value.range.into(),
            revision: astra_byte_source::SourceRevision(value.revision),
            bytes: value.bytes.into_owned(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiVfsListedFile {
    pub uri: RString,
    pub stat: FfiByteSourceStat,
}
impl From<LegacyVfsListedFile> for FfiVfsListedFile {
    fn from(value: LegacyVfsListedFile) -> Self {
        Self {
            uri: value.uri.into(),
            stat: value.stat.into(),
        }
    }
}
impl From<FfiVfsListedFile> for LegacyVfsListedFile {
    fn from(value: FfiVfsListedFile) -> Self {
        Self {
            uri: value.uri.to_string(),
            stat: value.stat.into(),
        }
    }
}
