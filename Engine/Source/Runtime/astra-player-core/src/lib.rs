use std::{collections::BTreeSet, future::Future, pin::Pin};

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const WEB_PLAYER_LIVE_EVIDENCE_SCHEMA: &str = "astra.player_web_live_evidence.v1";

mod platform_sink;
pub use astra_media::{PlayerAudioContractError, PlayerDecodedAudio};
pub use platform_sink::*;
mod media_lifecycle;
pub use astra_media::{
    PlayerAudioCompletion, PlayerAudioQueueController, PlayerMixedAudio,
    PlayerPersistentAudioError, PlayerPersistentVoiceSpec,
};
pub use media_lifecycle::*;
mod timeline;
pub use timeline::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlayerAction {
    Advance,
    ChooseIndex { index: usize },
    OpenSystemPage { page: String },
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerActionBinding {
    pub input: String,
    pub action: PlayerAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerActionMap {
    pub schema: String,
    pub bindings: Vec<PlayerActionBinding>,
}

impl PlayerActionMap {
    pub fn standard() -> Self {
        let mut bindings = vec![
            PlayerActionBinding {
                input: "Enter".into(),
                action: PlayerAction::Advance,
            },
            PlayerActionBinding {
                input: "Space".into(),
                action: PlayerAction::Advance,
            },
            PlayerActionBinding {
                input: "Escape".into(),
                action: PlayerAction::Back,
            },
            PlayerActionBinding {
                input: "KeyB".into(),
                action: PlayerAction::OpenSystemPage {
                    page: "backlog".into(),
                },
            },
        ];
        bindings.extend((0..9).map(|index| PlayerActionBinding {
            input: format!("Digit{}", index + 1),
            action: PlayerAction::ChooseIndex { index },
        }));
        Self {
            schema: "astra.player_action_map.v1".into(),
            bindings,
        }
    }

    pub fn keyboard(&self, physical_key: &str) -> Option<PlayerAction> {
        self.bindings
            .iter()
            .find(|binding| binding.input == physical_key)
            .map(|binding| binding.action.clone())
    }
}

/// Logical resource identity used only inside the Player command stream. It is
/// deliberately unrelated to platform handles and may safely cross runtime
/// provider boundaries.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct PlayerHostResourceId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerPackageSource {
    Bundled {
        relative_path: String,
        expected_hash: String,
    },
    UserAuthorized {
        expected_hash: String,
    },
    HttpsRange {
        url: String,
        expected_hash: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerDecodeKind {
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerHostCommand {
    OpenPackage {
        sequence: u64,
        source: PlayerPackageSource,
        package: PlayerHostResourceId,
    },
    ReadPackageRange {
        sequence: u64,
        package: PlayerHostResourceId,
        offset: u64,
        length: u32,
    },
    ClosePackage {
        sequence: u64,
        package: PlayerHostResourceId,
    },
    BeginSave {
        sequence: u64,
        slot: String,
        transaction: PlayerHostResourceId,
    },
    WriteSave {
        sequence: u64,
        transaction: PlayerHostResourceId,
        bytes: Vec<u8>,
    },
    CommitSave {
        sequence: u64,
        transaction: PlayerHostResourceId,
    },
    AbortSave {
        sequence: u64,
        transaction: PlayerHostResourceId,
    },
    ReadSave {
        sequence: u64,
        slot: String,
    },
    ListSaves {
        sequence: u64,
    },
    DeleteSave {
        sequence: u64,
        slot: String,
    },
    OpenAudio {
        sequence: u64,
        output: PlayerHostResourceId,
        sample_rate: u32,
        channels: u16,
        max_buffered_frames: u32,
    },
    QueryAudioFormat {
        sequence: u64,
    },
    SubmitAudio {
        sequence: u64,
        output: PlayerHostResourceId,
        packet_sequence: u64,
        channels: u16,
        samples: Vec<f32>,
    },
    QueryAudio {
        sequence: u64,
        output: PlayerHostResourceId,
    },
    DrainAudio {
        sequence: u64,
        output: PlayerHostResourceId,
    },
    CloseAudio {
        sequence: u64,
        output: PlayerHostResourceId,
    },
    OpenDecode {
        sequence: u64,
        session: PlayerHostResourceId,
        kind: PlayerDecodeKind,
    },
    Decode {
        sequence: u64,
        request_sequence: u64,
        session: PlayerHostResourceId,
        kind: PlayerDecodeKind,
        codec: String,
        description: Vec<u8>,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        coded_width: Option<u32>,
        coded_height: Option<u32>,
        keyframe: bool,
        bytes: Vec<u8>,
    },
    CloseDecode {
        sequence: u64,
        session: PlayerHostResourceId,
    },
    PresentRgba {
        sequence: u64,
        surface: PlayerHostResourceId,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    },
    PresentScene {
        sequence: u64,
        surface: PlayerHostResourceId,
        width: u32,
        height: u32,
        clear_rgba: [u8; 4],
        commands: Vec<astra_media_core::SceneCommand>,
        semantics: Option<astra_ui_core::UiSemanticSnapshot>,
    },
    CaptureSurface {
        sequence: u64,
        surface: PlayerHostResourceId,
    },
}

impl PlayerHostCommand {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::OpenPackage { sequence, .. }
            | Self::ReadPackageRange { sequence, .. }
            | Self::ClosePackage { sequence, .. }
            | Self::BeginSave { sequence, .. }
            | Self::WriteSave { sequence, .. }
            | Self::CommitSave { sequence, .. }
            | Self::AbortSave { sequence, .. }
            | Self::ReadSave { sequence, .. }
            | Self::ListSaves { sequence }
            | Self::DeleteSave { sequence, .. }
            | Self::OpenAudio { sequence, .. }
            | Self::QueryAudioFormat { sequence }
            | Self::SubmitAudio { sequence, .. }
            | Self::QueryAudio { sequence, .. }
            | Self::DrainAudio { sequence, .. }
            | Self::CloseAudio { sequence, .. }
            | Self::OpenDecode { sequence, .. }
            | Self::Decode { sequence, .. }
            | Self::CloseDecode { sequence, .. }
            | Self::PresentRgba { sequence, .. }
            | Self::PresentScene { sequence, .. }
            | Self::CaptureSurface { sequence, .. } => *sequence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerHostCommandBatch {
    pub commands: Vec<PlayerHostCommand>,
}

impl PlayerHostCommandBatch {
    pub fn new(commands: Vec<PlayerHostCommand>) -> Result<Self, PlayerHostCommandError> {
        let mut previous = 0_u64;
        for command in &commands {
            let sequence = command.sequence();
            if sequence == 0 || sequence <= previous {
                return Err(PlayerHostCommandError::SequenceNotStrictlyIncreasing);
            }
            previous = sequence;
        }
        Ok(Self { commands })
    }
}

pub trait PlayerHostCommandSource {
    fn take_host_commands(&mut self) -> Result<PlayerHostCommandBatch, PlayerHostCommandError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerHostCommandResult {
    PackageOpened {
        package: PlayerHostResourceId,
    },
    PackageRange {
        package: PlayerHostResourceId,
        bytes: Vec<u8>,
    },
    PackageClosed {
        package: PlayerHostResourceId,
    },
    SaveStarted {
        transaction: PlayerHostResourceId,
    },
    SaveCommitted {
        transaction: PlayerHostResourceId,
        hash: String,
    },
    SaveRead {
        bytes: Vec<u8>,
    },
    SaveList {
        slots: Vec<String>,
    },
    AudioOpened {
        output: PlayerHostResourceId,
    },
    AudioFormat {
        sample_rate: u32,
        channels: u16,
    },
    AudioState {
        output: PlayerHostResourceId,
        queued_frames: u64,
        callback_count: u64,
        submitted_samples: u64,
        consumed_samples: u64,
        underflow_count: u64,
        peak_dbfs_bits: u32,
        rms_dbfs_bits: u32,
    },
    AudioDrained {
        output: PlayerHostResourceId,
        sample_count: u64,
        peak_dbfs_bits: u32,
        rms_dbfs_bits: u32,
    },
    AudioClosed {
        output: PlayerHostResourceId,
    },
    DecodeOpened {
        session: PlayerHostResourceId,
    },
    Decoded {
        session: PlayerHostResourceId,
        format: String,
        hash: String,
        bytes: Vec<u8>,
    },
    DecodeClosed {
        session: PlayerHostResourceId,
    },
    Presented {
        surface: PlayerHostResourceId,
    },
    Captured {
        surface: PlayerHostResourceId,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    },
    Unit,
}

pub trait PlayerHostCommandSink {
    type Error;

    fn execute<'a>(
        &'a mut self,
        command: &'a PlayerHostCommand,
    ) -> Pin<Box<dyn Future<Output = Result<PlayerHostCommandResult, Self::Error>> + 'a>>;
}

pub struct PlayerHostCommandExecutor<S> {
    sink: S,
    last_sequence: u64,
}

pub struct PlayerSaveTransactionPlan {
    pub begin: PlayerHostCommandBatch,
    pub write: PlayerHostCommandBatch,
    pub commit: PlayerHostCommandBatch,
    pub abort: PlayerHostCommandBatch,
}

#[derive(Debug)]
pub enum PlayerSaveTransactionError<E> {
    Begin(PlayerHostCommandExecutionError<E>),
    Write {
        source: PlayerHostCommandExecutionError<E>,
        abort: Option<PlayerHostCommandExecutionError<E>>,
    },
    Commit {
        source: PlayerHostCommandExecutionError<E>,
        abort: Option<PlayerHostCommandExecutionError<E>>,
    },
}

impl<E: std::fmt::Display> std::fmt::Display for PlayerSaveTransactionError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Begin(error) => write!(formatter, "save begin failed: {error}"),
            Self::Write { source, abort } => {
                write_transaction_failure(formatter, "save write", source, abort.as_ref())
            }
            Self::Commit { source, abort } => {
                write_transaction_failure(formatter, "save commit", source, abort.as_ref())
            }
        }
    }
}

fn write_transaction_failure<E: std::fmt::Display>(
    formatter: &mut std::fmt::Formatter<'_>,
    operation: &str,
    source: &PlayerHostCommandExecutionError<E>,
    abort: Option<&PlayerHostCommandExecutionError<E>>,
) -> std::fmt::Result {
    write!(formatter, "{operation} failed: {source}")?;
    if let Some(abort) = abort {
        write!(formatter, "; save abort also failed: {abort}")?;
    }
    Ok(())
}

impl<E: std::error::Error + 'static> std::error::Error for PlayerSaveTransactionError<E> {}

impl<S> PlayerHostCommandExecutor<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            last_sequence: 0,
        }
    }
    pub fn sink(&self) -> &S {
        &self.sink
    }
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }
}

impl<S: PlayerHostCommandSink> PlayerHostCommandExecutor<S> {
    pub async fn execute_batch(
        &mut self,
        batch: PlayerHostCommandBatch,
    ) -> Result<Vec<PlayerHostCommandResult>, PlayerHostCommandExecutionError<S::Error>> {
        let mut results = Vec::with_capacity(batch.commands.len());
        for command in &batch.commands {
            if command.sequence() <= self.last_sequence {
                return Err(PlayerHostCommandExecutionError::SequenceNotStrictlyIncreasing);
            }
            let result = self
                .sink
                .execute(command)
                .await
                .map_err(PlayerHostCommandExecutionError::Sink)?;
            self.last_sequence = command.sequence();
            results.push(result);
        }
        Ok(results)
    }

    pub async fn execute_save_transaction(
        &mut self,
        plan: PlayerSaveTransactionPlan,
    ) -> Result<(), PlayerSaveTransactionError<S::Error>> {
        self.execute_batch(plan.begin)
            .await
            .map_err(PlayerSaveTransactionError::Begin)?;
        if let Err(source) = self.execute_batch(plan.write).await {
            let abort = self.execute_batch(plan.abort).await.err();
            return Err(PlayerSaveTransactionError::Write { source, abort });
        }
        if let Err(source) = self.execute_batch(plan.commit).await {
            let abort = self.execute_batch(plan.abort).await.err();
            return Err(PlayerSaveTransactionError::Commit { source, abort });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerHostCommandExecutionError<E> {
    SequenceNotStrictlyIncreasing,
    Sink(E),
}

impl<E: std::fmt::Display> std::fmt::Display for PlayerHostCommandExecutionError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SequenceNotStrictlyIncreasing => {
                formatter.write_str("Player host command sequence is not strictly increasing")
            }
            Self::Sink(error) => write!(formatter, "Player host command failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PlayerHostCommandExecutionError<E> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerHostCommandError {
    SequenceNotStrictlyIncreasing,
}

impl std::fmt::Display for PlayerHostCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SequenceNotStrictlyIncreasing => {
                formatter.write_str("player host command sequence is not strictly increasing")
            }
        }
    }
}
impl std::error::Error for PlayerHostCommandError {}

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
    pub runtime_state_hash: String,
    pub runtime_event_hash: String,
    pub runtime_presentation_hash: String,
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
            .chunks_exact(4)
            .filter(|pixel| *pixel != background)
            .count() as u64;
        if changed_pixels == 0 {
            return Err(PlayerPresentationError::NoVisualOutput);
        }
        let command_bytes = serde_json::to_vec(present_command)
            .map_err(PlayerPresentationError::CommandSerialization)?;
        if commands.is_empty() {
            return Err(PlayerPresentationError::EmptyCommandStream);
        }
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
            command_hash: Hash256::from_sha256(&command_bytes).to_string(),
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
    #[error("player presentation command stream is not serializable: {0}")]
    CommandSerialization(serde_json::Error),
}

fn validate_presentation_identity(
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

impl PlayerAutomationValidator {
    pub fn validate(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        self.validate_internal(script, transcript, None)
    }

    pub fn validate_with_platform_identity(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
        identity: &PlayerPlatformEvidenceIdentity,
    ) -> PlayerAutomationReport {
        self.validate_internal(script, transcript, Some(identity))
    }

    fn validate_internal(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
        identity: Option<&PlayerPlatformEvidenceIdentity>,
    ) -> PlayerAutomationReport {
        tracing::info!(
            event = "player.automation.validate.start",
            platform = ?script.platform,
            expected_route_count = script.expected_routes.len(),
            input_event_count = transcript.events.len(),
            "player automation validation started"
        );
        let (transcript_hash, serialization_check) = match transcript.hash() {
            Ok(hash) => (
                hash.to_string(),
                pass_check(
                    "player.transcript_serialization",
                    "player transcript has a canonical JSON identity",
                    vec![evidence("transcript_hash", hash)],
                ),
            ),
            Err(_) => (
                String::new(),
                blocked_check(
                    "player.transcript_serialization",
                    "player transcript could not be serialized without data loss",
                    "ASTRA_PLAYER_TRANSCRIPT_SERIALIZATION",
                ),
            ),
        };
        let mut checks = vec![
            serialization_check,
            schema_check(script, transcript),
            identity_check(script, transcript),
            live_input_surface_check(script.platform, &transcript.events),
            input_consumption_trace_check(
                script.platform,
                &transcript.events,
                &transcript.input_consumption,
            ),
            transcript_coverage_check(script, transcript),
            visual_region_check(&transcript.visual_regions),
            visual_comparison_check(transcript.visual_comparison.as_ref()),
            audio_meter_check(transcript.platform, &transcript.audio_meter),
            runtime_route_evidence_check(script, transcript),
            route_coverage_check(script, transcript),
        ];
        if let Some(identity) = identity {
            checks.push(platform_identity_check(identity));
        }
        let full_playable = if checks
            .iter()
            .all(|check| check.status == PlayerAutomationStatus::Pass)
        {
            let mut full_evidence = vec![
                evidence("transcript_hash", &transcript_hash),
                evidence("route_count", transcript.route_coverage.len()),
            ];
            if let Some(identity) = identity {
                full_evidence.extend([
                    evidence("profile_hash", &identity.profile_hash),
                    evidence("build_fingerprint", &identity.build_fingerprint),
                    evidence("session_id", &identity.session_id),
                ]);
            }
            pass_check(
                "player.full_playable",
                "live player automation covered route, visual and audio evidence",
                full_evidence,
            )
        } else {
            blocked_check(
                "player.full_playable",
                "live player automation evidence is incomplete or unsafe",
                "ASTRA_PLAYER_FULL_PLAYABLE_BLOCKED",
            )
        };
        checks.push(full_playable);
        let status = if checks
            .iter()
            .any(|check| check.status == PlayerAutomationStatus::Blocked)
        {
            PlayerAutomationStatus::Blocked
        } else {
            PlayerAutomationStatus::Pass
        };
        let report = PlayerAutomationReport {
            schema: "astra.player_automation_report.v1".to_string(),
            status,
            target: script.target.clone(),
            profile: script.profile.clone(),
            platform: script.platform,
            package_hash: script.package_hash.clone(),
            transcript_hash,
            route_coverage: transcript.route_coverage.clone(),
            checks,
        };
        match report.status {
            PlayerAutomationStatus::Pass => tracing::info!(
                event = "player.automation.validate.complete",
                status = "pass",
                check_count = report.checks.len(),
                route_count = report.route_coverage.len(),
                "player automation validation completed"
            ),
            PlayerAutomationStatus::Blocked => tracing::error!(
                event = "player.automation.validate.complete",
                status = "blocked",
                check_count = report.checks.len(),
                route_count = report.route_coverage.len(),
                "player automation validation blocked"
            ),
        }
        report
    }
}

fn platform_identity_check(identity: &PlayerPlatformEvidenceIdentity) -> PlayerAutomationCheck {
    if identity.profile_hash.starts_with("sha256:")
        && identity.build_fingerprint.starts_with("sha256:")
        && !identity.session_id.is_empty()
    {
        pass_check(
            "player.platform_identity",
            "player evidence is bound to the host session",
            vec![
                evidence("profile_hash", &identity.profile_hash),
                evidence("build_fingerprint", &identity.build_fingerprint),
                evidence("session_id", &identity.session_id),
            ],
        )
    } else {
        blocked_check(
            "player.platform_identity",
            "player platform identity is incomplete",
            "ASTRA_PLAYER_PLATFORM_IDENTITY",
        )
    }
}

fn schema_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if script.schema == "astra.player_automation_script.v1"
        && transcript.schema == "astra.player_input_transcript.v2"
        && is_safe_relative_ref(&script.scenario_ref)
    {
        pass_check(
            "player.automation_schema",
            "automation script and transcript schemas are valid",
            vec![evidence("scenario_ref", &script.scenario_ref)],
        )
    } else {
        blocked_check(
            "player.automation_schema",
            "automation script or transcript schema is invalid",
            "ASTRA_PLAYER_AUTOMATION_SCHEMA",
        )
    }
}

fn identity_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if script.target == transcript.target
        && script.profile == transcript.profile
        && script.platform == transcript.platform
        && script.package_hash == transcript.package_hash
        && script.package_hash.starts_with("sha256:")
    {
        pass_check(
            "player.package_identity",
            "transcript matches package, target, profile and platform",
            vec![
                evidence("target", &script.target),
                evidence("profile", &script.profile),
                evidence("package_hash", &script.package_hash),
            ],
        )
    } else {
        blocked_check(
            "player.package_identity",
            "transcript identity does not match script",
            "ASTRA_PLAYER_PACKAGE_IDENTITY",
        )
    }
}

fn live_input_surface_check(
    platform: PlayerPlatform,
    events: &[PlayerInputEvent],
) -> PlayerAutomationCheck {
    let mut forbidden = Vec::new();
    for event in events {
        if is_forbidden_input_source(&event.source) {
            forbidden.push(event.source.clone());
        }
    }
    if !forbidden.is_empty() {
        forbidden.sort();
        forbidden.dedup();
        return blocked_check_with_evidence(
            "player.live_input_surface",
            "transcript used a forbidden non-live input surface",
            "ASTRA_PLAYER_FORBIDDEN_INPUT_SURFACE",
            vec![evidence("forbidden_source", forbidden.join(","))],
        );
    }

    let has_required = events.iter().any(|event| match platform {
        PlayerPlatform::Windows => matches!(
            event.source.as_str(),
            "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Linux => matches!(
            event.source.as_str(),
            "uinput.keyboard" | "uinput.mouse" | "uinput.touch" | "uinput.gamepad"
        ),
        PlayerPlatform::Macos => matches!(
            event.source.as_str(),
            "cgevent.keyboard" | "cgevent.mouse" | "cgevent.gamepad"
        ),
        PlayerPlatform::Web => matches!(event.source.as_str(), "cdp.mouse" | "cdp.keyboard"),
        PlayerPlatform::Android => matches!(
            event.source.as_str(),
            "android.touch" | "android.keyboard" | "android.gamepad" | "android.accessibility"
        ),
    });
    let all_allowed = events.iter().all(|event| match platform {
        PlayerPlatform::Windows => matches!(
            event.source.as_str(),
            "window.focus" | "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Linux => matches!(
            event.source.as_str(),
            "window.focus" | "uinput.keyboard" | "uinput.mouse" | "uinput.touch" | "uinput.gamepad"
        ),
        PlayerPlatform::Macos => matches!(
            event.source.as_str(),
            "window.focus" | "cgevent.keyboard" | "cgevent.mouse" | "cgevent.gamepad"
        ),
        PlayerPlatform::Web => matches!(
            event.source.as_str(),
            "browser.focus" | "cdp.session" | "cdp.mouse" | "cdp.keyboard"
        ),
        PlayerPlatform::Android => matches!(
            event.source.as_str(),
            "android.focus"
                | "android.touch"
                | "android.keyboard"
                | "android.gamepad"
                | "android.accessibility"
        ),
    });
    if !events.is_empty() && has_required && all_allowed {
        pass_check(
            "player.live_input_surface",
            "transcript uses the required live input surface",
            vec![evidence("event_count", events.len())],
        )
    } else {
        blocked_check(
            "player.live_input_surface",
            "transcript does not prove live player input",
            "ASTRA_PLAYER_LIVE_INPUT_MISSING",
        )
    }
}

fn input_consumption_trace_check(
    platform: PlayerPlatform,
    events: &[PlayerInputEvent],
    consumption: &[PlayerInputConsumptionEvidence],
) -> PlayerAutomationCheck {
    let live_inputs = events
        .iter()
        .filter(|event| is_live_input_source(platform, &event.source))
        .collect::<Vec<_>>();
    let mut missing = 0usize;
    let mut invalid = 0usize;
    let mut consumed_sequences = BTreeSet::new();
    for evidence in consumption {
        if evidence.trace_event != "astra.player.input.consumed"
            || !is_consumption_trace_source(platform, &evidence.source)
            || !evidence.trace_hash.starts_with("sha256:")
            || evidence.input_sequence == 0
            || evidence.player_sequence == 0
            || evidence.kind.trim().is_empty()
        {
            invalid += 1;
            continue;
        }
        consumed_sequences.insert(evidence.input_sequence);
    }
    for event in &live_inputs {
        if !consumed_sequences.contains(&event.sequence) {
            missing += 1;
        }
    }

    if !live_inputs.is_empty() && missing == 0 && invalid == 0 {
        pass_check(
            "player.input_consumption_trace",
            "player host trace proves live input was consumed",
            vec![
                evidence("live_input_count", live_inputs.len()),
                evidence("consumed_trace_count", consumption.len()),
            ],
        )
    } else {
        blocked_check_with_evidence(
            "player.input_consumption_trace",
            "player host trace does not prove live input consumption",
            "ASTRA_PLAYER_INPUT_CONSUMPTION_TRACE_MISSING",
            vec![
                evidence("live_input_count", live_inputs.len()),
                evidence("missing_consumption_count", missing),
                evidence("invalid_consumption_count", invalid),
            ],
        )
    }
}

fn runtime_route_evidence_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if transcript.runtime_routes.is_empty() {
        return blocked_check(
            "player.runtime_route_evidence",
            "route coverage has no Runtime/provider evidence",
            "ASTRA_PLAYER_RUNTIME_ROUTE_EVIDENCE_MISSING",
        );
    }
    let consumed = transcript
        .input_consumption
        .iter()
        .map(|item| (item.input_sequence, item.player_sequence))
        .collect::<BTreeSet<_>>();
    let mut actual_routes = BTreeSet::new();
    let mut terminal_routes = BTreeSet::new();
    let mut invalid = 0usize;
    let mut previous_step = 0_u64;
    for route in &transcript.runtime_routes {
        let hashes_valid = route.runtime_state_hash.starts_with("hash128:")
            && route.runtime_event_hash.starts_with("hash128:")
            && route.runtime_presentation_hash.starts_with("hash128:")
            && route.trace_hash.starts_with("sha256:");
        let identity_valid = route.input_sequence > 0
            && route.player_sequence > 0
            && route.fixed_step > previous_step
            && consumed.contains(&(route.input_sequence, route.player_sequence));
        if !hashes_valid || !identity_valid {
            invalid += 1;
            continue;
        }
        previous_step = route.fixed_step;
        actual_routes.extend(route.coverage_reached.iter().cloned());
        terminal_routes.extend(route.terminal_route_ids.iter().cloned());
    }
    let declared_routes = transcript
        .route_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected_routes = script
        .expected_routes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if invalid == 0
        && !actual_routes.is_empty()
        && !terminal_routes.is_empty()
        && terminal_routes.is_subset(&actual_routes)
        && actual_routes == declared_routes
        && expected_routes.is_subset(&actual_routes)
    {
        pass_check(
            "player.runtime_route_evidence",
            "Runtime/provider traces prove route coverage",
            vec![
                evidence("runtime_step_count", transcript.runtime_routes.len()),
                evidence("runtime_route_count", actual_routes.len()),
                evidence("terminal_route_count", terminal_routes.len()),
            ],
        )
    } else {
        blocked_check_with_evidence(
            "player.runtime_route_evidence",
            "Runtime/provider traces do not match declared and expected routes",
            "ASTRA_PLAYER_RUNTIME_ROUTE_EVIDENCE_INVALID",
            vec![
                evidence("invalid_runtime_step_count", invalid),
                evidence("runtime_route_count", actual_routes.len()),
                evidence("terminal_route_count", terminal_routes.len()),
                evidence("declared_route_count", declared_routes.len()),
                evidence("expected_route_count", expected_routes.len()),
            ],
        )
    }
}

fn transcript_coverage_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    let expected_steps = script.steps.len().max(1);
    if transcript.events.len() >= expected_steps
        && transcript
            .events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    {
        pass_check(
            "player.input_transcript",
            "input transcript contains ordered live input events",
            vec![evidence("event_count", transcript.events.len())],
        )
    } else {
        blocked_check(
            "player.input_transcript",
            "input transcript is empty, incomplete or unordered",
            "ASTRA_PLAYER_TRANSCRIPT_INCOMPLETE",
        )
    }
}

fn visual_region_check(regions: &[PlayerVisualRegionEvidence]) -> PlayerAutomationCheck {
    let changed_regions = regions
        .iter()
        .filter(|region| {
            region.width > 0
                && region.height > 0
                && region.before_hash.starts_with("sha256:")
                && region.after_hash.starts_with("sha256:")
                && region.before_hash != region.after_hash
        })
        .count();
    if changed_regions > 0 {
        pass_check(
            "player.visual_region_hash",
            "visual region hash changed after live input",
            vec![evidence("changed_region_count", changed_regions)],
        )
    } else {
        blocked_check(
            "player.visual_region_hash",
            "visual region hash did not prove player-visible state change",
            "ASTRA_PLAYER_VISUAL_REGION_MISSING",
        )
    }
}

fn audio_meter_check(
    platform: PlayerPlatform,
    meter: &PlayerAudioMeterEvidence,
) -> PlayerAutomationCheck {
    let provider_matches = match platform {
        PlayerPlatform::Windows => meter.provider == "wasapi",
        PlayerPlatform::Web => meter.provider == "webaudio",
        PlayerPlatform::Linux => matches!(meter.provider.as_str(), "alsa" | "pipewire"),
        PlayerPlatform::Macos => meter.provider == "coreaudio",
        PlayerPlatform::Android => {
            matches!(meter.provider.as_str(), "oboe_aaudio" | "oboe_opensl_es")
        }
    };
    if provider_matches
        && meter.callback_count > 0
        && meter.host_report_hash.starts_with("sha256:")
        && meter.sample_count > 0
        && meter.peak_dbfs > -80.0
        && meter.rms_dbfs.is_finite()
    {
        pass_check(
            "player.audio_meter",
            "audio meter recorded non-silent output",
            vec![
                evidence("sample_count", meter.sample_count),
                evidence("callback_count", meter.callback_count),
                evidence("provider", &meter.provider),
                evidence("host_report_hash", &meter.host_report_hash),
                evidence("peak_dbfs", format!("{:.2}", meter.peak_dbfs)),
            ],
        )
    } else {
        blocked_check(
            "player.audio_meter",
            "audio meter did not prove non-silent playback",
            "ASTRA_PLAYER_AUDIO_METER_MISSING",
        )
    }
}

fn visual_comparison_check(
    comparison: Option<&PlayerVisualComparisonEvidence>,
) -> PlayerAutomationCheck {
    let Some(comparison) = comparison else {
        return blocked_check(
            "player.visual_comparison",
            "visual comparison evidence is missing",
            "ASTRA_PLAYER_VISUAL_COMPARISON_MISSING",
        );
    };
    if comparison.status == PlayerAutomationStatus::Pass
        && comparison.report_hash.starts_with("sha256:")
        && comparison.checkpoint_count > 0
    {
        pass_check(
            "player.visual_comparison",
            "visual comparison report passed required checkpoints",
            vec![
                evidence("visual_comparison_report_hash", &comparison.report_hash),
                evidence("checkpoint_count", comparison.checkpoint_count),
            ],
        )
    } else {
        blocked_check(
            "player.visual_comparison",
            "visual comparison evidence did not pass required checkpoints",
            "ASTRA_PLAYER_VISUAL_COMPARISON_BLOCKED",
        )
    }
}

fn route_coverage_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    let expected = expected_routes(script);
    let covered = transcript
        .route_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing = expected
        .iter()
        .filter(|route| !covered.contains(*route))
        .cloned()
        .collect::<Vec<_>>();
    if !expected.is_empty() && missing.is_empty() {
        pass_check(
            "player.route_coverage",
            "transcript covered all expected route ids",
            vec![evidence("route_count", covered.len())],
        )
    } else {
        blocked_check_with_evidence(
            "player.route_coverage",
            "transcript did not cover all expected route ids",
            "ASTRA_PLAYER_ROUTE_COVERAGE_MISSING",
            vec![evidence("missing_route_count", missing.len())],
        )
    }
}

fn expected_routes(script: &PlayerAutomationScript) -> BTreeSet<String> {
    let mut routes = script
        .expected_routes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for step in &script.steps {
        if let Some(route) = &step.expected_route_id {
            routes.insert(route.clone());
        }
    }
    routes
}

fn is_live_input_source(platform: PlayerPlatform, source: &str) -> bool {
    match platform {
        PlayerPlatform::Windows => matches!(
            source,
            "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Linux => matches!(
            source,
            "uinput.keyboard" | "uinput.mouse" | "uinput.touch" | "uinput.gamepad"
        ),
        PlayerPlatform::Macos => matches!(
            source,
            "cgevent.keyboard" | "cgevent.mouse" | "cgevent.gamepad"
        ),
        PlayerPlatform::Web => matches!(source, "cdp.mouse" | "cdp.keyboard"),
        PlayerPlatform::Android => matches!(
            source,
            "android.touch" | "android.keyboard" | "android.gamepad" | "android.accessibility"
        ),
    }
}

fn is_consumption_trace_source(platform: PlayerPlatform, source: &str) -> bool {
    match platform {
        PlayerPlatform::Windows => source == "player_host.trace",
        PlayerPlatform::Linux => source == "player_host.trace",
        PlayerPlatform::Macos => source == "player_host.trace",
        PlayerPlatform::Web => matches!(source, "player_host.trace" | "browser_host.trace"),
        PlayerPlatform::Android => {
            matches!(source, "player_host.trace" | "android_host.trace")
        }
    }
}

fn is_forbidden_input_source(source: &str) -> bool {
    matches!(
        source,
        "route_scenario"
            | "--route-scenario"
            | "dom.click"
            | "dom_click"
            | "js.callback"
            | "js_callback"
            | "vn_player_command"
            | "direct.vn_command"
    )
}

fn is_safe_relative_ref(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains("://")
        && !value.contains('\\')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}

fn pass_check(
    id: impl Into<String>,
    summary: impl Into<String>,
    evidence: Vec<PlayerAutomationEvidence>,
) -> PlayerAutomationCheck {
    PlayerAutomationCheck {
        id: id.into(),
        status: PlayerAutomationStatus::Pass,
        summary: summary.into(),
        diagnostic: None,
        evidence,
    }
}

fn blocked_check(
    id: impl Into<String>,
    summary: impl Into<String>,
    code: impl Into<String>,
) -> PlayerAutomationCheck {
    blocked_check_with_evidence(id, summary, code, Vec::new())
}

fn blocked_check_with_evidence(
    id: impl Into<String>,
    summary: impl Into<String>,
    code: impl Into<String>,
    evidence: Vec<PlayerAutomationEvidence>,
) -> PlayerAutomationCheck {
    PlayerAutomationCheck {
        id: id.into(),
        status: PlayerAutomationStatus::Blocked,
        summary: summary.into(),
        diagnostic: Some(Diagnostic::blocking(
            code,
            "player automation evidence blocked",
        )),
        evidence,
    }
}

fn evidence(key: impl Into<String>, value: impl ToString) -> PlayerAutomationEvidence {
    PlayerAutomationEvidence {
        key: key.into(),
        value: value.to_string(),
    }
}
