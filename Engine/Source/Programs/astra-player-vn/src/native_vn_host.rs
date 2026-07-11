use std::collections::BTreeMap;

use astra_asset::{AssetCatalog, VfsManifest, VfsSourceRef};
use astra_core::{Hash256, SchemaVersion};
use astra_media_core::{
    BlendMode, DrawCommand, GlyphBitmap, HeadlessRenderer, HeadlessRendererProvider, MediaError,
    RectI, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest, TextureFrame,
};
use astra_player_core::{
    PlayerAction, PlayerAudioLifecyclePlan, PlayerDecodeKind, PlayerDecodeLifecyclePlan,
    PlayerDecodedAudio, PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandError,
    PlayerHostResourceId, PlayerSaveTransactionPlan, PlayerTimelineTask, PlayerTimelineTaskAction,
};
use astra_plugin::{ProductRuntimeHost, RuntimeHostError, RuntimeHostSchemaRegistry};
use astra_plugin_abi::{
    GameRuntimeSessionId, RuntimeOpenRequest, RuntimeOutputDomain, RuntimeRestoreRequest,
    RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionCodec, RuntimeSectionPayload,
    RuntimeStepInput,
};
use astra_vn_core::{
    CompiledStory, PresentationCommand, VnPlayerCommand, VnRunConfig, VnRuntimeState,
};
use astra_vn_package::decode_compiled_story;
use astra_vn_runtime_provider::NativeVnRuntimeProvider;
use astra_vn_system::{SystemUiAction, SystemUiModel};

pub struct NativeVnHostCommandSource {
    host: ProductRuntimeHost,
    session_id: GameRuntimeSessionId,
    runtime_state: Option<VnRuntimeState>,
    renderer: HeadlessRenderer,
    surface: PlayerHostResourceId,
    command_sequence: u64,
    fixed_step: u64,
    width: u32,
    height: u32,
    textures: BTreeMap<String, TextureFrame>,
    scene_draw: Vec<DrawCommand>,
    last_draw: Vec<DrawCommand>,
    last_step_evidence: Option<NativeVnStepEvidence>,
    terminal_routes: std::collections::BTreeSet<String>,
    pending_timeline: Vec<PlayerTimelineTask>,
    media_assets: BTreeMap<String, PackagedMediaAsset>,
    pending_audio: Vec<NativeVnAudioRequest>,
    next_media_resource_id: u64,
}

#[derive(Debug, Clone)]
struct PackagedMediaAsset {
    codec: String,
    bytes: Vec<u8>,
    hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnAudioRequest {
    pub command_id: String,
    pub command: String,
    pub attributes: BTreeMap<String, String>,
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: Vec<u8>,
    pub encoded_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnStepEvidence {
    pub schema: String,
    pub fixed_step: u64,
    pub coverage_reached: std::collections::BTreeSet<String>,
    pub vn_state_hash_before: String,
    pub vn_state_hash_after: String,
    pub runtime_state_hash: String,
    pub runtime_event_hash: String,
    pub runtime_presentation_hash: String,
    pub current_state_id: Option<String>,
    pub pending_choice_ids: Vec<String>,
    pub terminal_route_ids: std::collections::BTreeSet<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RuntimeStepEffectEvidence {
    coverage_reached: std::collections::BTreeSet<String>,
    state_hash_before_advance: String,
    state_hash_after_advance: String,
}

#[derive(Debug, serde::Deserialize)]
struct RuntimeStepTraceEvidence {
    runtime_state_hash: String,
    runtime_event_hash: String,
    runtime_presentation_hash: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NativeVnPlayerSavePayload {
    schema: String,
    slot: String,
    sections: RuntimeSaveSections,
    runtime_state: VnRuntimeState,
    draw_commands_json: Vec<u8>,
    draw_commands_hash: Hash256,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NativeVnPlayerSaveEnvelope {
    schema: String,
    payload_hash: Hash256,
    payload: NativeVnPlayerSavePayload,
}

#[derive(Debug, serde::Deserialize)]
struct VnRuntimeStateSaveEnvelope {
    schema: String,
    state_hash: astra_core::Hash128,
    state: VnRuntimeState,
}

impl NativeVnHostCommandSource {
    pub fn new(
        compiled: CompiledStory,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        Self::open(
            compiled,
            config,
            width,
            height,
            surface,
            BTreeMap::new(),
            BTreeMap::new(),
        )
    }

    pub fn from_package(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        validate_product_provider_bindings(package)?;
        let compiled = decode_compiled_story(package)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        let textures = load_package_textures(package)?;
        let media_assets = load_package_media_assets(package)?;
        Self::open(
            compiled,
            config,
            width,
            height,
            surface,
            textures,
            media_assets,
        )
    }

    fn open(
        compiled: CompiledStory,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        textures: BTreeMap<String, TextureFrame>,
        media_assets: BTreeMap<String, PackagedMediaAsset>,
    ) -> Result<Self, NativeVnHostError> {
        if compiled.story_manifest.stories.is_empty() {
            return Err(NativeVnHostError::EmptyStory);
        }
        let package_hash = compiled.story_hash.to_string();
        let terminal_routes = compiled
            .route_graph
            .nodes
            .iter()
            .filter(|node| node.terminal)
            .map(|node| node.id.clone())
            .collect();
        let compiled_bytes = postcard::to_allocvec(&compiled)
            .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
        let compiled_section = RuntimeSectionPayload {
            section_id: "vn.compiled_story".to_string(),
            schema: "astra.vn.compiled_story".to_string(),
            version: SchemaVersion::default(),
            codec: RuntimeSectionCodec::Postcard,
            hash: Hash256::from_sha256(&compiled_bytes),
            bytes: compiled_bytes,
        };
        let schemas = RuntimeHostSchemaRegistry::new()
            .allow_version(
                RuntimeOutputDomain::Effect,
                "astra.vn.runtime_step_effect.v2",
                SchemaVersion::new(2, 0, 0),
            )
            .allow(
                RuntimeOutputDomain::Presentation,
                "astra.vn.presentation_command.v1",
            )
            .allow(RuntimeOutputDomain::Audio, "astra.vn.audio_command.v1")
            .allow(RuntimeOutputDomain::Await, "astra.runtime.await_id.v1")
            .allow(RuntimeOutputDomain::Effect, "astra.vn.timeline_task.v1")
            .allow(RuntimeOutputDomain::Trace, "astra.vn.runtime_step_trace.v1")
            .allow(RuntimeOutputDomain::Trace, "astra.vn.runtime_state.v1")
            .allow(
                RuntimeOutputDomain::DirtySaveSection,
                "astra.runtime.dirty_save_section.v1",
            );
        let mut host = ProductRuntimeHost::in_process(
            "astra-player.native-vn",
            NativeVnRuntimeProvider::default(),
            schemas,
        )?;
        let open = host.open(RuntimeOpenRequest {
            target_id: "nativevn-game".to_string(),
            profile: config.profile,
            locale: config.locale,
            seed: 0,
            package_hash,
            sections: vec![compiled_section],
        })?;
        let renderer = HeadlessRendererProvider.create(RendererCreateRequest {
            width,
            height,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "player".to_string(),
        })?;
        tracing::info!(
            event = "player.vn.runtime.open",
            width,
            height,
            "opened AstraVN Player command source through ProductRuntimeHost"
        );
        Ok(Self {
            host,
            session_id: open.session_id,
            runtime_state: None,
            renderer,
            surface,
            command_sequence: 0,
            fixed_step: 0,
            width,
            height,
            textures,
            scene_draw: Vec::new(),
            last_draw: Vec::new(),
            last_step_evidence: None,
            terminal_routes,
            pending_timeline: Vec::new(),
            media_assets,
            pending_audio: Vec::new(),
            next_media_resource_id: 10_000,
        })
    }

    pub fn last_step_evidence(&self) -> Option<&NativeVnStepEvidence> {
        self.last_step_evidence.as_ref()
    }

    pub fn take_timeline_tasks(&mut self) -> Vec<PlayerTimelineTask> {
        std::mem::take(&mut self.pending_timeline)
    }

    pub fn take_audio_requests(&mut self) -> Vec<NativeVnAudioRequest> {
        std::mem::take(&mut self.pending_audio)
    }

    pub fn pending_wait(&self) -> Option<&astra_vn_core::VnWaitState> {
        self.runtime_state
            .as_ref()
            .and_then(|state| state.pending_wait.as_ref())
    }

    pub fn prepare_audio_decode(
        &mut self,
        request: &NativeVnAudioRequest,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        if request.encoded_bytes.is_empty()
            || Hash256::from_sha256(&request.encoded_bytes) != request.encoded_hash
        {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_AUDIO_ENCODED_HASH: {}",
                request.asset_id
            )));
        }
        let session = self.next_media_resource()?;
        Ok(PlayerDecodeLifecyclePlan {
            session,
            open: PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenDecode {
                sequence: self.next_command_sequence()?,
                session,
                kind: PlayerDecodeKind::Audio,
            }])?,
            decode: PlayerHostCommandBatch::new(vec![PlayerHostCommand::Decode {
                sequence: self.next_command_sequence()?,
                request_sequence: 1,
                session,
                kind: PlayerDecodeKind::Audio,
                codec: request.codec.clone(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                bytes: request.encoded_bytes.clone(),
            }])?,
            close: PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseDecode {
                sequence: self.next_command_sequence()?,
                session,
            }])?,
        })
    }

    pub fn prepare_audio_playback(
        &mut self,
        audio: &PlayerDecodedAudio,
    ) -> Result<PlayerAudioLifecyclePlan, NativeVnHostError> {
        const PACKET_FRAMES: usize = 4096;
        if audio.samples.is_empty() || audio.frame_count() == 0 {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_AUDIO_EMPTY: decoded audio contains no frames".into(),
            ));
        }
        let frame_count = u32::try_from(audio.frame_count()).map_err(|_| {
            NativeVnHostError::Asset(
                "ASTRA_PLAYER_AUDIO_FRAME_BUDGET: frame count exceeds platform contract".into(),
            )
        })?;
        let output = self.next_media_resource()?;
        let open = PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenAudio {
            sequence: self.next_command_sequence()?,
            output,
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            max_buffered_frames: frame_count,
        }])?;
        let samples_per_packet = PACKET_FRAMES
            .checked_mul(usize::from(audio.channels))
            .ok_or_else(|| NativeVnHostError::Asset("ASTRA_PLAYER_AUDIO_PACKET_BUDGET".into()))?;
        let mut submits = Vec::new();
        for (index, samples) in audio.samples.chunks(samples_per_packet).enumerate() {
            submits.push(PlayerHostCommandBatch::new(vec![
                PlayerHostCommand::SubmitAudio {
                    sequence: self.next_command_sequence()?,
                    output,
                    packet_sequence: u64::try_from(index + 1).map_err(|_| {
                        NativeVnHostError::Asset("ASTRA_PLAYER_AUDIO_PACKET_SEQUENCE".into())
                    })?,
                    channels: audio.channels,
                    samples: samples.to_vec(),
                },
            ])?);
        }
        let drain = PlayerHostCommandBatch::new(vec![PlayerHostCommand::DrainAudio {
            sequence: self.next_command_sequence()?,
            output,
        }])?;
        let close = PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseAudio {
            sequence: self.next_command_sequence()?,
            output,
        }])?;
        Ok(PlayerAudioLifecyclePlan {
            output,
            expected_sample_count: audio.samples.len() as u64,
            open,
            submits,
            drain,
            close,
        })
    }

    pub fn complete_wait(
        &mut self,
        fence: impl Into<String>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.command(VnPlayerCommand::CompleteWait {
            fence: fence.into(),
        })
    }

    pub fn save(&mut self, slot: impl Into<String>) -> Result<Vec<u8>, NativeVnHostError> {
        let slot = slot.into();
        if slot.trim().is_empty() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_INVALID: save slot must not be empty".into(),
            ));
        }
        let runtime_state = self.runtime_state.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_STATE_MISSING: runtime has not launched".into(),
            )
        })?;
        let sections = self.host.save(RuntimeSaveRequest {
            session_id: self.session_id.clone(),
            slot: slot.clone(),
        })?;
        validate_saved_runtime_state(&sections, &runtime_state)?;
        let draw_commands_json = serde_json::to_vec(&self.last_draw)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        let payload = NativeVnPlayerSavePayload {
            schema: "astra.player.native_vn_save_payload.v1".into(),
            slot,
            sections,
            runtime_state,
            draw_commands_hash: Hash256::from_sha256(&draw_commands_json),
            draw_commands_json,
        };
        let payload_bytes = postcard::to_allocvec(&payload)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        postcard::to_allocvec(&NativeVnPlayerSaveEnvelope {
            schema: "astra.player.native_vn_save.v1".into(),
            payload_hash: Hash256::from_sha256(&payload_bytes),
            payload,
        })
        .map_err(|error| NativeVnHostError::Save(error.to_string()))
    }

    pub fn restore(&mut self, bytes: &[u8]) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let envelope: NativeVnPlayerSaveEnvelope =
            postcard::from_bytes(bytes).map_err(|error| {
                NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
            })?;
        if envelope.schema != "astra.player.native_vn_save.v1"
            || envelope.payload.schema != "astra.player.native_vn_save_payload.v1"
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_VERSION_UNSUPPORTED: save schema is not supported".into(),
            ));
        }
        let payload_bytes = postcard::to_allocvec(&envelope.payload)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        if Hash256::from_sha256(&payload_bytes) != envelope.payload_hash {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_INTEGRITY: save payload hash mismatch".into(),
            ));
        }
        if envelope.payload.sections.session_id != self.session_id {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SESSION_MISMATCH: save belongs to another runtime session"
                    .into(),
            ));
        }
        validate_saved_runtime_state(&envelope.payload.sections, &envelope.payload.runtime_state)?;
        let report = self.host.restore(RuntimeRestoreRequest {
            session_id: self.session_id.clone(),
            sections: envelope.payload.sections.sections,
        })?;
        if report.status != "restored" || !report.diagnostics.is_empty() {
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_RESTORE_FAILED: status={} diagnostics={}",
                report.status,
                report.diagnostics.join(",")
            )));
        }
        if Hash256::from_sha256(&envelope.payload.draw_commands_json)
            != envelope.payload.draw_commands_hash
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_INTEGRITY: presentation command hash mismatch".into(),
            ));
        }
        let draw_commands =
            serde_json::from_slice(&envelope.payload.draw_commands_json).map_err(|error| {
                NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
            })?;
        self.runtime_state = Some(envelope.payload.runtime_state);
        self.last_draw = draw_commands;
        self.last_step_evidence = None;
        let draw = self.last_draw.clone();
        self.present_draw(&draw, 0)
    }

    pub fn prepare_save_transaction(
        &mut self,
        slot: impl Into<String>,
        transaction: PlayerHostResourceId,
    ) -> Result<PlayerSaveTransactionPlan, NativeVnHostError> {
        let slot = slot.into();
        let bytes = self.save(slot.clone())?;
        let begin = PlayerHostCommandBatch::new(vec![PlayerHostCommand::BeginSave {
            sequence: self.next_command_sequence()?,
            slot,
            transaction,
        }])?;
        let write = PlayerHostCommandBatch::new(vec![PlayerHostCommand::WriteSave {
            sequence: self.next_command_sequence()?,
            transaction,
            bytes,
        }])?;
        let commit = PlayerHostCommandBatch::new(vec![PlayerHostCommand::CommitSave {
            sequence: self.next_command_sequence()?,
            transaction,
        }])?;
        let abort = PlayerHostCommandBatch::new(vec![PlayerHostCommand::AbortSave {
            sequence: self.next_command_sequence()?,
            transaction,
        }])?;
        Ok(PlayerSaveTransactionPlan {
            begin,
            write,
            commit,
            abort,
        })
    }

    pub fn read_save(
        &mut self,
        slot: impl Into<String>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = slot.into();
        if slot.trim().is_empty() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_INVALID: save slot must not be empty".into(),
            ));
        }
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::ReadSave {
                sequence: self.next_command_sequence()?,
                slot,
            },
        ])?)
    }

    fn next_command_sequence(&mut self) -> Result<u64, NativeVnHostError> {
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(self.command_sequence)
    }

    fn next_media_resource(&mut self) -> Result<PlayerHostResourceId, NativeVnHostError> {
        self.next_media_resource_id = self
            .next_media_resource_id
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(PlayerHostResourceId(self.next_media_resource_id))
    }

    pub fn launch(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.step("launch_default", serde_json::json!({}))
    }

    pub fn dispatch_action(
        &mut self,
        action: PlayerAction,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let command = match action {
            PlayerAction::Advance => {
                if self
                    .runtime_state
                    .as_ref()
                    .is_some_and(|state| state.pending_choice.is_some())
                {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_CHOICE_REQUIRED: advance cannot select a choice".into(),
                    ));
                }
                VnPlayerCommand::Advance
            }
            PlayerAction::ChooseIndex { index } => {
                let option_id = self
                    .runtime_state
                    .as_ref()
                    .and_then(|state| state.pending_choice.as_ref())
                    .and_then(|choice| choice.options.get(index))
                    .map(|option| option.id.clone())
                    .ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_CHOICE_INDEX: choice index is unavailable".into(),
                        )
                    })?;
                VnPlayerCommand::Choose { option_id }
            }
            PlayerAction::OpenSystemPage { page } => VnPlayerCommand::OpenSystem {
                page: astra_vn_core::SystemPageKind::parse(&page),
            },
            PlayerAction::Back => VnPlayerCommand::ReturnSystem,
        };
        self.command(command)
    }

    pub fn dispatch_pointer(
        &mut self,
        x: f64,
        y: f64,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::Input("ASTRA_PLAYER_STATE: runtime has not launched".into())
        })?;
        let model = if let Some(choice) = &state.pending_choice {
            SystemUiModel::choice(self.width, self.height, choice.options.len())
        } else if let Some(frame) = state.system_stack.last() {
            SystemUiModel::system(frame.page, self.width, self.height).ok_or_else(|| {
                NativeVnHostError::Input("ASTRA_PLAYER_SYSTEM_PAGE: unknown system page".into())
            })?
        } else {
            SystemUiModel::message(self.width, self.height)
        };
        let action = model.hit_test(x, y).cloned().ok_or_else(|| {
            NativeVnHostError::Input("ASTRA_PLAYER_HIT_TEST: pointer did not hit a control".into())
        })?;
        let action = match action {
            SystemUiAction::Advance => PlayerAction::Advance,
            SystemUiAction::ChooseIndex { index } => PlayerAction::ChooseIndex { index },
            SystemUiAction::Open { surface } => PlayerAction::OpenSystemPage {
                page: format!("{surface:?}").to_lowercase(),
            },
            SystemUiAction::Back => PlayerAction::Back,
            SystemUiAction::Activate { control_id } => {
                return Err(NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_CONTROL_UNBOUND: {control_id}"
                )))
            }
        };
        self.dispatch_action(action)
    }

    pub fn shutdown(mut self) -> Result<(), NativeVnHostError> {
        self.host.shutdown()?;
        self.host.destroy()?;
        Ok(())
    }

    fn command(
        &mut self,
        command: VnPlayerCommand,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let payload = serde_json::to_value(command)
            .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
        self.step("command", payload)
    }

    fn step(
        &mut self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.fixed_step = self
            .fixed_step
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        let output = self.host.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step: self.fixed_step,
            action: action.to_string(),
            payload,
        })?;
        let effect = output
            .outputs
            .iter()
            .find(|envelope| {
                envelope.domain == RuntimeOutputDomain::Effect
                    && envelope.schema == "astra.vn.runtime_step_effect.v2"
            })
            .ok_or_else(|| {
                NativeVnHostError::RuntimeEvidence(
                    "ASTRA_PLAYER_VN_EFFECT_MISSING: runtime step effect is required".into(),
                )
            })?
            .decode_postcard::<RuntimeStepEffectEvidence>(
                RuntimeOutputDomain::Effect,
                "astra.vn.runtime_step_effect.v2",
                SchemaVersion::new(2, 0, 0),
            )
            .map_err(|err| NativeVnHostError::RuntimeEvidence(err.to_string()))?;
        let runtime_trace = output
            .outputs
            .iter()
            .find(|envelope| {
                envelope.domain == RuntimeOutputDomain::Trace
                    && envelope.schema == "astra.vn.runtime_step_trace.v1"
            })
            .ok_or_else(|| {
                NativeVnHostError::RuntimeEvidence(
                    "ASTRA_PLAYER_VN_TRACE_MISSING: runtime step trace is required".into(),
                )
            })?
            .decode_postcard::<RuntimeStepTraceEvidence>(
                RuntimeOutputDomain::Trace,
                "astra.vn.runtime_step_trace.v1",
                SchemaVersion::new(1, 0, 0),
            )
            .map_err(|err| NativeVnHostError::RuntimeEvidence(err.to_string()))?;
        for trace in output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Trace)
        {
            if trace.schema == "astra.vn.runtime_state.v1" {
                self.runtime_state = Some(
                    trace
                        .decode_postcard(
                            RuntimeOutputDomain::Trace,
                            "astra.vn.runtime_state.v1",
                            SchemaVersion::new(1, 0, 0),
                        )
                        .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?,
                );
            }
        }
        let runtime_state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_STATE_MISSING: runtime state trace is required".into(),
            )
        })?;
        self.last_step_evidence = Some(NativeVnStepEvidence {
            schema: "astra.player_vn_step_evidence.v1".to_string(),
            fixed_step: self.fixed_step,
            coverage_reached: effect.coverage_reached,
            vn_state_hash_before: effect.state_hash_before_advance,
            vn_state_hash_after: effect.state_hash_after_advance,
            runtime_state_hash: runtime_trace.runtime_state_hash,
            runtime_event_hash: runtime_trace.runtime_event_hash,
            runtime_presentation_hash: runtime_trace.runtime_presentation_hash,
            current_state_id: runtime_state
                .cursor
                .as_ref()
                .map(|cursor| cursor.state_id.clone()),
            pending_choice_ids: runtime_state
                .pending_choice
                .as_ref()
                .map(|choice| {
                    choice
                        .options
                        .iter()
                        .map(|option| option.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
            terminal_route_ids: if runtime_state.cursor.is_none() {
                runtime_state
                    .route_coverage
                    .intersection(&self.terminal_routes)
                    .cloned()
                    .collect()
            } else {
                Default::default()
            },
        });
        for envelope in output.outputs.iter().filter(|envelope| {
            envelope.domain == RuntimeOutputDomain::Effect
                && envelope.schema == "astra.vn.timeline_task.v1"
        }) {
            let task = envelope
                .decode_postcard::<astra_vn_core::VnTimelineTask>(
                    RuntimeOutputDomain::Effect,
                    "astra.vn.timeline_task.v1",
                    SchemaVersion::new(1, 0, 0),
                )
                .map_err(|error| NativeVnHostError::RuntimeEvidence(error.to_string()))?;
            self.pending_timeline.push(player_timeline_task(task)?);
        }
        for envelope in output.outputs.iter().filter(|envelope| {
            envelope.domain == RuntimeOutputDomain::Audio
                && envelope.schema == "astra.vn.audio_command.v1"
        }) {
            let command = envelope
                .decode_postcard::<astra_vn_core::VnAudioCommand>(
                    RuntimeOutputDomain::Audio,
                    "astra.vn.audio_command.v1",
                    SchemaVersion::new(1, 0, 0),
                )
                .map_err(|error| NativeVnHostError::RuntimeEvidence(error.to_string()))?;
            let asset_id = command.attributes.get("asset").cloned().ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_AUDIO_ASSET_REQUIRED: {}",
                    command.command_id
                ))
            })?;
            let asset = self.media_assets.get(&asset_id).ok_or_else(|| {
                NativeVnHostError::Asset(format!("ASTRA_PLAYER_AUDIO_ASSET_MISSING: {asset_id}"))
            })?;
            self.pending_audio.push(NativeVnAudioRequest {
                command_id: command.command_id,
                command: command.command,
                attributes: command.attributes,
                asset_id,
                codec: asset.codec.clone(),
                encoded_bytes: asset.bytes.clone(),
                encoded_hash: asset.hash,
            });
        }
        let presentation = output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Presentation)
            .map(|envelope| {
                envelope
                    .decode_postcard(
                        RuntimeOutputDomain::Presentation,
                        "astra.vn.presentation_command.v1",
                        SchemaVersion::new(1, 0, 0),
                    )
                    .map_err(|err| NativeVnHostError::Serialize(err.to_string()))
            })
            .collect::<Result<Vec<PresentationCommand>, _>>()?;
        self.render(&presentation)
    }

    fn render(
        &mut self,
        presentation: &[PresentationCommand],
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let mut draw = vec![DrawCommand::clear([8, 10, 16, 255])];
        for command in presentation {
            match command {
                PresentationCommand::Dialogue { key, speaker, .. } => {
                    let panel_height = (self.height / 3).max(64);
                    draw.push(DrawCommand::rect(
                        "vn.dialogue.panel",
                        24,
                        self.height.saturating_sub(panel_height + 24),
                        self.width.saturating_sub(48),
                        panel_height,
                        [18, 22, 34, 236],
                    ));
                    if let Some(speaker) = speaker {
                        push_bitmap_text(
                            &mut draw,
                            speaker,
                            42,
                            self.height.saturating_sub(panel_height + 4),
                            [120, 210, 255, 255],
                        );
                    }
                    push_bitmap_text(
                        &mut draw,
                        key,
                        42,
                        self.height.saturating_sub(panel_height - 28),
                        [245, 245, 248, 255],
                    );
                }
                PresentationCommand::Choice { key, options } => {
                    push_bitmap_text(&mut draw, key, 42, 32, [245, 245, 248, 255]);
                    for (index, option) in options.iter().enumerate() {
                        let y = 64 + index as u32 * 38;
                        draw.push(DrawCommand::rect(
                            format!("vn.choice.{}", option.id),
                            34,
                            y,
                            self.width.saturating_sub(68),
                            30,
                            [30, 48, 70, 245],
                        ));
                        push_bitmap_text(&mut draw, &option.key, 46, y + 8, [255, 255, 255, 255]);
                    }
                }
                PresentationCommand::SystemPage { page } => {
                    push_bitmap_text(
                        &mut draw,
                        &format!("SYSTEM {page:?}"),
                        42,
                        42,
                        [220, 230, 255, 255],
                    );
                }
                PresentationCommand::Stage {
                    command,
                    attributes,
                } => {
                    self.apply_stage_command(command, attributes)?;
                }
                PresentationCommand::Marker { .. } => {}
            }
        }
        draw.splice(1..1, self.scene_draw.clone());
        self.last_draw = draw.clone();
        self.present_draw(&draw, presentation.len())
    }

    fn present_draw(
        &mut self,
        draw: &[DrawCommand],
        presentation_count: usize,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let frame = self.renderer.capture_frame(draw)?;
        let sequence = self.next_command_sequence()?;
        tracing::trace!(
            event = "player.vn.runtime.command.emit",
            sequence,
            presentation_count,
            frame_hash = %frame.hash,
            "emitted AstraVN Player host command"
        );
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentRgba {
                sequence,
                surface: self.surface,
                width: frame.width,
                height: frame.height,
                rgba8: frame.bytes,
            },
        ])?)
    }

    fn apply_stage_command(
        &mut self,
        command: &str,
        attributes: &BTreeMap<String, String>,
    ) -> Result<(), NativeVnHostError> {
        match command {
            "background" => {
                let asset_id = required_attribute(attributes, "asset", command)?;
                let frame = self.texture(asset_id)?.clone();
                self.scene_draw
                    .retain(|draw| !draw_id_is(draw, "vn.scene.background"));
                self.scene_draw.insert(
                    0,
                    DrawCommand::Texture {
                        id: "vn.scene.background".to_string(),
                        frame,
                        destination: RectI::new(0, 0, self.width, self.height),
                        opacity: 1.0,
                        blend: BlendMode::Alpha,
                    },
                );
            }
            "show" => {
                let asset_id = required_attribute(attributes, "asset", command)?;
                let frame = self.texture(asset_id)?.clone();
                let destination_height = self.height.saturating_mul(9) / 10;
                let destination_width = ((destination_height as u64 * frame.width as u64)
                    / frame.height as u64)
                    .min(self.width as u64) as u32;
                let x = match attributes.get("at").map(String::as_str) {
                    Some("left") => self.width / 12,
                    Some("center") => self.width.saturating_sub(destination_width) / 2,
                    _ => self
                        .width
                        .saturating_sub(destination_width + self.width / 12),
                };
                let id = attributes
                    .get("id")
                    .cloned()
                    .or_else(|| attributes.get("character").cloned())
                    .unwrap_or_else(|| "character".to_string());
                let draw_id = format!("vn.scene.character.{id}");
                self.scene_draw.retain(|draw| !draw_id_is(draw, &draw_id));
                self.scene_draw.push(DrawCommand::Texture {
                    id: draw_id,
                    frame,
                    destination: RectI::new(
                        x as i32,
                        self.height.saturating_sub(destination_height) as i32,
                        destination_width,
                        destination_height,
                    ),
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                });
            }
            "hide" => {
                let id = attributes
                    .get("id")
                    .or_else(|| attributes.get("character"))
                    .ok_or_else(|| {
                        NativeVnHostError::Asset(
                            "ASTRA_PLAYER_STAGE_ATTRIBUTE: hide requires id or character"
                                .to_string(),
                        )
                    })?;
                let draw_id = format!("vn.scene.character.{id}");
                self.scene_draw.retain(|draw| !draw_id_is(draw, &draw_id));
            }
            "movie" => {
                let asset_id = required_attribute(attributes, "asset", command)?;
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_VIDEO_DECODE_REQUIRED: packaged video asset {asset_id} requires the platform decode bridge"
                )));
            }
            _ => {}
        }
        Ok(())
    }

    fn texture(&self, asset_id: &str) -> Result<&TextureFrame, NativeVnHostError> {
        self.textures.get(asset_id).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_MISSING: cooked texture {asset_id} is not mounted"
            ))
        })
    }
}

fn validate_product_provider_bindings(
    package: &astra_package::PackageReader,
) -> Result<(), NativeVnHostError> {
    let policy = read_package_json(package, "provider.policy")?;
    let registry = read_package_json(package, "plugin.extension_registry")?;
    let headless_selected = policy.get("renderer").and_then(serde_json::Value::as_str)
        == Some("headless")
        || policy
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .any(|binding| {
                binding
                    .get("provider_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|provider| provider.contains("headless"))
            });
    if headless_selected {
        return Err(NativeVnHostError::ProviderBinding(
            "ASTRA_PLAYER_PRESENTATION_PROVIDER_INELIGIBLE: headless presentation is not packaged Player eligible"
                .to_string(),
        ));
    }
    for (slot, provider_id, capability) in [
        (
            "game_runtime_provider",
            "astra.runtime.native_vn",
            "runtime.native_vn",
        ),
        (
            "presentation",
            "astra.vn.standard_presentation",
            "presentation.vn.standard",
        ),
        ("renderer2d", "astra.renderer2d.wgpu", "renderer2d.wgpu"),
    ] {
        let policy_bound = policy
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.get("slot").and_then(serde_json::Value::as_str) == Some(slot)
                        && binding
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(provider_id)
                })
            });
        let registry_bound = registry
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.get("slot").and_then(serde_json::Value::as_str) == Some(slot)
                        && binding
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(provider_id)
                })
            });
        let registered = registry
            .get("providers")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|providers| {
                providers.iter().any(|provider| {
                    provider.get("slot").and_then(serde_json::Value::as_str) == Some(slot)
                        && provider
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(provider_id)
                        && provider
                            .get("capability")
                            .and_then(serde_json::Value::as_str)
                            == Some(capability)
                        && provider
                            .get("packaged")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                })
            });
        if !policy_bound || !registry_bound || !registered {
            return Err(NativeVnHostError::ProviderBinding(format!(
                "ASTRA_PLAYER_PROVIDER_BINDING_INVALID: {slot} must bind packaged provider {provider_id}"
            )));
        }
    }
    let presentation: astra_vn_presentation::VnPresentationProviderManifest = package
        .container()
        .decode_postcard("vn.presentation_provider_manifest")
        .map_err(|error| NativeVnHostError::ProviderBinding(error.to_string()))?;
    if !presentation.validate_standard().passed
        || presentation.renderer_provider != "astra.renderer2d.wgpu"
    {
        return Err(NativeVnHostError::ProviderBinding(
            "ASTRA_PLAYER_PRESENTATION_PROVIDER_INELIGIBLE: package presentation provider is not the product wgpu path"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_saved_runtime_state(
    sections: &RuntimeSaveSections,
    expected_state: &VnRuntimeState,
) -> Result<(), NativeVnHostError> {
    let section = sections
        .sections
        .iter()
        .find(|section| section.section_id == "vn.runtime_state")
        .ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_RUNTIME_SECTION_MISSING: vn.runtime_state is required".into(),
            )
        })?;
    if section.schema != "astra.vn.runtime_state_save.v1"
        || section.codec != RuntimeSectionCodec::Postcard
        || Hash256::from_sha256(&section.bytes) != section.hash
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_INTEGRITY: runtime section contract or hash mismatch".into(),
        ));
    }
    let saved: VnRuntimeStateSaveEnvelope =
        postcard::from_bytes(&section.bytes).map_err(|error| {
            NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
        })?;
    let state_bytes = postcard::to_allocvec(&saved.state)
        .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
    if saved.schema != "astra.vn.runtime_state_save.v1"
        || astra_core::Hash128::from_blake3(&state_bytes) != saved.state_hash
        || &saved.state != expected_state
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_INTEGRITY: runtime state evidence mismatch".into(),
        ));
    }
    Ok(())
}

fn player_timeline_task(
    task: astra_vn_core::VnTimelineTask,
) -> Result<PlayerTimelineTask, NativeVnHostError> {
    let action = match task.attributes.get("action").map(String::as_str) {
        None | Some("start") => PlayerTimelineTaskAction::Start,
        Some("cancel") => PlayerTimelineTaskAction::Cancel,
        Some(action) => {
            return Err(NativeVnHostError::RuntimeEvidence(format!(
                "ASTRA_PLAYER_TIMELINE_ACTION_UNSUPPORTED: {action}"
            )));
        }
    };
    let duration_ms = task
        .attributes
        .get("duration")
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                NativeVnHostError::RuntimeEvidence(format!(
                    "ASTRA_PLAYER_TIMELINE_DURATION_INVALID: {}",
                    task.command_id
                ))
            })
        })
        .transpose()?;
    Ok(PlayerTimelineTask {
        schema: "astra.player_timeline_task.v1".to_string(),
        task_id: task
            .attributes
            .get("id")
            .cloned()
            .unwrap_or(task.command_id),
        target: task.attributes.get("target").cloned(),
        action,
        duration_ms,
        fence: task.attributes.get("fence").cloned(),
    })
}

fn read_package_json(
    package: &astra_package::PackageReader,
    section: &str,
) -> Result<serde_json::Value, NativeVnHostError> {
    let bytes = package
        .container()
        .read_bounded(section, 256 * 1024)
        .map_err(|error| NativeVnHostError::ProviderBinding(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        NativeVnHostError::ProviderBinding(format!(
            "ASTRA_PLAYER_PROVIDER_SECTION_INVALID: {section}: {error}"
        ))
    })
}

fn load_package_textures(
    package: &astra_package::PackageReader,
) -> Result<BTreeMap<String, TextureFrame>, NativeVnHostError> {
    let catalog: AssetCatalog = serde_json::from_slice(
        &package
            .container()
            .read_section("asset.catalog")
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?,
    )
    .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
    let manifest: VfsManifest = serde_json::from_slice(
        &package
            .container()
            .read_section("asset.vfs_manifest")
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?,
    )
    .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
    let mut textures = BTreeMap::new();
    for asset in catalog.assets {
        if !asset.media_kind.starts_with("image") {
            continue;
        }
        let entry = manifest
            .entries
            .iter()
            .find(|entry| entry.uri == asset.uri)
            .ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_ASSET_VFS_MISSING: catalog asset {} has no VFS entry",
                    asset.asset_id
                ))
            })?;
        let VfsSourceRef::PackageSection { section_id } = &entry.source else {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_SOURCE: asset {} is not package-backed",
                asset.asset_id
            )));
        };
        let encoded = package
            .container()
            .read_section(section_id)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        if Hash256::from_sha256(&encoded) != entry.hash {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_HASH: asset {} failed VFS hash validation",
                asset.asset_id
            )));
        }
        let decoded = image::load_from_memory(&encoded)
            .map_err(|err| NativeVnHostError::Asset(format!("ASTRA_PLAYER_ASSET_DECODE: {err}")))?
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        let rgba8 = decoded.into_raw();
        textures.insert(
            asset.asset_id,
            TextureFrame {
                width,
                height,
                hash: Hash256::from_sha256(&rgba8),
                rgba8,
            },
        );
    }
    Ok(textures)
}

fn load_package_media_assets(
    package: &astra_package::PackageReader,
) -> Result<BTreeMap<String, PackagedMediaAsset>, NativeVnHostError> {
    const MAX_ENCODED_MEDIA_BYTES: usize = 512 * 1024 * 1024;
    let catalog: AssetCatalog = serde_json::from_slice(
        &package
            .container()
            .read_bounded("asset.catalog", 16 * 1024 * 1024)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?,
    )
    .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
    let manifest: VfsManifest = serde_json::from_slice(
        &package
            .container()
            .read_bounded("asset.vfs_manifest", 32 * 1024 * 1024)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?,
    )
    .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
    let mut assets = BTreeMap::new();
    for asset in catalog.assets.into_iter().filter(|asset| {
        asset.media_kind.starts_with("audio") || asset.media_kind.starts_with("voice")
    }) {
        let matches = manifest
            .entries
            .iter()
            .filter(|entry| entry.uri == asset.uri)
            .collect::<Vec<_>>();
        let [entry] = matches.as_slice() else {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_VFS_AMBIGUOUS: catalog asset {} must resolve to one VFS entry",
                asset.asset_id
            )));
        };
        let VfsSourceRef::PackageSection { section_id } = &entry.source else {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_SOURCE: asset {} is not package-backed",
                asset.asset_id
            )));
        };
        let bytes = package
            .container()
            .read_bounded(section_id, MAX_ENCODED_MEDIA_BYTES)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        let hash = Hash256::from_sha256(&bytes);
        if hash != entry.hash {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_HASH: asset {} failed VFS hash validation",
                asset.asset_id
            )));
        }
        let codec = sniff_audio_codec(&bytes).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_AUDIO_CODEC_UNSUPPORTED: {}",
                asset.asset_id
            ))
        })?;
        if assets
            .insert(
                asset.asset_id.clone(),
                PackagedMediaAsset {
                    codec: codec.to_string(),
                    bytes,
                    hash,
                },
            )
            .is_some()
        {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_ID_DUPLICATE: {}",
                asset.asset_id
            )));
        }
    }
    Ok(assets)
}

fn sniff_audio_codec(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3")
        || bytes
            .get(..2)
            .is_some_and(|prefix| prefix[0] == 0xff && prefix[1] & 0xe0 == 0xe0)
    {
        Some("mp3")
    } else if bytes.starts_with(b"OggS") {
        Some("ogg")
    } else if bytes.starts_with(b"fLaC") {
        Some("flac")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        Some("wav")
    } else {
        None
    }
}

fn required_attribute<'a>(
    attributes: &'a BTreeMap<String, String>,
    key: &str,
    command: &str,
) -> Result<&'a str, NativeVnHostError> {
    attributes.get(key).map(String::as_str).ok_or_else(|| {
        NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_STAGE_ATTRIBUTE: {command} requires {key}"
        ))
    })
}

fn draw_id_is(draw: &DrawCommand, expected: &str) -> bool {
    match draw {
        DrawCommand::Rect { id, .. }
        | DrawCommand::Texture { id, .. }
        | DrawCommand::VideoFrame { id, .. }
        | DrawCommand::Glyph { id, .. } => id == expected,
        _ => false,
    }
}

fn push_bitmap_text(
    commands: &mut Vec<DrawCommand>,
    text: &str,
    mut x: u32,
    y: u32,
    rgba: [u8; 4],
) {
    for (index, character) in text.chars().take(96).enumerate() {
        let alpha8 = bitmap_glyph(character);
        let hash = Hash256::from_sha256(&alpha8);
        commands.push(DrawCommand::Glyph {
            id: format!("vn.text.{index}"),
            glyph: GlyphBitmap {
                width: 5,
                height: 7,
                alpha8,
                hash,
            },
            x: x as i32,
            y: y as i32,
            rgba,
            opacity: 1.0,
            blend: BlendMode::Alpha,
        });
        x = x.saturating_add(6);
    }
}

fn bitmap_glyph(character: char) -> Vec<u8> {
    let rows: [u8; 7] = match character.to_ascii_uppercase() {
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0f, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0f],
        'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0f, 0x10, 0x10, 0x13, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1f],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x19, 0x15, 0x13, 0x13, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e],
        '6' => [0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e],
        '.' => [0, 0, 0, 0, 0, 0x06, 0x06],
        '_' => [0, 0, 0, 0, 0, 0, 0x1f],
        '-' => [0, 0, 0, 0x1f, 0, 0, 0],
        ' ' => [0; 7],
        _ => [0x0e, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
    };
    rows.into_iter()
        .flat_map(|row| {
            (0..5)
                .rev()
                .map(move |bit| if row & (1 << bit) != 0 { 255 } else { 0 })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum NativeVnHostError {
    #[error("compiled story has no playable entry state")]
    EmptyStory,
    #[error("Player host command sequence overflowed")]
    SequenceOverflow,
    #[error(transparent)]
    RuntimeHost(#[from] RuntimeHostError),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Command(#[from] PlayerHostCommandError),
    #[error("presentation serialization failed: {0}")]
    Serialize(String),
    #[error("package validation failed: {0}")]
    Package(String),
    #[error("presentation asset failed: {0}")]
    Asset(String),
    #[error("player input failed: {0}")]
    Input(String),
    #[error("runtime evidence failed: {0}")]
    RuntimeEvidence(String),
    #[error("provider binding failed: {0}")]
    ProviderBinding(String),
    #[error("player save failed: {0}")]
    Save(String),
}
