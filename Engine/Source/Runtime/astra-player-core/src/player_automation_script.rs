use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationScript {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    pub scenario_ref: String,
    #[serde(default)]
    pub expected_routes: Vec<String>,
    #[serde(default)]
    pub steps: Vec<PlayerAutomationStep>,
}

impl PlayerAutomationScript {
    pub fn new(
        target: impl Into<String>,
        profile: impl Into<String>,
        platform: PlayerPlatform,
        package_hash: impl Into<String>,
        scenario_ref: impl Into<String>,
    ) -> Self {
        Self {
            schema: "astra.player_automation_script.v1".to_string(),
            target: target.into(),
            profile: profile.into(),
            platform,
            package_hash: package_hash.into(),
            scenario_ref: scenario_ref.into(),
            expected_routes: Vec::new(),
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationStep {
    pub id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_route_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerPlatform {
    Windows,
    Linux,
    Macos,
    Web,
    Android,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputTranscript {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    #[serde(default)]
    pub events: Vec<PlayerInputEvent>,
    #[serde(default)]
    pub input_consumption: Vec<PlayerInputConsumptionEvidence>,
    #[serde(default)]
    pub visual_regions: Vec<PlayerVisualRegionEvidence>,
    pub audio_meter: PlayerAudioMeterEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_comparison: Option<PlayerVisualComparisonEvidence>,
    #[serde(default)]
    pub runtime_routes: Vec<PlayerRuntimeRouteEvidence>,
    #[serde(default)]
    pub route_coverage: Vec<String>,
}

impl PlayerInputTranscript {
    pub fn hash(&self) -> Result<Hash256, PlayerTranscriptHashError> {
        if !self.audio_meter.peak_dbfs.is_finite() || !self.audio_meter.rms_dbfs.is_finite() {
            return Err(PlayerTranscriptHashError::NonFiniteAudioMeter);
        }
        serde_json::to_vec(self)
            .map(|bytes| Hash256::from_sha256(&bytes))
            .map_err(PlayerTranscriptHashError::Serialization)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerTranscriptHashError {
    #[error("player transcript audio meter contains a non-finite value")]
    NonFiniteAudioMeter,
    #[error("serialize player transcript: {0}")]
    Serialization(serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputEvent {
    pub step_id: String,
    pub source: String,
    pub kind: String,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputConsumptionEvidence {
    pub input_sequence: u64,
    pub player_sequence: u64,
    pub source: String,
    pub kind: String,
    pub trace_event: String,
    pub trace_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerRuntimeRouteEvidence {
    pub input_sequence: u64,
    pub player_sequence: u64,
    pub fixed_step: u64,
    #[serde(default)]
    pub coverage_reached: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state_id: Option<String>,
    #[serde(default)]
    pub pending_choice_ids: Vec<String>,
    #[serde(default)]
    pub terminal_route_ids: Vec<String>,
    pub session_id: String,
    pub trace_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerVisualRegionEvidence {
    pub region_id: String,
    pub before_hash: String,
    pub after_hash: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAudioMeterEvidence {
    pub provider: String,
    pub callback_count: u64,
    pub host_report_hash: String,
    pub sample_count: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerVisualComparisonEvidence {
    pub report_hash: String,
    pub checkpoint_count: u32,
    pub status: PlayerAutomationStatus,
}

pub const PLAYER_PRESENTATION_REPORT_SCHEMA: &str = "astra.player_presentation_report.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPresentationRunIdentity {
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    pub profile_hash: String,
    pub build_fingerprint: String,
    pub session_id: String,
    pub renderer_provider: String,
    pub presentation_path: String,
    pub font_provider_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerPresentationReport {
    pub schema: String,
    pub status: PlayerAutomationStatus,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    pub profile_hash: String,
    pub build_fingerprint: String,
    pub session_id: String,
    pub renderer_provider: String,
    pub presentation_path: String,
    pub font_provider_hash: String,
    pub layout_hash: String,
    pub command_hash: String,
    pub capture_hash: String,
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub changed_pixels: u64,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

impl PlayerPresentationReport {
    pub fn from_live_capture(
        identity: PlayerPresentationRunIdentity,
        layout_hash: Hash256,
        present_command: &PlayerHostCommand,
        capture: &astra_platform::CapturedFrame,
        background: [u8; 4],
    ) -> Result<Self, PlayerPresentationError> {
        validate_presentation_identity(&identity)?;
        let PlayerHostCommand::PresentScene {
            sequence,
            width,
            height,
            commands,
            ..
        } = present_command
        else {
            return Err(PlayerPresentationError::UnsupportedCommand);
        };
        if *sequence == 0
            || *width == 0
            || *height == 0
            || capture.width != *width
            || capture.height != *height
        {
            return Err(PlayerPresentationError::InvalidDimensions);
        }
        let expected_bytes = usize::try_from(capture.width)
            .ok()
            .and_then(|width| {
                usize::try_from(capture.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(PlayerPresentationError::InvalidDimensions)?;
        if capture.rgba8.len() != expected_bytes {
            return Err(PlayerPresentationError::InvalidCaptureLength);
        }
        let changed_pixels = capture
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel.as_slice() != background)
            .count() as u64;
        if changed_pixels == 0 {
            return Err(PlayerPresentationError::NoVisualOutput);
        }
        if commands.is_empty() {
            return Err(PlayerPresentationError::EmptyCommandStream);
        }
        let command_hash = evidence_scene_command_hash(*sequence, *width, *height, commands);
        Ok(Self {
            schema: PLAYER_PRESENTATION_REPORT_SCHEMA.to_string(),
            status: PlayerAutomationStatus::Pass,
            target: identity.target,
            profile: identity.profile,
            platform: identity.platform,
            package_hash: identity.package_hash,
            profile_hash: identity.profile_hash,
            build_fingerprint: identity.build_fingerprint,
            session_id: identity.session_id,
            renderer_provider: identity.renderer_provider,
            presentation_path: identity.presentation_path,
            font_provider_hash: identity.font_provider_hash,
            layout_hash: layout_hash.to_string(),
            command_hash: command_hash.to_string(),
            capture_hash: Hash256::from_sha256(&capture.rgba8).to_string(),
            sequence: *sequence,
            width: capture.width,
            height: capture.height,
            changed_pixels,
            diagnostics: Vec::new(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerPresentationError {
    #[error("player presentation identity is incomplete or unsupported")]
    InvalidIdentity,
    #[error("player presentation capture dimensions or sequence are invalid")]
    InvalidDimensions,
    #[error("player presentation capture byte length is invalid")]
    InvalidCaptureLength,
    #[error("player presentation capture contains no visual output")]
    NoVisualOutput,
    #[error("player presentation command stream is empty")]
    EmptyCommandStream,
    #[error("player presentation evidence requires a PresentScene command")]
    UnsupportedCommand,
}

pub fn evidence_scene_command_hash(
    sequence: u64,
    width: u32,
    height: u32,
    commands: &[astra_media_core::SceneCommand],
) -> Hash256 {
    use astra_media_core::SceneCommand;

    let mut material = Vec::with_capacity(24 + commands.len() * 33);
    material.extend_from_slice(&sequence.to_le_bytes());
    material.extend_from_slice(&width.to_le_bytes());
    material.extend_from_slice(&height.to_le_bytes());
    material.extend_from_slice(&(commands.len() as u64).to_le_bytes());
    for command in commands {
        let (kind, pixels): (u8, Option<&[u8]>) = match command {
            SceneCommand::UploadTexture { frame, .. } => (0, Some(&frame.rgba8)),
            SceneCommand::UpdateTextureRegion { rgba8, .. } => (1, Some(rgba8)),
            SceneCommand::UploadGlyph { glyph, .. } => (2, Some(&glyph.pixels)),
            SceneCommand::ReleaseResource { .. } => (3, None),
            SceneCommand::Sprite { .. } => (4, None),
            SceneCommand::GlyphRun { .. } => (5, None),
            SceneCommand::Mesh2D { .. } => (6, None),
            SceneCommand::MeshBatch2D { .. } => (20, None),
            SceneCommand::Clear { .. } => (7, None),
            SceneCommand::Rect { .. } => (8, None),
            SceneCommand::Texture { frame, .. } => (9, Some(&frame.rgba8)),
            SceneCommand::VideoFrame { frame, .. } => (10, Some(&frame.rgba8)),
            SceneCommand::Glyph { glyph, .. } => (11, Some(&glyph.pixels)),
            SceneCommand::PushClip { .. } => (12, None),
            SceneCommand::PopClip => (13, None),
            SceneCommand::PushTransform { .. } => (14, None),
            SceneCommand::PopTransform => (15, None),
            SceneCommand::SetCamera { .. } => (16, None),
            SceneCommand::PushOpacity { .. } => (17, None),
            SceneCommand::PopOpacity => (18, None),
            SceneCommand::FilterGraph { .. } => (19, None),
            SceneCommand::PushPixelMask { .. } => (21, None),
            SceneCommand::PopPixelMask => (22, None),
        };
        material.push(kind);
        if let Some(pixels) = pixels {
            material.extend_from_slice(Hash256::from_sha256(pixels).as_bytes());
        }
    }
    Hash256::from_sha256(&material)
}

pub(super) fn validate_presentation_identity(
    identity: &PlayerPresentationRunIdentity,
) -> Result<(), PlayerPresentationError> {
    let renderer_matches = match identity.platform {
        PlayerPlatform::Windows => identity.renderer_provider == "wgpu_hardware",
        PlayerPlatform::Linux => identity.renderer_provider == "wgpu_vulkan",
        PlayerPlatform::Macos => identity.renderer_provider == "wgpu_metal",
        PlayerPlatform::Web => identity.renderer_provider == "webgpu",
        PlayerPlatform::Android => identity.renderer_provider == "wgpu_vulkan",
    };
    if identity.target.is_empty()
        || identity.profile.is_empty()
        || !identity.package_hash.starts_with("sha256:")
        || !identity.profile_hash.starts_with("sha256:")
        || !identity.build_fingerprint.starts_with("sha256:")
        || identity.session_id.is_empty()
        || !identity.font_provider_hash.starts_with("sha256:")
        || identity.presentation_path != "glyph_atlas"
        || !renderer_matches
    {
        return Err(PlayerPresentationError::InvalidIdentity);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationReport {
    pub schema: String,
    pub status: PlayerAutomationStatus,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    pub transcript_hash: String,
    #[serde(default)]
    pub route_coverage: Vec<String>,
    #[serde(default)]
    pub checks: Vec<PlayerAutomationCheck>,
}

impl PlayerAutomationReport {
    pub fn full_playable_passed(&self) -> bool {
        self.status == PlayerAutomationStatus::Pass
            && self.checks.iter().any(|check| {
                check.id == "player.full_playable" && check.status == PlayerAutomationStatus::Pass
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAutomationStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationCheck {
    pub id: String,
    pub status: PlayerAutomationStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Diagnostic>,
    #[serde(default)]
    pub evidence: Vec<PlayerAutomationEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationEvidence {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerPlatformEvidenceIdentity {
    pub profile_hash: String,
    pub build_fingerprint: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerAutomationValidator;
