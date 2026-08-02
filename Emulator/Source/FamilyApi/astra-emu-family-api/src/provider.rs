use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Hash256, SchemaVersion};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_SYMBOL_BYTES: usize = 128;
const MAX_EFFECTS_PER_STEP: usize = 65_536;
const MAX_TRACE_ENTRIES_PER_STEP: u32 = 1_000_000;
const MAX_DIAGNOSTICS_PER_STEP: usize = 256;
const MAX_SNAPSHOT_SECTIONS: usize = 128;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const MAX_EFFECT_PAYLOAD_BYTES_PER_STEP: usize = 256 * 1024 * 1024;
const MAX_RENDER_DRAWS: usize = 262_144;
const MAX_RENDER_TEXTURE_UPDATES: usize = 4096;
const MAX_AUDIO_SAMPLES_PER_COMMAND: usize = 4_194_304;
const MAX_WAITS_PER_STEP: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct FamilyId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct LegacyRuntimeSessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyFamilyPluginDescriptor {
    pub family_id: FamilyId,
    pub plugin_id: String,
    pub provider_id: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
    pub supported_formats: Vec<String>,
    pub permissions: Vec<String>,
    pub report_redaction: String,
    pub license: String,
}

impl LegacyFamilyPluginDescriptor {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        for (field, value) in [
            ("family_id", self.family_id.0.as_str()),
            ("plugin_id", self.plugin_id.as_str()),
            ("provider_id", self.provider_id.as_str()),
            ("engine_version", self.engine_version.as_str()),
            ("rustc_fingerprint", self.rustc_fingerprint.as_str()),
            ("feature_fingerprint", self.feature_fingerprint.as_str()),
            ("abi_fingerprint", self.abi_fingerprint.as_str()),
            ("report_redaction", self.report_redaction.as_str()),
            ("license", self.license.as_str()),
        ] {
            validate_symbol(field, value)?;
        }
        validate_unique_symbols("supported_formats", &self.supported_formats)?;
        validate_unique_symbols("permissions", &self.permissions)?;
        if self.supported_formats.is_empty() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_DESCRIPTOR_FORMATS",
                "family descriptor must declare at least one supported format",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyRuntimeHostCtx {
    pub case_id: String,
    pub package_id: String,
    pub package_hash: Hash256,
    pub mount_set_id: String,
    pub media_service_ids: Vec<String>,
    pub permission_policy_id: String,
    pub report_sink_id: String,
    pub target: String,
    pub profile: String,
}

impl LegacyRuntimeHostCtx {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        for (field, value) in [
            ("case_id", self.case_id.as_str()),
            ("package_id", self.package_id.as_str()),
            ("mount_set_id", self.mount_set_id.as_str()),
            ("permission_policy_id", self.permission_policy_id.as_str()),
            ("report_sink_id", self.report_sink_id.as_str()),
            ("target", self.target.as_str()),
            ("profile", self.profile.as_str()),
        ] {
            validate_symbol(field, value)?;
        }
        validate_unique_symbols("media_service_ids", &self.media_service_ids)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyProbeRequest {
    pub root_mount_id: String,
    pub candidate_uris: Vec<String>,
    pub marker_hashes: Vec<Hash256>,
    pub max_entries: u32,
    pub max_metadata_bytes: u64,
}

pub trait LegacyVfsReader: Send + Sync {
    fn stat_file(
        &self,
        mount_set_id: &str,
        uri: &str,
    ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError>;

    fn read_file_range(
        &self,
        mount_set_id: &str,
        uri: &str,
        expected_revision: astra_byte_source::SourceRevision,
        range: astra_byte_source::ByteRange,
        max_bytes: u64,
    ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError>;

    /// Lists only metadata for files matching one extension below `root`.
    /// Dynamic family code must use this bounded host port instead of opening a
    /// source directory or preloading a package.
    fn enumerate_by_extension(
        &self,
        _mount_set_id: &str,
        _root: &str,
        _extension_without_dot: &str,
        _max_entries: u32,
    ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
        Err(LegacyProviderError::invalid(
            "ASTRA_EMU_VFS_ENUMERATE_UNSUPPORTED",
            "the bound VFS does not expose bounded enumeration",
        ))
    }

    fn read_file(
        &self,
        mount_set_id: &str,
        uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, LegacyProviderError> {
        let stat = self.stat_file(mount_set_id, uri)?;
        if stat.len > max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_BOUNDS",
                "VFS entry exceeds the requested byte bound",
            ));
        }
        self.read_file_range(
            mount_set_id,
            uri,
            stat.revision,
            astra_byte_source::ByteRange {
                offset: 0,
                len: stat.len,
            },
            max_bytes,
        )
        .map(|result| result.bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsListedFile {
    pub uri: String,
    pub stat: astra_byte_source::ByteSourceStat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyProbeReport {
    pub family_id: FamilyId,
    pub confidence_permyriad: u16,
    pub markers: Vec<String>,
    pub blockers: Vec<LegacyDiagnostic>,
    pub content_identity: Hash256,
}

impl LegacyProbeReport {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        if self.confidence_permyriad > 10_000 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_PROBE_CONFIDENCE",
                "probe confidence exceeds 10000 permyriad",
            ));
        }
        validate_unique_symbols("markers", &self.markers)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyOpenRequest {
    pub requested_session_id: LegacyRuntimeSessionId,
    pub case_fingerprint: Hash256,
    pub script_uri: String,
    pub fixed_delta_ns: u64,
    pub session_seed: u64,
    pub compatibility_profile: String,
    pub family_options: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReplayMode {
    Live,
    RestoreContinuation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyStepBudget {
    pub max_instructions: u32,
    pub max_effects: u32,
    pub max_trace_entries: u32,
}

impl LegacyStepBudget {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        if self.max_instructions == 0 || self.max_instructions > 10_000_000 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_INSTRUCTION_BUDGET",
                "instruction budget must be in 1..=10000000",
            ));
        }
        if self.max_effects == 0 || self.max_effects as usize > MAX_EFFECTS_PER_STEP {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_EFFECT_BUDGET",
                "effect budget is outside the supported bound",
            ));
        }
        if self.max_trace_entries == 0 || self.max_trace_entries > MAX_TRACE_ENTRIES_PER_STEP {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_TRACE_BUDGET",
                "trace budget is outside the supported bound",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyStepInput {
    pub tick_index: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: LegacyReplayMode,
    pub input_edges: Vec<LegacyInputEdge>,
    pub await_results: Vec<LegacyAwaitResult>,
    pub provider_results: Vec<LegacyProviderResult>,
    pub budget: LegacyStepBudget,
}

impl LegacyStepInput {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        self.budget.validate()?;
        if self.tick_index == 0 || self.delta_ns == 0 || self.delta_ns > 1_000_000_000 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_TIMING",
                "tick must be non-zero and delta must be within 1ns..=1s",
            ));
        }
        if self.input_edges.len() > 4096
            || self.await_results.len() > 4096
            || self.provider_results.len() > 4096
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_INPUT_BOUNDS",
                "step input channel exceeds 4096 entries",
            ));
        }
        validate_sequence(
            "input_edges",
            self.input_edges.iter().map(|item| item.sequence),
        )?;
        validate_sequence(
            "await_results",
            self.await_results.iter().map(|item| item.sequence),
        )?;
        validate_sequence(
            "provider_results",
            self.provider_results.iter().map(|item| item.sequence),
        )?;
        for edge in &self.input_edges {
            validate_symbol("input_control", &edge.control)?;
            if !edge.value.is_finite() {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_STEP_INPUT_VALUE",
                    "input edge value must be finite",
                ));
            }
        }
        for result in &self.await_results {
            validate_symbol("await_token_id", &result.token_id)?;
            validate_symbol("await_status", &result.status)?;
        }
        for result in &self.provider_results {
            validate_symbol("provider_request_id", &result.request_id)?;
            validate_symbol("provider_id", &result.provider_id)?;
            validate_symbol("provider_status", &result.status)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyInputEdge {
    pub control: String,
    pub pressed: bool,
    pub value: f32,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyAwaitResult {
    pub token_id: String,
    pub status: String,
    pub payload_hash: Hash256,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyProviderResult {
    pub request_id: String,
    pub provider_id: String,
    pub status: String,
    pub payload_hash: Hash256,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyRuntimeStatus {
    Active,
    Awaiting,
    Terminal,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyStepOutput {
    pub status: LegacyRuntimeStatus,
    pub effects: Vec<LegacyEffect>,
    pub waits: Vec<LegacyWaitRequest>,
    pub trace: Vec<LegacyTraceEntry>,
    pub diagnostics: Vec<LegacyDiagnostic>,
    pub coverage: LegacyCoverageDelta,
    pub state_hash: Hash256,
}

impl LegacyStepOutput {
    pub fn validate(&self, budget: &LegacyStepBudget) -> Result<(), LegacyProviderError> {
        budget.validate()?;
        if self.effects.len() > budget.max_effects as usize
            || self.effects.len() > MAX_EFFECTS_PER_STEP
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_EFFECT_COUNT",
                "provider returned more effects than the negotiated budget",
            ));
        }
        if self.trace.len() > budget.max_trace_entries as usize {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_TRACE_COUNT",
                "provider returned more trace entries than the negotiated budget",
            ));
        }
        if self.diagnostics.len() > MAX_DIAGNOSTICS_PER_STEP {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_DIAGNOSTIC_COUNT",
                "provider returned too many diagnostics",
            ));
        }
        if self.waits.len() > MAX_WAITS_PER_STEP {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_WAIT_COUNT",
                "provider returned too many wait requests",
            ));
        }
        let mut sequences = BTreeSet::new();
        let mut payload_bytes = 0usize;
        for effect in &self.effects {
            if !sequences.insert(effect.sequence()) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_EFFECT_SEQUENCE_DUPLICATE",
                    "effect sequence is duplicated",
                ));
            }
            match effect {
                LegacyEffect::RuntimeEvent { event, payload, .. } => {
                    validate_symbol("runtime_event", event)?;
                    payload_bytes = payload_bytes.checked_add(payload.len()).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_EFFECT_PAYLOAD_BOUNDS",
                            "effect payload length overflow",
                        )
                    })?;
                }
                LegacyEffect::Presentation {
                    command, payload, ..
                } => {
                    validate_symbol("effect_command", command)?;
                    if command == "astra.emu.video_command.v1" {
                        let decoded: LegacyVideoCommandV1 =
                            postcard::from_bytes(payload).map_err(|_| {
                                LegacyProviderError::invalid(
                                    "ASTRA_EMU_VIDEO_COMMAND_DECODE",
                                    "video command payload is malformed",
                                )
                            })?;
                        decoded.validate()?;
                    }
                    payload_bytes = payload_bytes.checked_add(payload.len()).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_EFFECT_PAYLOAD_BOUNDS",
                            "effect payload length overflow",
                        )
                    })?;
                }
                LegacyEffect::Audio {
                    command, payload, ..
                } => {
                    validate_symbol("effect_command", command)?;
                    if command == "astra.emu.audio_command.v1" {
                        let decoded: LegacyAudioCommandV1 =
                            postcard::from_bytes(payload).map_err(|_| {
                                LegacyProviderError::invalid(
                                    "ASTRA_EMU_AUDIO_COMMAND_DECODE",
                                    "audio command payload is malformed",
                                )
                            })?;
                        decoded.validate()?;
                    }
                    payload_bytes = payload_bytes.checked_add(payload.len()).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_EFFECT_PAYLOAD_BOUNDS",
                            "effect payload length overflow",
                        )
                    })?;
                }
                LegacyEffect::TextCapture {
                    lease_id,
                    source_ref,
                    ..
                } => {
                    validate_symbol("text_lease_id", lease_id)?;
                    validate_symbol("text_source_ref", source_ref)?;
                }
                LegacyEffect::SetBlackboard { key, value, .. } => {
                    validate_symbol("blackboard_key", key)?;
                    payload_bytes = payload_bytes.checked_add(value.len()).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_EFFECT_PAYLOAD_BOUNDS",
                            "effect payload length overflow",
                        )
                    })?;
                }
                LegacyEffect::ScheduleEvent { event, payload, .. } => {
                    validate_symbol("scheduled_event", event)?;
                    payload_bytes = payload_bytes.checked_add(payload.len()).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_EFFECT_PAYLOAD_BOUNDS",
                            "effect payload length overflow",
                        )
                    })?;
                }
                LegacyEffect::SnapshotDirty { section_id, .. } => {
                    validate_symbol("snapshot_section", section_id)?;
                }
            }
        }
        if payload_bytes > MAX_EFFECT_PAYLOAD_BYTES_PER_STEP {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_EFFECT_PAYLOAD_BOUNDS",
                "combined effect payloads exceed the per-step bound",
            ));
        }
        let mut wait_tokens = BTreeSet::new();
        for wait in &self.waits {
            let token_id = match wait {
                LegacyWaitRequest::Frame { token_id, frames } => {
                    if *frames == 0 {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_WAIT_FRAME_BOUNDS",
                            "frame wait must request at least one frame",
                        ));
                    }
                    token_id
                }
                LegacyWaitRequest::Time {
                    token_id,
                    milliseconds,
                } => {
                    if *milliseconds == 0 {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_WAIT_TIME_BOUNDS",
                            "time wait must request a positive duration",
                        ));
                    }
                    token_id
                }
                LegacyWaitRequest::Input { token_id, mask } => {
                    if *mask == 0 {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_WAIT_INPUT_MASK",
                            "input wait mask must not be empty",
                        ));
                    }
                    token_id
                }
                LegacyWaitRequest::MediaFence { token_id, media_id } => {
                    validate_symbol("wait_media_id", media_id)?;
                    token_id
                }
                LegacyWaitRequest::PresentationFence { token_id, fence_id } => {
                    validate_symbol("wait_fence_id", fence_id)?;
                    token_id
                }
                LegacyWaitRequest::ProviderCompletion {
                    token_id,
                    request_id,
                } => {
                    validate_symbol("wait_request_id", request_id)?;
                    token_id
                }
                LegacyWaitRequest::FamilyOpaque {
                    token_id,
                    wait_kind,
                    ..
                } => {
                    validate_symbol("wait_kind", wait_kind)?;
                    token_id
                }
            };
            validate_symbol("wait_token_id", token_id)?;
            if !wait_tokens.insert(token_id.as_str()) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_WAIT_TOKEN_DUPLICATE",
                    "wait token id is duplicated",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyEffect {
    RuntimeEvent {
        sequence: u64,
        event: String,
        payload: Vec<u8>,
    },
    Presentation {
        sequence: u64,
        command: String,
        payload: Vec<u8>,
    },
    Audio {
        sequence: u64,
        command: String,
        payload: Vec<u8>,
    },
    TextCapture {
        sequence: u64,
        lease_id: String,
        text_hash: Hash256,
        byte_len: u32,
        speaker_hash: Option<Hash256>,
        source_ref: String,
    },
    SetBlackboard {
        sequence: u64,
        key: String,
        value: Vec<u8>,
    },
    ScheduleEvent {
        sequence: u64,
        due_tick: u64,
        event: String,
        payload: Vec<u8>,
    },
    SnapshotDirty {
        sequence: u64,
        section_id: String,
    },
}

/// Host-neutral, plaintext-free layout contract for an ephemeral text lease.
/// The family owns the original layout semantics while the host owns shaping,
/// glyph resources, and rendering through its explicitly selected providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTextPresentationV1 {
    pub layout_id: String,
    pub language: String,
    pub font_families: Vec<String>,
    pub body: LegacyTextRegionV1,
    pub speaker: Option<LegacyTextRegionV1>,
    pub rgba: [u8; 4],
}

/// Associates a plaintext-free layout with the single-use text lease emitted
/// in the same ordered effect batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTextPresentationLeaseV1 {
    pub lease_id: String,
    pub presentation: LegacyTextPresentationV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTextRegionV1 {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub line_height: f32,
    pub max_lines: u32,
}

impl LegacyTextPresentationV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_symbol("text_layout_id", &self.layout_id)?;
        if self.language.trim().is_empty()
            || self.language.len() > 64
            || self.font_families.is_empty()
            || self.font_families.len() > 8
            || self.font_families.iter().any(|family| {
                family.trim().is_empty()
                    || family.len() > 128
                    || family.chars().any(char::is_control)
            })
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_TEXT_LAYOUT_BINDING",
                "text language or explicit font family binding is invalid",
            ));
        }
        self.body.validate("body")?;
        if let Some(speaker) = self.speaker {
            speaker.validate("speaker")?;
        }
        if self.rgba[3] == 0 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_TEXT_LAYOUT_COLOR",
                "text color must have non-zero alpha",
            ));
        }
        Ok(())
    }
}

impl LegacyTextPresentationLeaseV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_symbol("text_presentation_lease_id", &self.lease_id)?;
        self.presentation.validate()
    }
}

impl LegacyTextRegionV1 {
    fn validate(self, region: &'static str) -> Result<(), LegacyProviderError> {
        if self.x < 0
            || self.y < 0
            || self.width == 0
            || self.height == 0
            || self.width > 16_384
            || self.height > 16_384
            || !self.font_size.is_finite()
            || !self.line_height.is_finite()
            || !(1.0..=512.0).contains(&self.font_size)
            || self.line_height < self.font_size
            || self.line_height > 1024.0
            || self.max_lines == 0
            || self.max_lines > 256
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_TEXT_LAYOUT_REGION",
                format!("text {region} layout region is invalid"),
            ));
        }
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);
        if right > i64::from(i32::MAX) || bottom > i64::from(i32::MAX) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_TEXT_LAYOUT_REGION",
                format!("text {region} layout region overflows"),
            ));
        }
        Ok(())
    }
}

/// Host-neutral GPU presentation packet. Family providers may emit this as the postcard payload
/// of a `Presentation` effect whose command is `astra.emu.render_frame.v1`. It deliberately owns
/// no window, device, queue, texture, or callback; the host uploads resource deltas to its own
/// renderer and executes the ordered draw list on the shared device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyRenderFrameV1 {
    pub width: u32,
    pub height: u32,
    pub texture_updates: Vec<LegacyTextureUpdateV1>,
    pub draws: Vec<LegacyDrawV1>,
}

/// Incremental, host-neutral scene packet.  This is the v5 rendering contract
/// for providers which retain resources across steps: it sends only texture
/// lifecycle operations and the current ordered draw list, never a rebuilt
/// full texture set.  Consumers must validate and prepare the whole packet
/// before committing any resource mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyScenePacketV1 {
    pub width: u32,
    pub height: u32,
    pub resources: Vec<LegacySceneResourceOperationV1>,
    pub draws: Vec<LegacyDrawV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacySceneResourceOperationV1 {
    CreateTexture(LegacySceneTextureCreateV1),
    UpdateTexture(LegacySceneTextureUpdateV1),
    DestroyTexture { texture_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySceneTextureCreateV1 {
    pub texture_id: u32,
    pub width: u32,
    pub height: u32,
    pub format: LegacyTextureFormat,
    pub content_hash: Hash256,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySceneTextureUpdateV1 {
    pub texture_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub format: LegacyTextureFormat,
    pub content_hash: Hash256,
    pub pixels: Vec<u8>,
}

/// Serializable texture metadata used to prepare a scene packet.  It never
/// stores decoded pixels, so it is safe to retain in an adapter snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySceneResourceStateV1 {
    pub textures: BTreeMap<u32, LegacySceneTextureDescriptorV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySceneTextureDescriptorV1 {
    pub width: u32,
    pub height: u32,
    pub format: LegacyTextureFormat,
}

/// A packet that has passed all lifecycle and draw-reference checks.  Hosts
/// must apply its resource operations atomically, then replace their retained
/// metadata with `next_resources`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyPreparedSceneCommitV1 {
    pub packet: LegacyScenePacketV1,
    pub next_resources: LegacySceneResourceStateV1,
}

/// Resource-backed presentation packet. Unlike [`LegacyRenderFrameV1`], this
/// contract never serializes decoded commercial pixels. The host resolves each
/// URI through the active family session, verifies the encoded identity, and
/// decodes it through an explicitly bound media provider before rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyRenderResourceFrameV1 {
    pub width: u32,
    pub height: u32,
    pub texture_resources: Vec<LegacyTextureResourceV1>,
    pub draws: Vec<LegacyDrawV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTextureResourceV1 {
    pub texture_id: u32,
    pub resource_uri: String,
    pub codec: String,
    pub encoded_hash: Hash256,
    pub decoded_width: u32,
    pub decoded_height: u32,
    pub decoded_format: LegacyTextureFormat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTextureUpdateV1 {
    pub texture_id: u32,
    pub width: u32,
    pub height: u32,
    pub format: LegacyTextureFormat,
    pub content_hash: Hash256,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyTextureFormat {
    Rgba8,
    LumaAlpha8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyDrawV1 {
    pub texture_id: u32,
    pub vertices: [LegacyVertexV1; 4],
    pub blend: LegacyBlendMode,
    pub scissor: Option<LegacyScissorV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVertexV1 {
    pub position: [f32; 2],
    pub tex_coord: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyBlendMode {
    Alpha,
    Add,
    Multiply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyScissorV1 {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Deterministic audio intent. Resource commands carry only a VFS URI; encoded
/// commercial bytes are resolved by the host and never enter save/replay or a
/// release report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyAudioCommandV1 {
    LoadResource {
        stream_id: u32,
        encoding: LegacyAudioEncoding,
        resource_uri: String,
    },
    CreateStream {
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        sample_format: LegacyAudioSampleFormat,
    },
    SubmitI16 {
        stream_id: u32,
        samples: Vec<i16>,
    },
    SubmitF32 {
        stream_id: u32,
        samples: Vec<f32>,
    },
    Play {
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    },
    Stop {
        stream_id: u32,
        fade_ms: u32,
    },
    Pause {
        stream_id: u32,
    },
    Resume {
        stream_id: u32,
    },
    SetParams {
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
    },
    DestroyStream {
        stream_id: u32,
    },
    MasterVolume {
        volume: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyAudioEncoding {
    Unknown,
    Wav,
    Ogg,
    Mp3,
    Flac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyAudioSampleFormat {
    I16,
    F32,
}

/// Host-decoded movie intent. The deterministic effect contains only a VFS URI;
/// encoded commercial media remains behind the host VFS boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyVideoCommandV1 {
    Play {
        playback_id: String,
        resource_uri: String,
        mode: LegacyVideoMode,
        stage_width: u32,
        stage_height: u32,
    },
    Stop {
        playback_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyVideoMode {
    ModalWithAudio,
    LayerNoAudio,
}

impl LegacyVideoCommandV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        let (playback_id, resource_uri, dimensions) = match self {
            Self::Play {
                playback_id,
                resource_uri,
                stage_width,
                stage_height,
                ..
            } => (
                playback_id,
                Some(resource_uri),
                Some((*stage_width, *stage_height)),
            ),
            Self::Stop { playback_id } => (playback_id, None, None),
        };
        validate_symbol("video_playback_id", playback_id)?;
        if let Some(uri) = resource_uri {
            validate_vfs_uri(uri)?;
        }
        if dimensions.is_some_and(|(width, height)| {
            !(320..=8192).contains(&width) || !(240..=8192).contains(&height)
        }) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VIDEO_DIMENSIONS",
                "video stage dimensions are outside supported bounds",
            ));
        }
        Ok(())
    }
}

impl LegacyAudioCommandV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        match self {
            Self::LoadResource { resource_uri, .. } => validate_vfs_uri(resource_uri),
            Self::CreateStream {
                sample_rate,
                channels,
                ..
            } if !(8_000..=384_000).contains(sample_rate) || !(1..=8).contains(channels) => {
                Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_AUDIO_FORMAT",
                    "audio stream format is outside supported bounds",
                ))
            }
            Self::SubmitI16 { samples, .. } if samples.len() > MAX_AUDIO_SAMPLES_PER_COMMAND => {
                Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_AUDIO_SAMPLE_BOUNDS",
                    "audio sample command exceeds the bounded buffer",
                ))
            }
            Self::SubmitF32 { samples, .. }
                if samples.len() > MAX_AUDIO_SAMPLES_PER_COMMAND
                    || samples.iter().any(|sample| !sample.is_finite()) =>
            {
                Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_AUDIO_SAMPLE_BOUNDS",
                    "audio samples are invalid or exceed the bounded buffer",
                ))
            }
            Self::Play { volume, pan, .. } | Self::SetParams { volume, pan, .. }
                if !valid_audio_gain(*volume)
                    || !pan.is_finite()
                    || !(-1.0..=1.0).contains(pan) =>
            {
                Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_AUDIO_PARAMS",
                    "audio gain or pan is invalid",
                ))
            }
            Self::MasterVolume { volume } if !valid_audio_gain(*volume) => Err(
                LegacyProviderError::invalid("ASTRA_EMU_AUDIO_PARAMS", "master volume is invalid"),
            ),
            _ => Ok(()),
        }
    }
}

fn valid_audio_gain(value: f32) -> bool {
    value.is_finite() && (0.0..=4.0).contains(&value)
}

fn validate_vfs_uri(value: &str) -> Result<(), LegacyProviderError> {
    let valid_scheme = |scheme: &str| {
        !scheme.is_empty()
            && scheme.len() <= 64
            && scheme.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || (index > 0 && matches!(byte, b'+' | b'-' | b'.'))
            })
    };
    let path = if let Some((scheme, path)) = value.split_once(":/") {
        if !valid_scheme(scheme) || value[scheme.len() + 2..].contains(':') {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_URI",
                "VFS URI has an invalid scheme",
            ));
        }
        path
    } else {
        value
    };
    if value.is_empty()
        || value.len() > 4096
        || path.is_empty()
        || path.starts_with('/')
        || path.contains(':')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_VFS_URI",
            "VFS URI is empty, unsafe, or exceeds bounds",
        ));
    }
    Ok(())
}

impl LegacyRenderFrameV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        if !(1..=8192).contains(&self.width) || !(1..=8192).contains(&self.height) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_RENDER_DIMENSIONS",
                "render dimensions are outside supported bounds",
            ));
        }
        if self.texture_updates.len() > MAX_RENDER_TEXTURE_UPDATES
            || self.draws.len() > MAX_RENDER_DRAWS
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_RENDER_COUNT_BOUNDS",
                "render resource or draw count exceeds bounds",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut bytes = 0usize;
        for update in &self.texture_updates {
            if !ids.insert(update.texture_id) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_RENDER_TEXTURE_DUPLICATE",
                    "a render frame contains duplicate texture updates",
                ));
            }
            let channels = match update.format {
                LegacyTextureFormat::Rgba8 => 4usize,
                LegacyTextureFormat::LumaAlpha8 => 2usize,
            };
            let expected = usize::try_from(update.width)
                .ok()
                .and_then(|width| {
                    usize::try_from(update.height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(channels))
                .ok_or_else(|| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_RENDER_TEXTURE_BOUNDS",
                        "render texture length overflow",
                    )
                })?;
            if expected != update.pixels.len()
                || Hash256::from_sha256(&update.pixels) != update.content_hash
            {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_RENDER_TEXTURE_IDENTITY",
                    "render texture dimensions, bytes, or hash do not match",
                ));
            }
            bytes = bytes.checked_add(expected).ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_RENDER_TEXTURE_BOUNDS",
                    "render upload length overflow",
                )
            })?;
        }
        if bytes > MAX_EFFECT_PAYLOAD_BYTES_PER_STEP {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_RENDER_TEXTURE_BOUNDS",
                "render texture uploads exceed the per-step bound",
            ));
        }
        validate_render_draws(&self.draws)
    }
}

impl LegacyScenePacketV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_render_dimensions_and_counts(
            self.width,
            self.height,
            self.resources.len(),
            self.draws.len(),
        )?;
        let mut bytes = 0usize;
        for operation in &self.resources {
            match operation {
                LegacySceneResourceOperationV1::CreateTexture(texture) => {
                    validate_scene_pixels(
                        texture.width,
                        texture.height,
                        texture.format,
                        &texture.pixels,
                        texture.content_hash,
                    )?;
                    bytes = add_scene_upload_bytes(bytes, texture.pixels.len())?;
                }
                LegacySceneResourceOperationV1::UpdateTexture(texture) => {
                    if texture.width == 0 || texture.height == 0 {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_SCENE_TEXTURE_REGION",
                            "texture update region must be nonempty",
                        ));
                    }
                    validate_scene_pixels(
                        texture.width,
                        texture.height,
                        texture.format,
                        &texture.pixels,
                        texture.content_hash,
                    )?;
                    bytes = add_scene_upload_bytes(bytes, texture.pixels.len())?;
                }
                LegacySceneResourceOperationV1::DestroyTexture { .. } => {}
            }
        }
        validate_render_draws(&self.draws)
    }
}

impl LegacySceneResourceStateV1 {
    /// Validates a complete transaction against retained resource metadata
    /// without changing this state.  The caller chooses when to commit the
    /// returned replacement state, so an invalid packet cannot partially
    /// mutate a renderer or adapter.
    pub fn prepare(
        &self,
        packet: LegacyScenePacketV1,
    ) -> Result<LegacyPreparedSceneCommitV1, LegacyProviderError> {
        packet.validate()?;
        let mut next = self.clone();
        for operation in &packet.resources {
            match operation {
                LegacySceneResourceOperationV1::CreateTexture(texture) => {
                    if next.textures.contains_key(&texture.texture_id) {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_SCENE_TEXTURE_EXISTS",
                            "scene packet creates an existing texture",
                        ));
                    }
                    next.textures.insert(
                        texture.texture_id,
                        LegacySceneTextureDescriptorV1 {
                            width: texture.width,
                            height: texture.height,
                            format: texture.format,
                        },
                    );
                }
                LegacySceneResourceOperationV1::UpdateTexture(texture) => {
                    let descriptor = next.textures.get(&texture.texture_id).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_SCENE_TEXTURE_MISSING",
                            "scene packet updates an unknown texture",
                        )
                    })?;
                    let right = texture.x.checked_add(texture.width);
                    let bottom = texture.y.checked_add(texture.height);
                    if descriptor.format != texture.format
                        || right.is_none_or(|right| right > descriptor.width)
                        || bottom.is_none_or(|bottom| bottom > descriptor.height)
                    {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_SCENE_TEXTURE_REGION",
                            "scene update format or region does not match retained texture",
                        ));
                    }
                }
                LegacySceneResourceOperationV1::DestroyTexture { texture_id } => {
                    if next.textures.remove(texture_id).is_none() {
                        return Err(LegacyProviderError::invalid(
                            "ASTRA_EMU_SCENE_TEXTURE_MISSING",
                            "scene packet destroys an unknown texture",
                        ));
                    }
                }
            }
        }
        for draw in &packet.draws {
            if draw.texture_id != u32::MAX && !next.textures.contains_key(&draw.texture_id) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_SCENE_DRAW_TEXTURE_MISSING",
                    "scene draw references a texture unavailable after commit",
                ));
            }
        }
        Ok(LegacyPreparedSceneCommitV1 {
            packet,
            next_resources: next,
        })
    }

    pub fn commit(&mut self, prepared: LegacyPreparedSceneCommitV1) {
        *self = prepared.next_resources;
    }
}

fn add_scene_upload_bytes(current: usize, bytes: usize) -> Result<usize, LegacyProviderError> {
    let total = current.checked_add(bytes).ok_or_else(|| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_RENDER_TEXTURE_BOUNDS",
            "scene texture upload length overflow",
        )
    })?;
    if total > MAX_EFFECT_PAYLOAD_BYTES_PER_STEP {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_RENDER_TEXTURE_BOUNDS",
            "scene texture uploads exceed the per-step bound",
        ));
    }
    Ok(total)
}

fn validate_scene_pixels(
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    pixels: &[u8],
    content_hash: Hash256,
) -> Result<(), LegacyProviderError> {
    let channels = match format {
        LegacyTextureFormat::Rgba8 => 4usize,
        LegacyTextureFormat::LumaAlpha8 => 2usize,
    };
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_RENDER_TEXTURE_BOUNDS",
                "scene texture length overflow",
            )
        })?;
    if expected != pixels.len() || Hash256::from_sha256(pixels) != content_hash {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_RENDER_TEXTURE_IDENTITY",
            "scene texture dimensions, bytes, or hash do not match",
        ));
    }
    Ok(())
}

impl LegacyRenderResourceFrameV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_render_dimensions_and_counts(
            self.width,
            self.height,
            self.texture_resources.len(),
            self.draws.len(),
        )?;
        let mut ids = BTreeSet::new();
        for resource in &self.texture_resources {
            if !ids.insert(resource.texture_id) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_RENDER_TEXTURE_DUPLICATE",
                    "a resource frame contains duplicate texture resources",
                ));
            }
            validate_vfs_uri(&resource.resource_uri)?;
            if resource.codec.is_empty()
                || resource.codec.len() > 32
                || resource.codec.bytes().any(|byte| {
                    !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-'
                })
                || !(1..=16_384).contains(&resource.decoded_width)
                || !(1..=16_384).contains(&resource.decoded_height)
            {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_RENDER_RESOURCE_DESCRIPTOR",
                    "render resource codec or decoded dimensions are invalid",
                ));
            }
        }
        if self
            .draws
            .iter()
            .any(|draw| !ids.contains(&draw.texture_id))
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_RENDER_TEXTURE_MISSING",
                "a resource frame draw references an undeclared texture",
            ));
        }
        validate_render_draws(&self.draws)
    }
}

fn validate_render_dimensions_and_counts(
    width: u32,
    height: u32,
    texture_count: usize,
    draw_count: usize,
) -> Result<(), LegacyProviderError> {
    if !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_RENDER_DIMENSIONS",
            "render dimensions are outside supported bounds",
        ));
    }
    if texture_count > MAX_RENDER_TEXTURE_UPDATES || draw_count > MAX_RENDER_DRAWS {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_RENDER_COUNT_BOUNDS",
            "render resource or draw count exceeds bounds",
        ));
    }
    Ok(())
}

fn validate_render_draws(draws: &[LegacyDrawV1]) -> Result<(), LegacyProviderError> {
    for draw in draws {
        if draw
            .vertices
            .iter()
            .flat_map(|vertex| {
                vertex
                    .position
                    .iter()
                    .chain(&vertex.tex_coord)
                    .chain(&vertex.color)
            })
            .any(|value| !value.is_finite())
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_RENDER_VERTEX_INVALID",
                "render vertices must contain only finite values",
            ));
        }
        if let Some(scissor) = draw.scissor {
            if scissor.x < 0 || scissor.y < 0 || scissor.width <= 0 || scissor.height <= 0 {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_RENDER_SCISSOR_INVALID",
                    "render scissor must be positive and within the stage",
                ));
            }
        }
    }
    Ok(())
}

impl LegacyEffect {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::RuntimeEvent { sequence, .. }
            | Self::Presentation { sequence, .. }
            | Self::Audio { sequence, .. }
            | Self::TextCapture { sequence, .. }
            | Self::SetBlackboard { sequence, .. }
            | Self::ScheduleEvent { sequence, .. }
            | Self::SnapshotDirty { sequence, .. } => *sequence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyWaitRequest {
    Frame {
        token_id: String,
        frames: u32,
    },
    Time {
        token_id: String,
        milliseconds: u32,
    },
    Input {
        token_id: String,
        mask: u64,
    },
    MediaFence {
        token_id: String,
        media_id: String,
    },
    PresentationFence {
        token_id: String,
        fence_id: String,
    },
    ProviderCompletion {
        token_id: String,
        request_id: String,
    },
    FamilyOpaque {
        token_id: String,
        wait_kind: String,
        payload_hash: Hash256,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTraceEntry {
    pub sequence: u64,
    pub context_id: u32,
    pub pc: u64,
    pub opcode: String,
    pub action: Option<String>,
    pub yield_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyDiagnostic {
    pub code: String,
    pub severity: String,
    pub subject: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyCoverageDelta {
    pub instructions: u64,
    pub syscalls: u64,
    pub contexts: Vec<u32>,
    pub presentation_commands: u64,
    pub audio_commands: u64,
    pub text_events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySnapshotSection {
    pub section_id: String,
    pub schema: String,
    pub version: SchemaVersion,
    pub hash: Hash256,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySnapshotEnvelope {
    pub family_id: FamilyId,
    pub session_id: LegacyRuntimeSessionId,
    pub schema_version: SchemaVersion,
    pub case_fingerprint: Hash256,
    pub fixed_step: u64,
    pub session_seed: u64,
    pub runtime_cursor: u64,
    pub family_sections: Vec<LegacySnapshotSection>,
    pub redaction_status: String,
}

impl LegacySnapshotEnvelope {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        if self.family_sections.len() > MAX_SNAPSHOT_SECTIONS {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SNAPSHOT_SECTION_COUNT",
                "snapshot section count exceeds the supported bound",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut total = 0usize;
        for section in &self.family_sections {
            validate_symbol("snapshot.section_id", &section.section_id)?;
            validate_symbol("snapshot.schema", &section.schema)?;
            if !ids.insert(section.section_id.as_str()) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_SNAPSHOT_SECTION_DUPLICATE",
                    "snapshot section id is duplicated",
                ));
            }
            if Hash256::from_sha256(&section.bytes) != section.hash {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_SNAPSHOT_SECTION_HASH",
                    "snapshot section hash does not match its bytes",
                ));
            }
            total = total.checked_add(section.bytes.len()).ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_SNAPSHOT_SIZE_OVERFLOW",
                    "snapshot byte count overflowed",
                )
            })?;
        }
        if total > MAX_SNAPSHOT_BYTES {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SNAPSHOT_SIZE",
                "snapshot exceeds the supported byte bound",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyRestoreReport {
    pub restored_fixed_step: u64,
    pub session_seed: u64,
    pub state_hash: Hash256,
    pub diagnostics: Vec<LegacyDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyShutdownReport {
    pub final_state_hash: Hash256,
    pub instruction_count: u64,
    pub syscall_count: u64,
    pub diagnostics: Vec<LegacyDiagnostic>,
}

pub trait LegacyRuntimeProvider: Send {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor;
    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError>;
    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError>;
    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError>;
    fn save(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacySnapshotEnvelope, LegacyProviderError>;
    fn restore(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        snapshot: &LegacySnapshotEnvelope,
    ) -> Result<LegacyRestoreReport, LegacyProviderError>;
    /// Consumes plaintext captured for a `TextCapture` effect. The lease is an
    /// out-of-band, single-use channel: its value is never serializable and
    /// must not enter RuntimeWorld, save/replay, reports, logs, or packages.
    fn take_ephemeral_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<LegacyEphemeralText>, LegacyProviderError>;
    /// Resolves a family-owned virtual resource for a host media service.
    ///
    /// The returned commercial bytes are an ephemeral, bounded host channel.
    /// They must never enter effects, RuntimeWorld, save/replay, reports, logs,
    /// or packages. Archive and virtual-path semantics remain owned by the
    /// family provider instead of being duplicated in Manager hosts.
    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, LegacyProviderError>;
    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyEphemeralText {
    pub lease_id: String,
    pub text: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{code}: {message}")]
pub struct LegacyProviderError {
    code: String,
    message: String,
}

impl LegacyProviderError {
    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub fn remote(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}

pub fn validate_symbol(field: &str, value: &str) -> Result<(), LegacyProviderError> {
    if value.is_empty()
        || value.len() > MAX_SYMBOL_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
    {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_INVALID_SYMBOL",
            format!("{field} is empty, too long, or contains unsupported bytes"),
        ));
    }
    Ok(())
}

fn validate_unique_symbols(field: &str, values: &[String]) -> Result<(), LegacyProviderError> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_symbol(field, value)?;
        if !seen.insert(value.as_str()) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_DUPLICATE_SYMBOL",
                format!("{field} contains a duplicate value"),
            ));
        }
    }
    Ok(())
}

fn validate_sequence(
    field: &str,
    values: impl IntoIterator<Item = u64>,
) -> Result<(), LegacyProviderError> {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|prior| value <= prior) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_STEP_SEQUENCE_ORDER",
                format!("{field} sequence must be strictly increasing"),
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{de::DeserializeOwned, Serialize};

    fn budget() -> LegacyStepBudget {
        LegacyStepBudget {
            max_instructions: 1,
            max_effects: 8,
            max_trace_entries: 8,
        }
    }

    #[test]
    fn video_command_requires_safe_identity_uri_and_stage_bounds() {
        let command = LegacyVideoCommandV1::Play {
            playback_id: "movie.1".into(),
            resource_uri: "movie/opening.mp4".into(),
            mode: LegacyVideoMode::ModalWithAudio,
            stage_width: 1280,
            stage_height: 720,
        };
        command.validate().unwrap();
        assert!(LegacyVideoCommandV1::Play {
            playback_id: "movie.1".into(),
            resource_uri: "../escape.mp4".into(),
            mode: LegacyVideoMode::ModalWithAudio,
            stage_width: 1280,
            stage_height: 720,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn prepared_scene_commit_is_atomic_and_accepts_partial_uploads() {
        let pixels = vec![0, 0, 0, 255, 255, 255, 255, 255];
        let create = LegacyScenePacketV1 {
            width: 640,
            height: 480,
            resources: vec![LegacySceneResourceOperationV1::CreateTexture(
                LegacySceneTextureCreateV1 {
                    texture_id: 7,
                    width: 2,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&pixels),
                    pixels,
                },
            )],
            draws: vec![],
        };
        let mut state = LegacySceneResourceStateV1::default();
        let prepared = state.prepare(create).expect("create packet prepares");
        state.commit(prepared);

        let update_pixels = vec![1, 2, 3, 4];
        let update = LegacyScenePacketV1 {
            width: 640,
            height: 480,
            resources: vec![LegacySceneResourceOperationV1::UpdateTexture(
                LegacySceneTextureUpdateV1 {
                    texture_id: 7,
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&update_pixels),
                    pixels: update_pixels,
                },
            )],
            draws: vec![],
        };
        let prepared = state.prepare(update).expect("partial packet prepares");
        state.commit(prepared);
        assert!(state.textures.contains_key(&7));

        let missing = LegacyScenePacketV1 {
            width: 640,
            height: 480,
            resources: vec![LegacySceneResourceOperationV1::DestroyTexture { texture_id: 99 }],
            draws: vec![],
        };
        assert!(state.prepare(missing).is_err());
        assert!(state.textures.contains_key(&7));
    }

    #[test]
    fn resource_frame_is_uri_bound_and_rejects_undeclared_draw_textures() {
        let draw = LegacyDrawV1 {
            texture_id: 7,
            vertices: [LegacyVertexV1 {
                position: [0.0, 0.0],
                tex_coord: [0.0, 0.0],
                color: [1.0; 4],
            }; 4],
            blend: LegacyBlendMode::Alpha,
            scissor: None,
        };
        let frame = LegacyRenderResourceFrameV1 {
            width: 1280,
            height: 720,
            texture_resources: vec![LegacyTextureResourceV1 {
                texture_id: 7,
                resource_uri: "minori:/bg/title.png".into(),
                codec: "png".into(),
                encoded_hash: Hash256::from_sha256(b"encoded"),
                decoded_width: 1280,
                decoded_height: 720,
                decoded_format: LegacyTextureFormat::Rgba8,
            }],
            draws: vec![draw.clone()],
        };
        frame.validate().unwrap();
        round_trip(&frame);
        let invalid = LegacyRenderResourceFrameV1 {
            texture_resources: Vec::new(),
            draws: vec![draw],
            ..frame
        };
        assert_eq!(
            invalid.validate().unwrap_err().code(),
            "ASTRA_EMU_RENDER_TEXTURE_MISSING"
        );
    }

    #[test]
    fn step_output_rejects_duplicate_or_invalid_waits() {
        let output = LegacyStepOutput {
            status: LegacyRuntimeStatus::Awaiting,
            effects: Vec::new(),
            waits: vec![
                LegacyWaitRequest::MediaFence {
                    token_id: "wait.1".into(),
                    media_id: "movie.1".into(),
                },
                LegacyWaitRequest::Input {
                    token_id: "wait.1".into(),
                    mask: 1,
                },
            ],
            trace: Vec::new(),
            diagnostics: Vec::new(),
            coverage: LegacyCoverageDelta::default(),
            state_hash: Hash256::from_sha256(&[]),
        };
        assert_eq!(
            output.validate(&budget()).unwrap_err().code(),
            "ASTRA_EMU_WAIT_TOKEN_DUPLICATE"
        );
        let invalid = LegacyStepOutput {
            waits: vec![LegacyWaitRequest::Input {
                token_id: "wait.1".into(),
                mask: 0,
            }],
            ..output
        };
        assert_eq!(
            invalid.validate(&budget()).unwrap_err().code(),
            "ASTRA_EMU_WAIT_INPUT_MASK"
        );
    }

    #[test]
    fn every_binary_effect_command_and_wait_variant_round_trips_through_postcard() {
        round_trip(&vec![
            LegacyEffect::RuntimeEvent {
                sequence: 0,
                event: "event.test".into(),
                payload: vec![1],
            },
            LegacyEffect::Presentation {
                sequence: 1,
                command: "present.test".into(),
                payload: vec![2],
            },
            LegacyEffect::Audio {
                sequence: 2,
                command: "audio.test".into(),
                payload: vec![3],
            },
            LegacyEffect::TextCapture {
                sequence: 3,
                lease_id: "lease.test".into(),
                text_hash: Hash256::from_sha256(b"text"),
                byte_len: 4,
                speaker_hash: None,
                source_ref: "source.test".into(),
            },
            LegacyEffect::SetBlackboard {
                sequence: 4,
                key: "key.test".into(),
                value: vec![4],
            },
            LegacyEffect::ScheduleEvent {
                sequence: 5,
                due_tick: 9,
                event: "event.later".into(),
                payload: vec![5],
            },
            LegacyEffect::SnapshotDirty {
                sequence: 6,
                section_id: "section.test".into(),
            },
        ]);
        round_trip(&vec![
            LegacyWaitRequest::Frame {
                token_id: "wait.frame".into(),
                frames: 1,
            },
            LegacyWaitRequest::Time {
                token_id: "wait.time".into(),
                milliseconds: 2,
            },
            LegacyWaitRequest::Input {
                token_id: "wait.input".into(),
                mask: 1,
            },
            LegacyWaitRequest::MediaFence {
                token_id: "wait.media".into(),
                media_id: "media.test".into(),
            },
            LegacyWaitRequest::PresentationFence {
                token_id: "wait.presentation".into(),
                fence_id: "fence.test".into(),
            },
            LegacyWaitRequest::ProviderCompletion {
                token_id: "wait.provider".into(),
                request_id: "request.test".into(),
            },
            LegacyWaitRequest::FamilyOpaque {
                token_id: "wait.opaque".into(),
                wait_kind: "opaque.test".into(),
                payload_hash: Hash256::from_sha256(b"opaque"),
            },
        ]);
        round_trip(&vec![
            LegacyAudioCommandV1::LoadResource {
                stream_id: 1,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: "audio/test.ogg".into(),
            },
            LegacyAudioCommandV1::CreateStream {
                stream_id: 2,
                sample_rate: 48_000,
                channels: 2,
                sample_format: LegacyAudioSampleFormat::F32,
            },
            LegacyAudioCommandV1::SubmitI16 {
                stream_id: 2,
                samples: vec![1, -1],
            },
            LegacyAudioCommandV1::SubmitF32 {
                stream_id: 2,
                samples: vec![0.25, -0.25],
            },
            LegacyAudioCommandV1::Play {
                stream_id: 2,
                volume: 1.0,
                pan: 0.0,
                repeat: false,
                fade_in_ms: 0,
            },
            LegacyAudioCommandV1::Stop {
                stream_id: 2,
                fade_ms: 0,
            },
            LegacyAudioCommandV1::Pause { stream_id: 2 },
            LegacyAudioCommandV1::Resume { stream_id: 2 },
            LegacyAudioCommandV1::SetParams {
                stream_id: 2,
                volume: 0.5,
                pan: -0.5,
                repeat: true,
            },
            LegacyAudioCommandV1::DestroyStream { stream_id: 2 },
            LegacyAudioCommandV1::MasterVolume { volume: 0.75 },
        ]);
        round_trip(&vec![
            LegacyVideoCommandV1::Play {
                playback_id: "movie.test".into(),
                resource_uri: "movie/test.mp4".into(),
                mode: LegacyVideoMode::ModalWithAudio,
                stage_width: 1280,
                stage_height: 720,
            },
            LegacyVideoCommandV1::Stop {
                playback_id: "movie.test".into(),
            },
        ]);
    }

    #[test]
    fn text_presentation_requires_explicit_bounded_font_and_regions() {
        let valid = LegacyTextPresentationV1 {
            layout_id: "family.message".into(),
            language: "ja-JP".into(),
            font_families: vec!["Noto Sans JP".into()],
            body: LegacyTextRegionV1 {
                x: 160,
                y: 568,
                width: 960,
                height: 112,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 3,
            },
            speaker: None,
            rgba: [255, 255, 255, 255],
        };
        valid.validate().unwrap();
        LegacyTextPresentationLeaseV1 {
            lease_id: "lease.test".into(),
            presentation: valid.clone(),
        }
        .validate()
        .unwrap();

        let mut missing_font = valid.clone();
        missing_font.font_families.clear();
        assert_eq!(
            missing_font.validate().unwrap_err().code(),
            "ASTRA_EMU_TEXT_LAYOUT_BINDING"
        );

        let mut transparent = valid.clone();
        transparent.rgba[3] = 0;
        assert_eq!(
            transparent.validate().unwrap_err().code(),
            "ASTRA_EMU_TEXT_LAYOUT_COLOR"
        );

        let mut invalid_region = valid;
        invalid_region.body.line_height = 20.0;
        assert_eq!(
            invalid_region.validate().unwrap_err().code(),
            "ASTRA_EMU_TEXT_LAYOUT_REGION"
        );
    }

    #[test]
    fn media_resource_validation_accepts_stable_family_uris_and_blocks_traversal() {
        LegacyAudioCommandV1::LoadResource {
            stream_id: 1,
            encoding: LegacyAudioEncoding::Ogg,
            resource_uri: "minori:/bgm/theme.ogg".into(),
        }
        .validate()
        .unwrap();
        for resource_uri in [
            "minori:/bgm/../secret.ogg",
            "Minori:/bgm/theme.ogg",
            "minori:/bgm\\theme.ogg",
        ] {
            assert!(LegacyAudioCommandV1::LoadResource {
                stream_id: 1,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: resource_uri.into(),
            }
            .validate()
            .is_err());
        }
    }

    fn round_trip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let bytes = postcard::to_allocvec(value).unwrap();
        let decoded: T = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(&decoded, value);
    }
}
