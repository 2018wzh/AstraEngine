use std::collections::BTreeMap;

use abi_stable::{
    std_types::{RArc, ROption, RString, RVec},
    StableAbi,
};
use astra_core::{Hash256, SchemaVersion};

use crate::{
    FamilyId, LegacyAwaitResult, LegacyCoverageDelta, LegacyDiagnostic, LegacyEffect,
    LegacyEphemeralText, LegacyFamilyPluginDescriptor, LegacyInputEdge, LegacyOpenRequest,
    LegacyPayload, LegacyPayloadStorage, LegacyProbeReport, LegacyProbeRequest,
    LegacyProviderError, LegacyProviderResult, LegacyReplayMode, LegacyRestoreReport,
    LegacyRuntimeHostCtx, LegacyRuntimeSessionId, LegacyRuntimeStatus, LegacyShutdownReport,
    LegacySnapshotEnvelope, LegacySnapshotSection, LegacyStepBudget, LegacyStepInput,
    LegacyStepOutput, LegacyTraceEntry, LegacyVfsListedFile, LegacyWaitRequest,
};

pub type FfiBulkBytes = RArc<RVec<u8>>;

struct FfiBulkStorage(FfiBulkBytes);

impl LegacyPayloadStorage for FfiBulkStorage {
    fn bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn payload_to_ffi(value: LegacyPayload) -> FfiBulkBytes {
    match value {
        LegacyPayload::Native(bytes) => bulk_bytes_from_vec(bytes),
        LegacyPayload::Foreign(storage) => storage
            .as_any()
            .downcast_ref::<FfiBulkStorage>()
            .map(|storage| storage.0.clone())
            .unwrap_or_else(|| bulk_bytes_from_vec(storage.bytes().to_vec())),
    }
}

fn payload_from_ffi(value: FfiBulkBytes) -> LegacyPayload {
    LegacyPayload::from_foreign(std::sync::Arc::new(FfiBulkStorage(value)))
}

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
        pub struct $ffi { $(pub $field: RString,)+ pub payload_hash: FfiHash256, pub sequence: u64 }
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
            payload_hash: value.payload_hash.into(),
            sequence: value.sequence,
        }
    }
}
impl From<FfiAwaitResult> for LegacyAwaitResult {
    fn from(value: FfiAwaitResult) -> Self {
        Self {
            token_id: value.token_id.to_string(),
            status: value.status.to_string(),
            payload_hash: value.payload_hash.into(),
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
            payload_hash: value.payload_hash.into(),
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
            payload_hash: value.payload_hash.into(),
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
pub enum FfiEffectKind {
    RuntimeEvent,
    Presentation,
    Audio,
    TextCapture,
    SetBlackboard,
    ScheduleEvent,
    SnapshotDirty,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiEffect {
    pub kind: FfiEffectKind,
    pub sequence: u64,
    pub name: RString,
    pub auxiliary: RString,
    pub payload: FfiBulkBytes,
    pub hash: ROption<FfiHash256>,
    pub auxiliary_hash: ROption<FfiHash256>,
    pub number: u64,
}

impl From<LegacyEffect> for FfiEffect {
    fn from(value: LegacyEffect) -> Self {
        let empty = || RArc::new(RVec::new());
        match value {
            LegacyEffect::RuntimeEvent {
                sequence,
                event,
                payload,
            } => Self {
                kind: FfiEffectKind::RuntimeEvent,
                sequence,
                name: event.into(),
                auxiliary: RString::new(),
                payload: payload_to_ffi(payload),
                hash: ROption::RNone,
                auxiliary_hash: ROption::RNone,
                number: 0,
            },
            LegacyEffect::Presentation {
                sequence,
                command,
                payload,
            } => Self {
                kind: FfiEffectKind::Presentation,
                sequence,
                name: command.into(),
                auxiliary: RString::new(),
                payload: payload_to_ffi(payload),
                hash: ROption::RNone,
                auxiliary_hash: ROption::RNone,
                number: 0,
            },
            LegacyEffect::Audio {
                sequence,
                command,
                payload,
            } => Self {
                kind: FfiEffectKind::Audio,
                sequence,
                name: command.into(),
                auxiliary: RString::new(),
                payload: payload_to_ffi(payload),
                hash: ROption::RNone,
                auxiliary_hash: ROption::RNone,
                number: 0,
            },
            LegacyEffect::TextCapture {
                sequence,
                lease_id,
                text_hash,
                byte_len,
                speaker_hash,
                source_ref,
            } => Self {
                kind: FfiEffectKind::TextCapture,
                sequence,
                name: lease_id.into(),
                auxiliary: source_ref.into(),
                payload: empty(),
                hash: ROption::RSome(text_hash.into()),
                auxiliary_hash: speaker_hash.map(Into::into).into(),
                number: u64::from(byte_len),
            },
            LegacyEffect::SetBlackboard {
                sequence,
                key,
                value,
            } => Self {
                kind: FfiEffectKind::SetBlackboard,
                sequence,
                name: key.into(),
                auxiliary: RString::new(),
                payload: payload_to_ffi(value),
                hash: ROption::RNone,
                auxiliary_hash: ROption::RNone,
                number: 0,
            },
            LegacyEffect::ScheduleEvent {
                sequence,
                due_tick,
                event,
                payload,
            } => Self {
                kind: FfiEffectKind::ScheduleEvent,
                sequence,
                name: event.into(),
                auxiliary: RString::new(),
                payload: payload_to_ffi(payload),
                hash: ROption::RNone,
                auxiliary_hash: ROption::RNone,
                number: due_tick,
            },
            LegacyEffect::SnapshotDirty {
                sequence,
                section_id,
            } => Self {
                kind: FfiEffectKind::SnapshotDirty,
                sequence,
                name: section_id.into(),
                auxiliary: RString::new(),
                payload: empty(),
                hash: ROption::RNone,
                auxiliary_hash: ROption::RNone,
                number: 0,
            },
        }
    }
}

impl TryFrom<FfiEffect> for LegacyEffect {
    type Error = LegacyProviderError;
    fn try_from(value: FfiEffect) -> Result<Self, Self::Error> {
        let payload = || payload_from_ffi(value.payload.clone());
        Ok(match value.kind {
            FfiEffectKind::RuntimeEvent => Self::RuntimeEvent {
                sequence: value.sequence,
                event: value.name.to_string(),
                payload: payload(),
            },
            FfiEffectKind::Presentation => Self::Presentation {
                sequence: value.sequence,
                command: value.name.to_string(),
                payload: payload(),
            },
            FfiEffectKind::Audio => Self::Audio {
                sequence: value.sequence,
                command: value.name.to_string(),
                payload: payload(),
            },
            FfiEffectKind::TextCapture => Self::TextCapture {
                sequence: value.sequence,
                lease_id: value.name.to_string(),
                text_hash: value
                    .hash
                    .into_option()
                    .ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_FFI_EFFECT",
                            "text capture hash is missing",
                        )
                    })?
                    .into(),
                byte_len: u32::try_from(value.number).map_err(|_| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_FFI_EFFECT",
                        "text capture byte length overflowed",
                    )
                })?,
                speaker_hash: value.auxiliary_hash.into_option().map(Into::into),
                source_ref: value.auxiliary.to_string(),
            },
            FfiEffectKind::SetBlackboard => Self::SetBlackboard {
                sequence: value.sequence,
                key: value.name.to_string(),
                value: payload(),
            },
            FfiEffectKind::ScheduleEvent => Self::ScheduleEvent {
                sequence: value.sequence,
                due_tick: value.number,
                event: value.name.to_string(),
                payload: payload(),
            },
            FfiEffectKind::SnapshotDirty => Self::SnapshotDirty {
                sequence: value.sequence,
                section_id: value.name.to_string(),
            },
        })
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
    pub payload_hash: ROption<FfiHash256>,
}

impl From<LegacyWaitRequest> for FfiWaitRequest {
    fn from(value: LegacyWaitRequest) -> Self {
        let base = |kind, token_id: String| Self {
            kind,
            token_id: token_id.into(),
            name: RString::new(),
            keys: RVec::new(),
            number: 0,
            payload_hash: ROption::RNone,
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
                payload_hash,
            } => Self {
                name: wait_kind.into(),
                payload_hash: ROption::RSome(payload_hash.into()),
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
                payload_hash: value
                    .payload_hash
                    .into_option()
                    .ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_FFI_WAIT",
                            "opaque wait hash is missing",
                        )
                    })?
                    .into(),
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
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiStepOutput {
    pub status: FfiRuntimeStatus,
    pub effects: RVec<FfiEffect>,
    pub waits: RVec<FfiWaitRequest>,
    pub trace: RVec<FfiTraceEntry>,
    pub diagnostics: RVec<FfiDiagnostic>,
    pub coverage: FfiCoverageDelta,
    pub state_hash: FfiHash256,
}

impl From<LegacyStepOutput> for FfiStepOutput {
    fn from(value: LegacyStepOutput) -> Self {
        Self {
            status: value.status.into(),
            effects: value
                .effects
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            waits: value
                .waits
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
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
            state_hash: value.state_hash.into(),
        }
    }
}

impl TryFrom<FfiStepOutput> for LegacyStepOutput {
    type Error = LegacyProviderError;
    fn try_from(value: FfiStepOutput) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status.into(),
            effects: value
                .effects
                .iter()
                .cloned()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            waits: value
                .waits
                .iter()
                .cloned()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            trace: value.trace.iter().cloned().map(Into::into).collect(),
            diagnostics: value.diagnostics.iter().cloned().map(Into::into).collect(),
            coverage: value.coverage.into(),
            state_hash: value.state_hash.into(),
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSnapshotSection {
    pub section_id: RString,
    pub schema: RString,
    pub version: FfiSchemaVersion,
    pub hash: FfiHash256,
    pub bytes: FfiBulkBytes,
}

impl From<LegacySnapshotSection> for FfiSnapshotSection {
    fn from(value: LegacySnapshotSection) -> Self {
        Self {
            section_id: value.section_id.into(),
            schema: value.schema.into(),
            version: value.version.into(),
            hash: value.hash.into(),
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
            hash: value.hash.into(),
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
    pub state_hash: FfiHash256,
    pub diagnostics: RVec<FfiDiagnostic>,
}
impl From<LegacyRestoreReport> for FfiRestoreReport {
    fn from(value: LegacyRestoreReport) -> Self {
        Self {
            restored_fixed_step: value.restored_fixed_step,
            session_seed: value.session_seed,
            state_hash: value.state_hash.into(),
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
            state_hash: value.state_hash.into(),
            diagnostics: value.diagnostics.iter().cloned().map(Into::into).collect(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiShutdownReport {
    pub final_state_hash: FfiHash256,
    pub instruction_count: u64,
    pub syscall_count: u64,
    pub diagnostics: RVec<FfiDiagnostic>,
}
impl From<LegacyShutdownReport> for FfiShutdownReport {
    fn from(value: LegacyShutdownReport) -> Self {
        Self {
            final_state_hash: value.final_state_hash.into(),
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
            final_state_hash: value.final_state_hash.into(),
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
    pub content_hash: FfiHash256,
    pub bytes: FfiBulkBytes,
}
impl From<astra_byte_source::RangeReadResult> for FfiRangeReadResult {
    fn from(value: astra_byte_source::RangeReadResult) -> Self {
        Self {
            range: value.range.into(),
            revision: value.revision.0.into(),
            content_hash: value.content_hash.into(),
            bytes: bulk_bytes_from_vec(value.bytes),
        }
    }
}
impl From<FfiRangeReadResult> for astra_byte_source::RangeReadResult {
    fn from(value: FfiRangeReadResult) -> Self {
        Self {
            range: value.range.into(),
            revision: astra_byte_source::SourceRevision(value.revision.into()),
            content_hash: value.content_hash.into(),
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
