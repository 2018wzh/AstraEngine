use std::collections::BTreeMap;

use abi_stable::{
    std_types::{RArc, ROption, RString, RVec},
    StableAbi,
};
use astra_core::{Hash256, SchemaVersion};

use crate::{
    FamilyId, LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7,
    LegacyAudioSampleFormat, LegacyAwaitResult, LegacyBlackboardMutation, LegacyBlendMode,
    LegacyControlTransaction, LegacyCoverageDelta, LegacyDiagnostic, LegacyDirtySection,
    LegacyDrawV1, LegacyEphemeralText, LegacyEvent, LegacyFamilyPluginDescriptor, LegacyInputEdge,
    LegacyLiveOutput, LegacyOpenRequest, LegacyPayload, LegacyPcmBufferV7, LegacyProbeReport,
    LegacyProbeRequest, LegacyProviderError, LegacyProviderResult, LegacyRenderResourceFrameV1,
    LegacyReplayMode, LegacyRestoreReport, LegacyRuntimeHostCtx, LegacyRuntimeSessionId,
    LegacyRuntimeStatus, LegacySceneResourceOperationV7, LegacySceneTransactionV7,
    LegacyScheduledEvent, LegacyScissorV1, LegacySequenced, LegacyShutdownReport,
    LegacySnapshotEnvelope, LegacySnapshotSection, LegacyStepBudget, LegacyStepInput,
    LegacyStepOutput, LegacyTextLease, LegacyTextPresentationLeaseV1, LegacyTextureFormat,
    LegacyTraceEntry, LegacyVertexV1, LegacyVfsListedFile, LegacyVideoCommandV1, LegacyVideoMode,
    LegacyWaitRequest,
};

pub type FfiBulkBytes = RArc<RVec<u8>>;

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
    pub bytes: FfiBulkBytes,
}

impl FfiOwnedBytes {
    pub fn empty() -> Self {
        Self {
            bytes: RArc::new(RVec::new()),
        }
    }

    pub fn new(bytes: FfiBulkBytes) -> Self {
        Self { bytes }
    }

    pub fn into_bytes(self) -> FfiBulkBytes {
        self.bytes
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

pub fn bulk_bytes_from_vec(bytes: Vec<u8>) -> FfiBulkBytes {
    RArc::new(bytes.into())
}

pub fn bulk_bytes_to_vec(bytes: &FfiBulkBytes) -> Vec<u8> {
    bytes.as_slice().to_vec()
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiFamilyPluginDescriptor {
    pub family_id: RString,
    pub plugin_id: RString,
    pub provider_id: RString,
    pub engine_version: RString,
    pub rustc_fingerprint: RString,
    pub feature_fingerprint: RString,
    pub abi_fingerprint: RString,
    pub supported_formats: RVec<RString>,
    pub permissions: RVec<RString>,
    pub report_redaction: RString,
    pub license: RString,
}

impl From<LegacyFamilyPluginDescriptor> for FfiFamilyPluginDescriptor {
    fn from(value: LegacyFamilyPluginDescriptor) -> Self {
        Self {
            family_id: value.family_id.0.into(),
            plugin_id: value.plugin_id.into(),
            provider_id: value.provider_id.into(),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiStepBudget {
    pub max_instructions: u32,
    pub max_effects: u32,
    pub max_trace_entries: u32,
}

impl From<LegacyStepBudget> for FfiStepBudget {
    fn from(value: LegacyStepBudget) -> Self {
        Self {
            max_instructions: value.max_instructions,
            max_effects: value.max_effects,
            max_trace_entries: value.max_trace_entries,
        }
    }
}

impl From<FfiStepBudget> for LegacyStepBudget {
    fn from(value: FfiStepBudget) -> Self {
        Self {
            max_instructions: value.max_instructions,
            max_effects: value.max_effects,
            max_trace_entries: value.max_trace_entries,
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
    pub budget: FfiStepBudget,
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
            budget: value.budget.into(),
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
            budget: value.budget.into(),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiWaitKind {
    Frame,
    Time,
    Input,
    MediaFence,
    PresentationFence,
    ProviderCompletion,
    FamilyOpaque,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiWaitRequest {
    pub kind: FfiWaitKind,
    pub token_id: RString,
    pub name: RString,
    pub keys: RVec<RString>,
    pub number: u32,
    pub payload_len: u64,
}

impl From<LegacyWaitRequest> for FfiWaitRequest {
    fn from(value: LegacyWaitRequest) -> Self {
        let base = |kind, token_id: String| Self {
            kind,
            token_id: token_id.into(),
            name: RString::new(),
            keys: RVec::new(),
            number: 0,
            payload_len: 0,
        };
        match value {
            LegacyWaitRequest::Frame { token_id, frames } => Self {
                number: frames,
                ..base(FfiWaitKind::Frame, token_id)
            },
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => Self {
                number: milliseconds,
                ..base(FfiWaitKind::Time, token_id)
            },
            LegacyWaitRequest::Input { token_id, keys } => Self {
                keys: strings_to_ffi(keys),
                ..base(FfiWaitKind::Input, token_id)
            },
            LegacyWaitRequest::MediaFence { token_id, media_id } => Self {
                name: media_id.into(),
                ..base(FfiWaitKind::MediaFence, token_id)
            },
            LegacyWaitRequest::PresentationFence { token_id, fence_id } => Self {
                name: fence_id.into(),
                ..base(FfiWaitKind::PresentationFence, token_id)
            },
            LegacyWaitRequest::ProviderCompletion {
                token_id,
                request_id,
            } => Self {
                name: request_id.into(),
                ..base(FfiWaitKind::ProviderCompletion, token_id)
            },
            LegacyWaitRequest::FamilyOpaque {
                token_id,
                wait_kind,
                payload_len,
            } => Self {
                name: wait_kind.into(),
                payload_len,
                ..base(FfiWaitKind::FamilyOpaque, token_id)
            },
        }
    }
}

impl TryFrom<FfiWaitRequest> for LegacyWaitRequest {
    type Error = LegacyProviderError;
    fn try_from(value: FfiWaitRequest) -> Result<Self, Self::Error> {
        let token_id = value.token_id.to_string();
        Ok(match value.kind {
            FfiWaitKind::Frame => Self::Frame {
                token_id,
                frames: value.number,
            },
            FfiWaitKind::Time => Self::Time {
                token_id,
                milliseconds: value.number,
            },
            FfiWaitKind::Input => Self::Input {
                token_id,
                keys: strings_from_ffi(value.keys),
            },
            FfiWaitKind::MediaFence => Self::MediaFence {
                token_id,
                media_id: value.name.to_string(),
            },
            FfiWaitKind::PresentationFence => Self::PresentationFence {
                token_id,
                fence_id: value.name.to_string(),
            },
            FfiWaitKind::ProviderCompletion => Self::ProviderCompletion {
                token_id,
                request_id: value.name.to_string(),
            },
            FfiWaitKind::FamilyOpaque => Self::FamilyOpaque {
                token_id,
                wait_kind: value.name.to_string(),
                payload_len: value.payload_len,
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
    pub scissor: ROption<FfiLiveScissor>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveTextureCreate {
    pub texture_id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub format: FfiLiveTextureFormat,
    pub pixels: RVec<u8>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveTextureUpdate {
    pub texture_id: u32,
    pub generation: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub format: FfiLiveTextureFormat,
    pub pixels: RVec<u8>,
}

#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiLiveSceneResourceOperation {
    Create(FfiLiveTextureCreate),
    Update(FfiLiveTextureUpdate),
    Destroy { texture_id: u32, generation: u64 },
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveSceneTransaction {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub resources: RVec<FfiLiveSceneResourceOperation>,
    pub draws: RVec<FfiLiveDraw>,
    pub reset_resources: bool,
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
#[derive(Debug, Clone, StableAbi)]
pub enum FfiLivePcmBuffer {
    I16(RVec<i16>),
    F32(RVec<f32>),
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveAudioPacket {
    pub sequence: u64,
    pub stream_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: FfiLivePcmBuffer,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveAudioCommandKind {
    LoadResource,
    CreateStream,
    SubmitI16,
    SubmitF32,
    Play,
    Stop,
    Pause,
    Resume,
    SetParams,
    DestroyStream,
    MasterVolume,
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

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveAudioCommand {
    pub sequence: u64,
    pub kind: FfiLiveAudioCommandKind,
    pub stream_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub encoding: FfiLiveAudioEncoding,
    pub sample_format: FfiLiveAudioSampleFormat,
    pub resource_uri: RString,
    pub samples: FfiLivePcmBuffer,
    pub volume: f32,
    pub pan: f32,
    pub repeat: bool,
    pub fade_ms: u32,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLiveVideoCommandKind {
    Play,
    Stop,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveVideoCommand {
    pub sequence: u64,
    pub playback_id: RString,
    pub resource_uri: RString,
    pub mode: FfiLiveVideoMode,
    pub stage_width: u32,
    pub stage_height: u32,
    pub kind: FfiLiveVideoCommandKind,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveEvent {
    pub sequence: u64,
    pub event: RString,
    pub payload: RVec<u8>,
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
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLiveOutput {
    pub scenes: RVec<FfiLiveSceneTransaction>,
    pub resource_scenes: RVec<FfiLiveResourceScene>,
    pub audio: RVec<FfiLiveAudioPacket>,
    pub audio_commands: RVec<FfiLiveAudioCommand>,
    pub text: RVec<FfiLiveTextLease>,
    pub text_presentations: RVec<FfiLiveTextPresentation>,
    pub video: RVec<FfiLiveVideoCommand>,
}

fn ffi_live_format(value: LegacyTextureFormat) -> FfiLiveTextureFormat {
    match value {
        LegacyTextureFormat::Rgba8 => FfiLiveTextureFormat::Rgba8,
        LegacyTextureFormat::LumaAlpha8 => FfiLiveTextureFormat::LumaAlpha8,
    }
}

fn legacy_live_format(value: FfiLiveTextureFormat) -> LegacyTextureFormat {
    match value {
        FfiLiveTextureFormat::Rgba8 => LegacyTextureFormat::Rgba8,
        FfiLiveTextureFormat::LumaAlpha8 => LegacyTextureFormat::LumaAlpha8,
    }
}

fn ffi_live_draw(value: LegacyDrawV1) -> FfiLiveDraw {
    FfiLiveDraw {
        texture_id: value.texture_id,
        vertices: value.vertices.map(|vertex| FfiLiveVertex {
            position: vertex.position,
            tex_coord: vertex.tex_coord,
            color: vertex.color,
        }),
        blend: match value.blend {
            LegacyBlendMode::Alpha => FfiLiveBlendMode::Alpha,
            LegacyBlendMode::Add => FfiLiveBlendMode::Additive,
            LegacyBlendMode::Opaque => FfiLiveBlendMode::Opaque,
            LegacyBlendMode::Multiply => FfiLiveBlendMode::Multiply,
            LegacyBlendMode::Screen => FfiLiveBlendMode::Screen,
        },
        scissor: value
            .scissor
            .map(|value| FfiLiveScissor {
                x: value.x,
                y: value.y,
                width: value.width,
                height: value.height,
            })
            .into(),
    }
}

fn legacy_live_draw(value: FfiLiveDraw) -> LegacyDrawV1 {
    LegacyDrawV1 {
        texture_id: value.texture_id,
        vertices: value.vertices.map(|vertex| LegacyVertexV1 {
            position: vertex.position,
            tex_coord: vertex.tex_coord,
            color: vertex.color,
        }),
        blend: match value.blend {
            FfiLiveBlendMode::Alpha => LegacyBlendMode::Alpha,
            FfiLiveBlendMode::Additive => LegacyBlendMode::Add,
            FfiLiveBlendMode::Opaque => LegacyBlendMode::Opaque,
            FfiLiveBlendMode::Multiply => LegacyBlendMode::Multiply,
            FfiLiveBlendMode::Screen => LegacyBlendMode::Screen,
        },
        scissor: value.scissor.into_option().map(|value| LegacyScissorV1 {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }),
    }
}

fn live_payload_to_ffi(value: LegacyPayload) -> RVec<u8> {
    match value {
        LegacyPayload::Native(bytes) => bytes.into(),
        LegacyPayload::Foreign(_) => {
            panic!("ASTRA_EMU_FFI_LIVE_FOREIGN_PAYLOAD_REJECTED")
        }
    }
}

fn live_payload_from_ffi(value: RVec<u8>) -> LegacyPayload {
    LegacyPayload::Native(value.into_vec())
}

fn ffi_live_scene(value: LegacySceneTransactionV7) -> FfiLiveSceneTransaction {
    FfiLiveSceneTransaction {
        sequence: value.sequence,
        width: value.width,
        height: value.height,
        resources: value
            .resources
            .into_iter()
            .map(|operation| match operation {
                LegacySceneResourceOperationV7::CreateTexture {
                    texture_id,
                    generation,
                    width,
                    height,
                    format,
                    pixels,
                } => FfiLiveSceneResourceOperation::Create(FfiLiveTextureCreate {
                    texture_id,
                    generation,
                    width,
                    height,
                    format: ffi_live_format(format),
                    pixels: live_payload_to_ffi(pixels),
                }),
                LegacySceneResourceOperationV7::UpdateTexture {
                    texture_id,
                    generation,
                    x,
                    y,
                    width,
                    height,
                    format,
                    pixels,
                } => FfiLiveSceneResourceOperation::Update(FfiLiveTextureUpdate {
                    texture_id,
                    generation,
                    x,
                    y,
                    width,
                    height,
                    format: ffi_live_format(format),
                    pixels: live_payload_to_ffi(pixels),
                }),
                LegacySceneResourceOperationV7::DestroyTexture {
                    texture_id,
                    generation,
                } => FfiLiveSceneResourceOperation::Destroy {
                    texture_id,
                    generation,
                },
            })
            .collect::<Vec<_>>()
            .into(),
        draws: value
            .draws
            .into_iter()
            .map(ffi_live_draw)
            .collect::<Vec<_>>()
            .into(),
        reset_resources: value.reset_resources,
    }
}

fn legacy_live_scene(value: FfiLiveSceneTransaction) -> LegacySceneTransactionV7 {
    LegacySceneTransactionV7 {
        sequence: value.sequence,
        width: value.width,
        height: value.height,
        resources: value
            .resources
            .into_iter()
            .map(|operation| match operation {
                FfiLiveSceneResourceOperation::Create(value) => {
                    LegacySceneResourceOperationV7::CreateTexture {
                        texture_id: value.texture_id,
                        generation: value.generation,
                        width: value.width,
                        height: value.height,
                        format: legacy_live_format(value.format),
                        pixels: live_payload_from_ffi(value.pixels),
                    }
                }
                FfiLiveSceneResourceOperation::Update(value) => {
                    LegacySceneResourceOperationV7::UpdateTexture {
                        texture_id: value.texture_id,
                        generation: value.generation,
                        x: value.x,
                        y: value.y,
                        width: value.width,
                        height: value.height,
                        format: legacy_live_format(value.format),
                        pixels: live_payload_from_ffi(value.pixels),
                    }
                }
                FfiLiveSceneResourceOperation::Destroy {
                    texture_id,
                    generation,
                } => LegacySceneResourceOperationV7::DestroyTexture {
                    texture_id,
                    generation,
                },
            })
            .collect(),
        draws: value.draws.into_iter().map(legacy_live_draw).collect(),
        reset_resources: value.reset_resources,
    }
}

fn ffi_live_audio(value: LegacyAudioPacketV7) -> FfiLiveAudioPacket {
    FfiLiveAudioPacket {
        sequence: value.sequence,
        stream_id: value.stream_id,
        sample_rate: value.sample_rate,
        channels: value.channels,
        pcm: match value.pcm {
            LegacyPcmBufferV7::I16(samples) => FfiLivePcmBuffer::I16(samples.into()),
            LegacyPcmBufferV7::F32(samples) => FfiLivePcmBuffer::F32(samples.into()),
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
            FfiLivePcmBuffer::I16(samples) => LegacyPcmBufferV7::I16(samples.into_vec()),
            FfiLivePcmBuffer::F32(samples) => LegacyPcmBufferV7::F32(samples.into_vec()),
        },
    }
}

fn ffi_live_audio_command(sequence: u64, value: LegacyAudioCommandV1) -> FfiLiveAudioCommand {
    let (
        kind,
        stream_id,
        sample_rate,
        channels,
        encoding,
        sample_format,
        resource_uri,
        samples,
        volume,
        pan,
        repeat,
        fade_ms,
    ) = match value {
        LegacyAudioCommandV1::LoadResource {
            stream_id,
            encoding,
            resource_uri,
        } => (
            FfiLiveAudioCommandKind::LoadResource,
            stream_id,
            0,
            0,
            match encoding {
                LegacyAudioEncoding::Unknown => FfiLiveAudioEncoding::Unknown,
                LegacyAudioEncoding::Wav => FfiLiveAudioEncoding::Wav,
                LegacyAudioEncoding::Ogg => FfiLiveAudioEncoding::Ogg,
                LegacyAudioEncoding::Mp3 => FfiLiveAudioEncoding::Mp3,
                LegacyAudioEncoding::Flac => FfiLiveAudioEncoding::Flac,
            },
            FfiLiveAudioSampleFormat::I16,
            resource_uri,
            FfiLivePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format,
        } => (
            FfiLiveAudioCommandKind::CreateStream,
            stream_id,
            sample_rate,
            channels,
            FfiLiveAudioEncoding::Unknown,
            match sample_format {
                LegacyAudioSampleFormat::I16 => FfiLiveAudioSampleFormat::I16,
                LegacyAudioSampleFormat::F32 => FfiLiveAudioSampleFormat::F32,
            },
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => (
            FfiLiveAudioCommandKind::SubmitI16,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(samples.into()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => (
            FfiLiveAudioCommandKind::SubmitF32,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::F32,
            String::new(),
            FfiLivePcmBuffer::F32(samples.into()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => (
            FfiLiveAudioCommandKind::Play,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            volume,
            pan,
            repeat,
            fade_in_ms,
        ),
        LegacyAudioCommandV1::Stop { stream_id, fade_ms } => (
            FfiLiveAudioCommandKind::Stop,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            fade_ms,
        ),
        LegacyAudioCommandV1::Pause { stream_id } => (
            FfiLiveAudioCommandKind::Pause,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::Resume { stream_id } => (
            FfiLiveAudioCommandKind::Resume,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
        } => (
            FfiLiveAudioCommandKind::SetParams,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            volume,
            pan,
            repeat,
            0,
        ),
        LegacyAudioCommandV1::DestroyStream { stream_id } => (
            FfiLiveAudioCommandKind::DestroyStream,
            stream_id,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        LegacyAudioCommandV1::MasterVolume { volume } => (
            FfiLiveAudioCommandKind::MasterVolume,
            0,
            0,
            0,
            FfiLiveAudioEncoding::Unknown,
            FfiLiveAudioSampleFormat::I16,
            String::new(),
            FfiLivePcmBuffer::I16(RVec::new()),
            volume,
            0.0,
            false,
            0,
        ),
    };
    FfiLiveAudioCommand {
        sequence,
        kind,
        stream_id,
        sample_rate,
        channels,
        encoding,
        sample_format,
        resource_uri: resource_uri.into(),
        samples,
        volume,
        pan,
        repeat,
        fade_ms,
    }
}

fn legacy_live_audio_command(value: FfiLiveAudioCommand) -> LegacySequenced<LegacyAudioCommandV1> {
    let FfiLiveAudioCommand {
        sequence,
        kind,
        stream_id,
        sample_rate,
        channels,
        encoding,
        sample_format,
        resource_uri,
        samples,
        volume,
        pan,
        repeat,
        fade_ms,
    } = value;
    let encoding = match encoding {
        FfiLiveAudioEncoding::Unknown => LegacyAudioEncoding::Unknown,
        FfiLiveAudioEncoding::Wav => LegacyAudioEncoding::Wav,
        FfiLiveAudioEncoding::Ogg => LegacyAudioEncoding::Ogg,
        FfiLiveAudioEncoding::Mp3 => LegacyAudioEncoding::Mp3,
        FfiLiveAudioEncoding::Flac => LegacyAudioEncoding::Flac,
    };
    let sample_format = match sample_format {
        FfiLiveAudioSampleFormat::I16 => LegacyAudioSampleFormat::I16,
        FfiLiveAudioSampleFormat::F32 => LegacyAudioSampleFormat::F32,
    };
    let (i16_samples, f32_samples) = match (kind, samples) {
        (FfiLiveAudioCommandKind::SubmitI16, FfiLivePcmBuffer::I16(samples)) => {
            (Some(samples.into_vec()), None)
        }
        (FfiLiveAudioCommandKind::SubmitF32, FfiLivePcmBuffer::F32(samples)) => {
            (None, Some(samples.into_vec()))
        }
        (FfiLiveAudioCommandKind::SubmitI16, FfiLivePcmBuffer::F32(_))
        | (FfiLiveAudioCommandKind::SubmitF32, FfiLivePcmBuffer::I16(_)) => {
            panic!("ASTRA_EMU_FFI_LIVE_AUDIO_FORMAT_MISMATCH")
        }
        (_, FfiLivePcmBuffer::I16(values)) if !values.is_empty() => {
            panic!("ASTRA_EMU_FFI_LIVE_AUDIO_UNEXPECTED_I16")
        }
        (_, FfiLivePcmBuffer::F32(values)) if !values.is_empty() => {
            panic!("ASTRA_EMU_FFI_LIVE_AUDIO_UNEXPECTED_F32")
        }
        (_, _) => (None, None),
    };
    let value = match kind {
        FfiLiveAudioCommandKind::LoadResource => LegacyAudioCommandV1::LoadResource {
            stream_id,
            encoding,
            resource_uri: resource_uri.to_string(),
        },
        FfiLiveAudioCommandKind::CreateStream => LegacyAudioCommandV1::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format,
        },
        FfiLiveAudioCommandKind::SubmitI16 => LegacyAudioCommandV1::SubmitI16 {
            stream_id,
            samples: i16_samples.expect("i16 sample command payload was checked"),
        },
        FfiLiveAudioCommandKind::SubmitF32 => LegacyAudioCommandV1::SubmitF32 {
            stream_id,
            samples: f32_samples.expect("f32 sample command payload was checked"),
        },
        FfiLiveAudioCommandKind::Play => LegacyAudioCommandV1::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms: fade_ms,
        },
        FfiLiveAudioCommandKind::Stop => LegacyAudioCommandV1::Stop { stream_id, fade_ms },
        FfiLiveAudioCommandKind::Pause => LegacyAudioCommandV1::Pause { stream_id },
        FfiLiveAudioCommandKind::Resume => LegacyAudioCommandV1::Resume { stream_id },
        FfiLiveAudioCommandKind::SetParams => LegacyAudioCommandV1::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
        },
        FfiLiveAudioCommandKind::DestroyStream => LegacyAudioCommandV1::DestroyStream { stream_id },
        FfiLiveAudioCommandKind::MasterVolume => LegacyAudioCommandV1::MasterVolume { volume },
    };
    LegacySequenced { sequence, value }
}

fn ffi_live_text(value: LegacySequenced<LegacyTextPresentationLeaseV1>) -> FfiLiveTextPresentation {
    let LegacySequenced {
        sequence,
        value: binding,
    } = value;
    FfiLiveTextPresentation {
        sequence,
        lease_id: binding.lease_id.into(),
        layout_id: binding.presentation.layout_id.into(),
        language: binding.presentation.language.into(),
        font_families: strings_to_ffi(binding.presentation.font_families),
        body: FfiLiveTextRegion {
            x: binding.presentation.body.x,
            y: binding.presentation.body.y,
            width: binding.presentation.body.width,
            height: binding.presentation.body.height,
            font_size: binding.presentation.body.font_size,
            line_height: binding.presentation.body.line_height,
            max_lines: binding.presentation.body.max_lines,
        },
        speaker: binding
            .presentation
            .speaker
            .map(|speaker| FfiLiveTextRegion {
                x: speaker.x,
                y: speaker.y,
                width: speaker.width,
                height: speaker.height,
                font_size: speaker.font_size,
                line_height: speaker.line_height,
                max_lines: speaker.max_lines,
            })
            .into(),
        rgba: binding.presentation.rgba,
    }
}

fn legacy_live_text(
    value: FfiLiveTextPresentation,
) -> LegacySequenced<LegacyTextPresentationLeaseV1> {
    LegacySequenced {
        sequence: value.sequence,
        value: LegacyTextPresentationLeaseV1 {
            lease_id: value.lease_id.to_string(),
            presentation: crate::LegacyTextPresentationV1 {
                layout_id: value.layout_id.to_string(),
                language: value.language.to_string(),
                font_families: value
                    .font_families
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                body: crate::LegacyTextRegionV1 {
                    x: value.body.x,
                    y: value.body.y,
                    width: value.body.width,
                    height: value.body.height,
                    font_size: value.body.font_size,
                    line_height: value.body.line_height,
                    max_lines: value.body.max_lines,
                },
                speaker: value
                    .speaker
                    .into_option()
                    .map(|speaker| crate::LegacyTextRegionV1 {
                        x: speaker.x,
                        y: speaker.y,
                        width: speaker.width,
                        height: speaker.height,
                        font_size: speaker.font_size,
                        line_height: speaker.line_height,
                        max_lines: speaker.max_lines,
                    }),
                rgba: value.rgba,
            },
        },
    }
}

fn ffi_live_output(value: LegacyLiveOutput) -> FfiLiveOutput {
    FfiLiveOutput {
        scenes: value
            .scenes
            .into_iter()
            .map(ffi_live_scene)
            .collect::<Vec<_>>()
            .into(),
        resource_scenes: value
            .resource_scenes
            .into_iter()
            .map(|scene| FfiLiveResourceScene {
                sequence: scene.sequence,
                width: scene.value.width,
                height: scene.value.height,
                textures: scene
                    .value
                    .texture_resources
                    .into_iter()
                    .map(|texture| FfiLiveResourceTexture {
                        texture_id: texture.texture_id,
                        resource_uri: texture.resource_uri.into(),
                        codec: texture.codec.into(),
                        revision: texture.revision,
                        decoded_width: texture.decoded_width,
                        decoded_height: texture.decoded_height,
                        decoded_format: ffi_live_format(texture.decoded_format),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                draws: scene
                    .value
                    .draws
                    .into_iter()
                    .map(ffi_live_draw)
                    .collect::<Vec<_>>()
                    .into(),
            })
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
        text: value
            .text
            .into_iter()
            .map(|text| FfiLiveTextLease {
                sequence: text.sequence,
                lease_id: text.lease_id.into(),
                byte_len: text.byte_len,
                source_ref: text.source_ref.into(),
            })
            .collect::<Vec<_>>()
            .into(),
        text_presentations: value
            .text_presentations
            .into_iter()
            .map(ffi_live_text)
            .collect::<Vec<_>>()
            .into(),
        video: value
            .video
            .into_iter()
            .map(|video| {
                let sequence = video.sequence;
                let (kind, playback_id, resource_uri, mode, stage_width, stage_height) =
                    match video.value {
                        LegacyVideoCommandV1::Play {
                            playback_id,
                            resource_uri,
                            mode,
                            stage_width,
                            stage_height,
                        } => (
                            FfiLiveVideoCommandKind::Play,
                            playback_id,
                            resource_uri,
                            match mode {
                                LegacyVideoMode::ModalWithAudio => FfiLiveVideoMode::ModalWithAudio,
                                LegacyVideoMode::LayerNoAudio => FfiLiveVideoMode::LayerNoAudio,
                            },
                            stage_width,
                            stage_height,
                        ),
                        LegacyVideoCommandV1::Stop { playback_id } => (
                            FfiLiveVideoCommandKind::Stop,
                            playback_id,
                            String::new(),
                            FfiLiveVideoMode::LayerNoAudio,
                            0,
                            0,
                        ),
                    };
                FfiLiveVideoCommand {
                    sequence,
                    playback_id: playback_id.into(),
                    resource_uri: resource_uri.into(),
                    mode,
                    stage_width,
                    stage_height,
                    kind,
                }
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

fn legacy_live_output(value: FfiLiveOutput) -> LegacyLiveOutput {
    LegacyLiveOutput {
        scenes: value.scenes.into_iter().map(legacy_live_scene).collect(),
        resource_scenes: value
            .resource_scenes
            .into_iter()
            .map(|scene| LegacySequenced {
                sequence: scene.sequence,
                value: LegacyRenderResourceFrameV1 {
                    width: scene.width,
                    height: scene.height,
                    texture_resources: scene
                        .textures
                        .into_iter()
                        .map(|texture| crate::LegacyTextureResourceV1 {
                            texture_id: texture.texture_id,
                            resource_uri: texture.resource_uri.to_string(),
                            codec: texture.codec.to_string(),
                            revision: texture.revision,
                            decoded_width: texture.decoded_width,
                            decoded_height: texture.decoded_height,
                            decoded_format: legacy_live_format(texture.decoded_format),
                        })
                        .collect(),
                    draws: scene.draws.into_iter().map(legacy_live_draw).collect(),
                },
            })
            .collect(),
        audio: value.audio.into_iter().map(legacy_live_audio).collect(),
        audio_commands: value
            .audio_commands
            .into_iter()
            .map(legacy_live_audio_command)
            .collect(),
        text: value
            .text
            .into_iter()
            .map(|text| LegacyTextLease {
                sequence: text.sequence,
                lease_id: text.lease_id.to_string(),
                byte_len: text.byte_len,
                source_ref: text.source_ref.to_string(),
            })
            .collect(),
        text_presentations: value
            .text_presentations
            .into_iter()
            .map(legacy_live_text)
            .collect(),
        video: value
            .video
            .into_iter()
            .map(|video| LegacySequenced {
                sequence: video.sequence,
                value: match video.kind {
                    FfiLiveVideoCommandKind::Play => LegacyVideoCommandV1::Play {
                        playback_id: video.playback_id.to_string(),
                        resource_uri: video.resource_uri.to_string(),
                        mode: match video.mode {
                            FfiLiveVideoMode::ModalWithAudio => LegacyVideoMode::ModalWithAudio,
                            FfiLiveVideoMode::LayerNoAudio => LegacyVideoMode::LayerNoAudio,
                        },
                        stage_width: video.stage_width,
                        stage_height: video.stage_height,
                    },
                    FfiLiveVideoCommandKind::Stop => LegacyVideoCommandV1::Stop {
                        playback_id: video.playback_id.to_string(),
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
    pub value: RVec<u8>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiScheduledEvent {
    pub sequence: u64,
    pub due_tick: u64,
    pub event: RString,
    pub payload: RVec<u8>,
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
    pub scheduled_events: RVec<FfiScheduledEvent>,
    pub dirty_sections: RVec<FfiDirtySection>,
    pub waits: RVec<FfiWaitRequest>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
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
                        payload: live_payload_to_ffi(event.payload),
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
                        value: live_payload_to_ffi(mutation.value),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                scheduled_events: value
                    .control
                    .scheduled_events
                    .into_iter()
                    .map(|event| FfiScheduledEvent {
                        sequence: event.sequence,
                        due_tick: event.due_tick,
                        event: event.event.into(),
                        payload: live_payload_to_ffi(event.payload),
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
                        payload: live_payload_from_ffi(event.payload),
                    })
                    .collect(),
                blackboard: control
                    .blackboard
                    .into_iter()
                    .map(|mutation| LegacyBlackboardMutation {
                        sequence: mutation.sequence,
                        key: mutation.key.to_string(),
                        value: live_payload_from_ffi(mutation.value),
                    })
                    .collect(),
                scheduled_events: control
                    .scheduled_events
                    .into_iter()
                    .map(|event| LegacyScheduledEvent {
                        sequence: event.sequence,
                        due_tick: event.due_tick,
                        event: event.event.to_string(),
                        payload: live_payload_from_ffi(event.payload),
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
    fn scene_rgba8_allocation_moves_across_family_ffi_wire() {
        let pixels = vec![255, 0, 128, 255];
        let source_ptr = pixels.as_ptr();
        let transaction = LegacySceneTransactionV7 {
            sequence: 7,
            width: 1,
            height: 1,
            resources: vec![LegacySceneResourceOperationV7::CreateTexture {
                texture_id: 11,
                generation: 1,
                width: 1,
                height: 1,
                format: LegacyTextureFormat::Rgba8,
                pixels: LegacyPayload::Native(pixels),
            }],
            draws: Vec::new(),
            reset_resources: false,
        };

        let ffi = ffi_live_scene(transaction);
        let ffi_ptr = match ffi.resources.as_slice().first().expect("scene resource") {
            FfiLiveSceneResourceOperation::Create(value) => value.pixels.as_slice().as_ptr(),
            _ => panic!("expected create texture"),
        };
        assert_eq!(ffi_ptr, source_ptr);

        let legacy = legacy_live_scene(ffi);
        let returned_ptr = match legacy.resources.as_slice().first().expect("scene resource") {
            LegacySceneResourceOperationV7::CreateTexture { pixels, .. } => {
                pixels.as_bytes().as_ptr()
            }
            _ => panic!("expected create texture"),
        };
        assert_eq!(returned_ptr, source_ptr);
    }

    #[test]
    fn pcm_i16_allocation_moves_across_family_ffi_wire() {
        let samples = vec![-3_i16, 0, 17, 4096];
        let source_ptr = samples.as_ptr();
        let packet = LegacyAudioPacketV7 {
            sequence: 9,
            stream_id: 2,
            sample_rate: 48_000,
            channels: 2,
            pcm: LegacyPcmBufferV7::I16(samples),
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
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSnapshotSection {
    pub section_id: RString,
    pub schema: RString,
    pub version: FfiSchemaVersion,
    pub bytes: FfiBulkBytes,
}

impl From<LegacySnapshotSection> for FfiSnapshotSection {
    fn from(value: LegacySnapshotSection) -> Self {
        Self {
            section_id: value.section_id.into(),
            schema: value.schema.into(),
            version: value.version.into(),
            bytes: bulk_bytes_from_vec(value.bytes),
        }
    }
}
impl From<FfiSnapshotSection> for LegacySnapshotSection {
    fn from(value: FfiSnapshotSection) -> Self {
        Self {
            section_id: value.section_id.to_string(),
            schema: value.schema.to_string(),
            version: value.version.into(),
            bytes: bulk_bytes_to_vec(&value.bytes),
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
    pub diagnostics: RVec<FfiDiagnostic>,
}
impl From<LegacyShutdownReport> for FfiShutdownReport {
    fn from(value: LegacyShutdownReport) -> Self {
        Self {
            final_state_revision: value.final_state_revision,
            instruction_count: value.instruction_count,
            syscall_count: value.syscall_count,
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
    pub revision: FfiHash256,
}
impl From<astra_byte_source::ByteSourceStat> for FfiByteSourceStat {
    fn from(value: astra_byte_source::ByteSourceStat) -> Self {
        Self {
            len: value.len,
            revision: value.revision.0.into(),
        }
    }
}
impl From<FfiByteSourceStat> for astra_byte_source::ByteSourceStat {
    fn from(value: FfiByteSourceStat) -> Self {
        Self {
            len: value.len,
            revision: astra_byte_source::SourceRevision(value.revision.into()),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiRangeReadResult {
    pub range: FfiByteRange,
    pub revision: FfiHash256,
    pub bytes: FfiBulkBytes,
}
impl From<astra_byte_source::RangeReadResult> for FfiRangeReadResult {
    fn from(value: astra_byte_source::RangeReadResult) -> Self {
        Self {
            range: value.range.into(),
            revision: value.revision.0.into(),
            bytes: bulk_bytes_from_vec(value.bytes),
        }
    }
}
impl From<FfiRangeReadResult> for astra_byte_source::RangeReadResult {
    fn from(value: FfiRangeReadResult) -> Self {
        Self {
            range: value.range.into(),
            revision: astra_byte_source::SourceRevision(value.revision.into()),
            bytes: bulk_bytes_to_vec(&value.bytes),
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
