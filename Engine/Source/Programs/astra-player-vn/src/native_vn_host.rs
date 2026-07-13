use std::collections::{BTreeMap, BTreeSet};

use astra_asset::{AssetCatalog, VfsManifest, VfsSourceRef};
use astra_core::{Hash256, SchemaVersion};
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, TextDirection,
    TextLayoutConfig, TextLayoutProvider, TextLayoutRequest, TextRenderResourceOwner, TextRun,
    WrapPolicy,
};
use astra_media_core::{BlendMode, MediaError, RectI, SceneCommand, TextureFrame};
use astra_player_core::{
    PlayerAction, PlayerAudioLifecyclePlan, PlayerDecodeKind, PlayerDecodeLifecyclePlan,
    PlayerDecodedAudio, PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandError,
    PlayerHostResourceId, PlayerSaveTransactionPlan, PlayerTimelineTask, PlayerTimelineTaskAction,
};
use astra_plugin::{ProductRuntimeHost, RuntimeHostError, RuntimeHostSchemaRegistry};
use astra_plugin_abi::{
    GameRuntimeSessionId, RuntimeOpenRequest, RuntimeOutputDomain, RuntimePrepareRequest,
    RuntimeProbeRequest, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections,
    RuntimeSectionCodec, RuntimeSectionPayload, RuntimeStepInput, RuntimeStepMode,
    ValidatedRuntimeProviderSelection, NATIVE_VN_PROVIDER_ID,
};
use astra_vn_core::{
    CompiledCommand, CompiledStory, PresentationCommand, StageBlendMode, StageClipPolicy,
    StageCommand, StageLayerKind, SystemPageKind, VnPlayerCommand, VnRunConfig, VnRuntimeState,
};
use astra_vn_package::{
    decode_compiled_story, load_localization as load_package_localization,
    load_player_locale_config, load_presentation_provider_manifest, ProductStageDirector,
    ProductStageState, StageDirectorOutput, VnLocalizationTable, VnPresentationProviderManifest,
};
use astra_vn_runtime_provider::NativeVnRuntimeProvider;
use astra_vn_system::{SystemUiAction, SystemUiModel};

pub struct NativeVnHostCommandSource {
    host: ProductRuntimeHost,
    session_id: GameRuntimeSessionId,
    runtime_state: Option<VnRuntimeState>,
    text_provider: CosmicTextLayoutProvider,
    font_families: Vec<String>,
    text_resources: TextRenderResourceOwner,
    localization: VnLocalizationTable,
    surface: PlayerHostResourceId,
    command_sequence: u64,
    fixed_step: u64,
    session_seed: u64,
    width: u32,
    height: u32,
    textures: BTreeMap<String, TextureFrame>,
    live_texture_ids: BTreeSet<String>,
    live_layout_ids: BTreeSet<String>,
    scene_draw: Vec<SceneCommand>,
    last_draw: Vec<SceneCommand>,
    last_step_evidence: Option<NativeVnStepEvidence>,
    terminal_routes: std::collections::BTreeSet<String>,
    pending_timeline: Vec<PlayerTimelineTask>,
    media_assets: BTreeMap<String, PackagedMediaAsset>,
    pending_audio: Vec<NativeVnAudioOutput>,
    next_media_resource_id: u64,
    presentation_manifest: VnPresentationProviderManifest,
    presentation_profile: String,
    stage_director: ProductStageDirector,
    shutdown_started: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnAudioControlRequest {
    pub command_id: String,
    pub action: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeVnAudioOutput {
    Start(NativeVnAudioRequest),
    Control(NativeVnAudioControlRequest),
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

struct ProductPresentationBinding {
    textures: BTreeMap<String, TextureFrame>,
    media_assets: BTreeMap<String, PackagedMediaAsset>,
    localization: VnLocalizationTable,
    text_provider: CosmicTextLayoutProvider,
    font_families: Vec<String>,
    manifest: VnPresentationProviderManifest,
}

struct ProductPackageBinding {
    runtime_provider: ValidatedRuntimeProviderSelection,
    package_hash: Hash256,
    package_section_ids: Vec<String>,
    presentation: ProductPresentationBinding,
}

impl NativeVnHostCommandSource {
    pub fn from_package(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        validate_product_provider_bindings(package)?;
        let runtime_provider = package.runtime_provider_selection().clone();
        if config.profile != runtime_provider.profile() {
            return Err(NativeVnHostError::Package(format!(
                "ASTRA_PLAYER_RUNTIME_PROFILE_MISMATCH: requested profile {} does not match package provider profile {}",
                config.profile,
                runtime_provider.profile()
            )));
        }
        let compiled = decode_compiled_story(package)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        let textures = load_package_textures(package)?;
        let media_assets = load_package_media_assets(package)?;
        let presentation_manifest =
            load_presentation_provider_manifest(package, &config.profile)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        validate_story_presentation(
            &compiled,
            &textures,
            &presentation_manifest,
            &config.profile,
        )?;
        let locale_config = load_player_locale_config(package)
            .map_err(|error| NativeVnHostError::Localization(error.to_string()))?;
        if locale_config
            .available_locales
            .binary_search(&config.locale)
            .is_err()
        {
            return Err(NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_LOCALE_UNDECLARED: locale {} is not declared by player.locale_config",
                config.locale
            )));
        }
        let localization = load_package_localization(package, &config.locale, 16 * 1024 * 1024)
            .map_err(|error| NativeVnHostError::Localization(error.to_string()))?;
        let text_provider = CosmicTextLayoutProvider::from_package(
            package,
            "media.font_manifest",
            FontBindingContext {
                target: runtime_provider.target().to_string(),
                profile: config.profile.clone(),
                default_locale: config.locale.clone(),
            },
            TextLayoutConfig::production_defaults(),
        )?;
        let font_families = text_provider
            .identity()?
            .fonts
            .into_iter()
            .map(|font| font.family)
            .collect::<Vec<_>>();
        validate_story_text(&compiled, &localization, &text_provider, &font_families)?;
        Self::open(
            compiled,
            config,
            width,
            height,
            surface,
            ProductPackageBinding {
                runtime_provider,
                package_hash: package.package_hash(),
                package_section_ids: package
                    .container()
                    .entries()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect(),
                presentation: ProductPresentationBinding {
                    textures,
                    media_assets,
                    localization,
                    text_provider,
                    font_families,
                    manifest: presentation_manifest,
                },
            },
        )
    }

    fn open(
        compiled: CompiledStory,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        binding: ProductPackageBinding,
    ) -> Result<Self, NativeVnHostError> {
        if compiled.story_manifest.stories.is_empty() {
            return Err(NativeVnHostError::EmptyStory);
        }
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
        let stage_director = ProductStageDirector::new(
            binding.presentation.manifest.clone(),
            binding.runtime_provider.profile(),
            astra_vn_package::StageViewport { width, height },
        )
        .map_err(stage_director_error)?;
        let runtime_provider = &binding.runtime_provider;
        let schemas = RuntimeHostSchemaRegistry::from_descriptor(runtime_provider.descriptor());
        if runtime_provider.provider_id() != NATIVE_VN_PROVIDER_ID {
            return Err(NativeVnHostError::Package(format!(
                "ASTRA_PLAYER_RUNTIME_PROVIDER_UNAVAILABLE: package selected unlinked provider {}",
                runtime_provider.provider_id()
            )));
        }
        let instance_id = format!(
            "astra-player.native-vn.{}",
            runtime_provider
                .binding_hash()
                .to_string()
                .trim_start_matches("sha256:")
        );
        let mut host = ProductRuntimeHost::bound_in_process(
            instance_id,
            runtime_provider,
            NativeVnRuntimeProvider::default(),
            schemas,
        )?;
        let prepare = match host.prepare(RuntimePrepareRequest {
            target_id: runtime_provider.target().to_string(),
            profile: config.profile.clone(),
            package_hash: binding.package_hash.to_string(),
            section_ids: binding.package_section_ids.clone(),
        }) {
            Ok(report) => report,
            Err(error) => return Err(cleanup_runtime_host(&mut host, error)),
        };
        if prepare.status != "pass" || !prepare.diagnostics.is_empty() {
            let error = NativeVnHostError::Package(format!(
                "ASTRA_PLAYER_RUNTIME_PREPARE_BLOCKED: provider preparation returned {} with {} diagnostics",
                prepare.status,
                prepare.diagnostics.len()
            ));
            return Err(cleanup_runtime_host(&mut host, error));
        }
        let probe = match host.probe(RuntimeProbeRequest {
            target_id: runtime_provider.target().to_string(),
            profile: config.profile.clone(),
            platform: None,
            section_ids: binding.package_section_ids,
        }) {
            Ok(report) => report,
            Err(error) => return Err(cleanup_runtime_host(&mut host, error)),
        };
        if probe.status != "pass" || !probe.diagnostics.is_empty() {
            let error = NativeVnHostError::Package(format!(
                "ASTRA_PLAYER_RUNTIME_PROBE_BLOCKED: provider probe returned {} with {} diagnostics",
                probe.status,
                probe.diagnostics.len()
            ));
            return Err(cleanup_runtime_host(&mut host, error));
        }
        let open = match host.open(RuntimeOpenRequest {
            target_id: runtime_provider.target().to_string(),
            profile: config.profile,
            locale: config.locale,
            seed: 0,
            package_hash: binding.package_hash.to_string(),
            sections: vec![compiled_section],
        }) {
            Ok(report) => report,
            Err(error) => return Err(cleanup_runtime_host(&mut host, error)),
        };
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
            text_provider: binding.presentation.text_provider,
            font_families: binding.presentation.font_families,
            text_resources: TextRenderResourceOwner::default(),
            localization: binding.presentation.localization,
            surface,
            command_sequence: 0,
            fixed_step: 0,
            session_seed: 0,
            width,
            height,
            textures: binding.presentation.textures,
            live_texture_ids: BTreeSet::new(),
            live_layout_ids: BTreeSet::new(),
            scene_draw: Vec::new(),
            last_draw: Vec::new(),
            last_step_evidence: None,
            terminal_routes,
            pending_timeline: Vec::new(),
            media_assets: binding.presentation.media_assets,
            pending_audio: Vec::new(),
            next_media_resource_id: 10_000,
            presentation_profile: binding.runtime_provider.profile().to_string(),
            presentation_manifest: binding.presentation.manifest,
            stage_director,
            shutdown_started: false,
        })
    }

    pub fn last_step_evidence(&self) -> Option<&NativeVnStepEvidence> {
        self.last_step_evidence.as_ref()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id.0
    }

    pub fn provider_id(&self) -> &'static str {
        astra_plugin_abi::NATIVE_VN_PROVIDER_ID
    }

    pub fn take_timeline_tasks(&mut self) -> Vec<PlayerTimelineTask> {
        std::mem::take(&mut self.pending_timeline)
    }

    pub(crate) fn restore_timeline_tasks(&mut self, mut tasks: Vec<PlayerTimelineTask>) {
        tasks.append(&mut self.pending_timeline);
        self.pending_timeline = tasks;
    }

    pub fn take_audio_requests(&mut self) -> Vec<NativeVnAudioOutput> {
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

    pub fn prepare_persistent_audio_open(
        &mut self,
        sample_rate: u32,
        channels: u16,
        max_buffered_frames: u32,
    ) -> Result<(PlayerHostResourceId, PlayerHostCommandBatch), NativeVnHostError> {
        if max_buffered_frames == 0 {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_AUDIO_BUFFER_BUDGET: persistent output capacity is zero".into(),
            ));
        }
        let output = self.next_media_resource()?;
        let batch = PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenAudio {
            sequence: self.next_command_sequence()?,
            output,
            sample_rate,
            channels,
            max_buffered_frames,
        }])?;
        Ok((output, batch))
    }

    pub fn prepare_audio_output_format_query(
        &mut self,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::QueryAudioFormat {
                sequence: self.next_command_sequence()?,
            },
        ])?)
    }

    pub fn prepare_persistent_audio_query(
        &mut self,
        output: PlayerHostResourceId,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::QueryAudio {
                sequence: self.next_command_sequence()?,
                output,
            },
        ])?)
    }

    pub fn prepare_persistent_audio_submit(
        &mut self,
        output: PlayerHostResourceId,
        packet_sequence: u64,
        audio: &astra_player_core::PlayerMixedAudio,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if audio.samples.is_empty() {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_AUDIO_PACKET_EMPTY: mixer produced no samples".into(),
            ));
        }
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::SubmitAudio {
                sequence: self.next_command_sequence()?,
                output,
                packet_sequence,
                channels: audio.channels,
                samples: audio.samples.clone(),
            },
        ])?)
    }

    pub fn prepare_persistent_audio_drain(
        &mut self,
        output: PlayerHostResourceId,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::DrainAudio {
                sequence: self.next_command_sequence()?,
                output,
            },
        ])?)
    }

    pub fn prepare_persistent_audio_close(
        &mut self,
        output: PlayerHostResourceId,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::CloseAudio {
                sequence: self.next_command_sequence()?,
                output,
            },
        ])?)
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
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentScene {
                sequence: self.command_sequence,
                surface: self.surface,
                width: self.width,
                height: self.height,
                clear_rgba: [8, 10, 16, 255],
                commands: self.last_draw.clone(),
            },
        ])?)
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

    pub fn release_resources(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_REPEATED: resource shutdown already started".to_string(),
            ));
        }
        let mut commands = self.text_resources.shutdown();
        commands.extend(self.live_texture_ids.iter().map(|resource_id| {
            SceneCommand::ReleaseResource {
                resource_id: resource_id.clone(),
            }
        }));
        commands.push(SceneCommand::rect(
            "vn.shutdown.clear",
            0,
            0,
            self.width,
            self.height,
            [0, 0, 0, 255],
        ));
        self.live_texture_ids.clear();
        self.live_layout_ids.clear();
        self.scene_draw.clear();
        self.shutdown_started = true;
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentScene {
                sequence: self.command_sequence,
                surface: self.surface,
                width: self.width,
                height: self.height,
                clear_rgba: [0, 0, 0, 255],
                commands,
            },
        ])?)
    }

    pub fn shutdown(mut self) -> Result<(), NativeVnHostError> {
        if !self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_ORDER: release_resources must execute before provider shutdown"
                    .to_string(),
            ));
        }
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
        if self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_STATE: runtime input arrived after shutdown started"
                    .to_string(),
            ));
        }
        self.fixed_step = self
            .fixed_step
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        let output = self.host.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step: self.fixed_step,
            delta_ns: 16_666_667,
            session_seed: self.session_seed,
            mode: RuntimeStepMode::Live,
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
            if command.command == "audio" {
                if let Some(action) = command.attributes.get("action") {
                    if !matches!(action.as_str(), "stop" | "pause" | "resume") {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_AUDIO_CONTROL_UNSUPPORTED: {}",
                            command.command_id
                        )));
                    }
                    let target = command.attributes.get("target").cloned().ok_or_else(|| {
                        NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_AUDIO_CONTROL_TARGET_REQUIRED: {}",
                            command.command_id
                        ))
                    })?;
                    self.pending_audio.push(NativeVnAudioOutput::Control(
                        NativeVnAudioControlRequest {
                            command_id: command.command_id,
                            action: action.clone(),
                            target,
                        },
                    ));
                    continue;
                }
            }
            let asset_id = command.attributes.get("asset").cloned().ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_AUDIO_ASSET_REQUIRED: {}",
                    command.command_id
                ))
            })?;
            let asset = self.media_assets.get(&asset_id).ok_or_else(|| {
                NativeVnHostError::Asset(format!("ASTRA_PLAYER_AUDIO_ASSET_MISSING: {asset_id}"))
            })?;
            self.pending_audio
                .push(NativeVnAudioOutput::Start(NativeVnAudioRequest {
                    command_id: command.command_id,
                    command: command.command,
                    attributes: command.attributes,
                    asset_id,
                    codec: asset.codec.clone(),
                    encoded_bytes: asset.bytes.clone(),
                    encoded_hash: asset.hash,
                }));
        }
        let presentation = output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Presentation)
            .map(|envelope| {
                envelope
                    .decode_postcard(
                        RuntimeOutputDomain::Presentation,
                        "astra.vn.presentation_command.v2",
                        SchemaVersion::new(2, 0, 0),
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
        let mut next_stage_director = self.stage_director.clone();
        for command in presentation {
            if let PresentationCommand::Stage(stage) = command {
                let outputs = next_stage_director
                    .apply(stage)
                    .map_err(stage_director_error)?;
                reject_unwired_stage_outputs(&outputs)?;
            }
        }
        let next_scene_draw = stage_scene_commands(
            next_stage_director.state(),
            &self.textures,
            self.width,
            self.height,
        )?;

        let next_texture_ids = scene_texture_ids(&next_scene_draw);
        let mut lifecycle = Vec::new();
        for asset_id in self.live_texture_ids.difference(&next_texture_ids) {
            lifecycle.push(SceneCommand::ReleaseResource {
                resource_id: asset_id.clone(),
            });
        }
        for asset_id in next_texture_ids.difference(&self.live_texture_ids) {
            lifecycle.push(SceneCommand::UploadTexture {
                resource_id: asset_id.clone(),
                frame: self.texture(asset_id)?.clone(),
            });
        }

        let mut next_text_resources = self.text_resources.clone();
        let mut next_layout_ids = BTreeSet::new();
        let mut text_lifecycle = Vec::new();
        let mut text_draw = Vec::new();
        let mut panel_draw = Vec::new();
        let body_font_size = (self.height as f32 / 30.0).clamp(18.0, 34.0);
        for command in presentation {
            match command {
                PresentationCommand::Dialogue { key, speaker, .. } => {
                    let panel_height = (self.height / 3).max(64);
                    panel_draw.push(SceneCommand::rect(
                        "vn.dialogue.panel",
                        24,
                        self.height.saturating_sub(panel_height + 24),
                        self.width.saturating_sub(48),
                        panel_height,
                        [18, 22, 34, 236],
                    ));
                    if let Some(speaker) = speaker {
                        let localization_key = format!("speaker.{speaker}");
                        append_text_layout(
                            &mut next_text_resources,
                            &self.text_provider,
                            &self.font_families,
                            &self.localization,
                            &mut next_layout_ids,
                            &mut text_lifecycle,
                            &mut text_draw,
                            "vn.dialogue.speaker",
                            &localization_key,
                            42,
                            self.height.saturating_sub(panel_height + 12),
                            self.width.saturating_sub(84),
                            (body_font_size * 0.78).max(16.0),
                            1,
                            [120, 210, 255, 255],
                        )?;
                    }
                    append_text_layout(
                        &mut next_text_resources,
                        &self.text_provider,
                        &self.font_families,
                        &self.localization,
                        &mut next_layout_ids,
                        &mut text_lifecycle,
                        &mut text_draw,
                        "vn.dialogue.body",
                        key,
                        42,
                        self.height.saturating_sub(panel_height.saturating_sub(42)),
                        self.width.saturating_sub(84),
                        body_font_size,
                        4,
                        [245, 245, 248, 255],
                    )?;
                }
                PresentationCommand::Choice { key, options } => {
                    append_text_layout(
                        &mut next_text_resources,
                        &self.text_provider,
                        &self.font_families,
                        &self.localization,
                        &mut next_layout_ids,
                        &mut text_lifecycle,
                        &mut text_draw,
                        "vn.choice.prompt",
                        key,
                        42,
                        32,
                        self.width.saturating_sub(84),
                        body_font_size,
                        2,
                        [245, 245, 248, 255],
                    )?;
                    let model = SystemUiModel::choice(self.width, self.height, options.len());
                    for (index, (option, control)) in
                        options.iter().zip(model.controls.iter()).enumerate()
                    {
                        panel_draw.push(SceneCommand::rect(
                            format!("vn.choice.{}", option.id),
                            control.bounds.x,
                            control.bounds.y,
                            control.bounds.width,
                            control.bounds.height.saturating_sub(4),
                            [30, 48, 70, 245],
                        ));
                        append_text_layout(
                            &mut next_text_resources,
                            &self.text_provider,
                            &self.font_families,
                            &self.localization,
                            &mut next_layout_ids,
                            &mut text_lifecycle,
                            &mut text_draw,
                            &format!("vn.choice.option.{index}"),
                            &option.key,
                            control.bounds.x.saturating_add(12),
                            control.bounds.y.saturating_add(10),
                            control.bounds.width.saturating_sub(24),
                            (body_font_size * 0.82).max(16.0),
                            1,
                            [255, 255, 255, 255],
                        )?;
                    }
                }
                PresentationCommand::SystemPage { page } => {
                    let model =
                        SystemUiModel::system(*page, self.width, self.height).ok_or_else(|| {
                            NativeVnHostError::Input(
                                "ASTRA_PLAYER_SYSTEM_PAGE: unknown system page".into(),
                            )
                        })?;
                    panel_draw.push(SceneCommand::rect(
                        "vn.system.panel",
                        0,
                        0,
                        self.width,
                        self.height,
                        [12, 18, 30, 252],
                    ));
                    append_text_layout(
                        &mut next_text_resources,
                        &self.text_provider,
                        &self.font_families,
                        &self.localization,
                        &mut next_layout_ids,
                        &mut text_lifecycle,
                        &mut text_draw,
                        "vn.system.title",
                        system_page_localization_key(*page)?,
                        42,
                        96,
                        self.width.saturating_sub(84),
                        (body_font_size * 1.25).min(42.0),
                        1,
                        [220, 230, 255, 255],
                    )?;
                    for control in model.controls {
                        panel_draw.push(SceneCommand::rect(
                            format!("vn.system.control.{}", control.id),
                            control.bounds.x,
                            control.bounds.y,
                            control.bounds.width,
                            control.bounds.height,
                            [38, 58, 84, 255],
                        ));
                        append_text_layout(
                            &mut next_text_resources,
                            &self.text_provider,
                            &self.font_families,
                            &self.localization,
                            &mut next_layout_ids,
                            &mut text_lifecycle,
                            &mut text_draw,
                            &format!("vn.system.control_text.{}", control.id),
                            "system.back",
                            control.bounds.x.saturating_add(16),
                            control.bounds.y.saturating_add(12),
                            control.bounds.width.saturating_sub(32),
                            (body_font_size * 0.72).max(16.0),
                            1,
                            [255; 4],
                        )?;
                    }
                }
                PresentationCommand::SystemOption { option } => {
                    let y = 180u32.saturating_add((next_layout_ids.len() as u32) * 64);
                    panel_draw.push(SceneCommand::rect(
                        format!("vn.system.option.{}", option.id),
                        42,
                        y,
                        self.width.saturating_sub(84),
                        54,
                        [38, 58, 84, 255],
                    ));
                    append_text_layout(
                        &mut next_text_resources,
                        &self.text_provider,
                        &self.font_families,
                        &self.localization,
                        &mut next_layout_ids,
                        &mut text_lifecycle,
                        &mut text_draw,
                        &format!("vn.system.option_text.{}", option.id),
                        &option.key,
                        58,
                        y.saturating_add(12),
                        self.width.saturating_sub(116),
                        (body_font_size * 0.72).max(16.0),
                        1,
                        [255; 4],
                    )?;
                }
                PresentationCommand::Stage(_) => {}
                PresentationCommand::Extension(extension) => {
                    return Err(NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_EXTENSION_PROVIDER_UNWIRED: command {} requires provider {}",
                        extension.command, extension.provider_id
                    )));
                }
                PresentationCommand::Marker { .. } => {}
            }
        }
        for layout_id in self.live_layout_ids.difference(&next_layout_ids) {
            text_lifecycle.extend(next_text_resources.remove_layout(layout_id)?);
        }
        lifecycle.extend(text_lifecycle);
        lifecycle.push(SceneCommand::rect(
            "vn.frame.clear",
            0,
            0,
            self.width,
            self.height,
            [8, 10, 16, 255],
        ));
        lifecycle.extend(next_scene_draw.iter().cloned());
        lifecycle.extend(panel_draw);
        lifecycle.extend(text_draw);
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        tracing::trace!(
            event = "player.vn.runtime.command.emit",
            sequence = self.command_sequence,
            presentation_count = presentation.len(),
            scene_command_count = lifecycle.len(),
            "emitted AstraVN Player host command"
        );
        self.last_draw = lifecycle.clone();
        self.scene_draw = next_scene_draw;
        self.stage_director = next_stage_director;
        self.live_texture_ids = next_texture_ids;
        self.text_resources = next_text_resources;
        self.live_layout_ids = next_layout_ids;
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentScene {
                sequence: self.command_sequence,
                surface: self.surface,
                width: self.width,
                height: self.height,
                clear_rgba: [8, 10, 16, 255],
                commands: lifecycle,
            },
        ])?)
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

fn cleanup_runtime_host(
    host: &mut ProductRuntimeHost,
    error: impl std::fmt::Display,
) -> NativeVnHostError {
    match host.cleanup_after_failure() {
        Ok(_) => NativeVnHostError::Package(error.to_string()),
        Err(cleanup_error) => NativeVnHostError::Package(format!(
            "ASTRA_PLAYER_RUNTIME_CLEANUP_FAILED: {error}; cleanup failed: {cleanup_error}"
        )),
    }
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

fn stage_scene_commands(
    state: &ProductStageState,
    textures: &BTreeMap<String, TextureFrame>,
    width: u32,
    height: u32,
) -> Result<Vec<SceneCommand>, NativeVnHostError> {
    if state.camera.rotation.millionths != 0 {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_CAMERA_ROTATION_UNWIRED: retained sprite renderer does not yet execute rotated camera geometry"
                .to_string(),
        ));
    }
    let mut entities = state
        .entities
        .values()
        .filter(|entity| entity.visible)
        .collect::<Vec<_>>();
    entities.sort_by(|left, right| {
        let left_z = state
            .layers
            .get(&left.layer)
            .map_or(i32::MIN, |layer| layer.z);
        let right_z = state
            .layers
            .get(&right.layer)
            .map_or(i32::MIN, |layer| layer.z);
        left_z.cmp(&right_z).then(left.id.cmp(&right.id))
    });
    let mut commands = Vec::new();
    for entity in entities {
        let layer = state.layers.get(&entity.layer).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_STAGE_LAYER_UNKNOWN: entity {} references an undeclared layer",
                entity.id
            ))
        })?;
        let texture = textures
            .get(&entity.asset)
            .ok_or_else(|| missing_texture(&entity.asset))?;
        let clip = match layer.clip {
            Some(StageClipPolicy::SafeArea) => Some(safe_area_rect(
                width,
                height,
                state.safe_area.width,
                state.safe_area.height,
            )?),
            Some(StageClipPolicy::Stage) => Some(RectI::new(0, 0, width, height)),
            None => None,
        };
        if let Some(clip) = clip {
            commands.push(SceneCommand::PushClip { rect: clip });
        }
        let destination = if layer.kind == StageLayerKind::Background {
            RectI::new(0, 0, width, height)
        } else {
            entity_destination(
                state,
                texture,
                entity.x.millionths,
                entity.y.millionths,
                width,
                height,
            )?
        };
        commands.push(SceneCommand::Sprite {
            id: format!("vn.scene.{}.{}", entity.layer, entity.id),
            texture_id: entity.asset.clone(),
            source: None,
            destination,
            opacity: fixed_opacity(entity.opacity.millionths)?,
            blend: stage_blend(layer.blend)?,
        });
        if clip.is_some() {
            commands.push(SceneCommand::PopClip);
        }
    }
    Ok(commands)
}

fn entity_destination(
    state: &ProductStageState,
    texture: &TextureFrame,
    entity_x: i64,
    entity_y: i64,
    width: u32,
    height: u32,
) -> Result<RectI, NativeVnHostError> {
    let zoom = state.camera.zoom.millionths;
    if zoom <= 0 {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_CAMERA_ZOOM: camera zoom must remain positive".to_string(),
        ));
    }
    let base_height = height.saturating_mul(9) / 10;
    let base_width = ((u64::from(base_height) * u64::from(texture.width))
        / u64::from(texture.height))
    .min(u64::from(width));
    let destination_width = scale_dimension(base_width, zoom)?;
    let destination_height = scale_dimension(u64::from(base_height), zoom)?;
    let center_x = camera_coordinate(
        entity_x,
        state.camera.x.millionths + state.camera.shake_x.millionths,
        i64::from(state.viewport.width) * 500_000,
        i64::from(width) * 500_000,
        zoom,
    )?;
    let bottom_y = camera_coordinate(
        entity_y,
        state.camera.y.millionths + state.camera.shake_y.millionths,
        i64::from(state.viewport.height) * 500_000,
        i64::from(height) * 500_000,
        zoom,
    )?;
    let x = center_x - i64::from(destination_width) / 2;
    let y = bottom_y - i64::from(destination_height);
    Ok(RectI::new(
        i32::try_from(x).map_err(|_| stage_coordinate_error())?,
        i32::try_from(y).map_err(|_| stage_coordinate_error())?,
        destination_width,
        destination_height,
    ))
}

fn camera_coordinate(
    value: i64,
    camera: i64,
    stage_center: i64,
    output_center: i64,
    zoom: i64,
) -> Result<i64, NativeVnHostError> {
    let centered = i128::from(value) - i128::from(camera) - i128::from(stage_center);
    let scaled = centered
        .checked_mul(i128::from(zoom))
        .ok_or_else(stage_coordinate_error)?
        / 1_000_000;
    i64::try_from((scaled + i128::from(output_center)) / 1_000_000)
        .map_err(|_| stage_coordinate_error())
}

fn scale_dimension(value: u64, zoom: i64) -> Result<u32, NativeVnHostError> {
    let scaled = u128::from(value)
        .checked_mul(u128::try_from(zoom).map_err(|_| stage_coordinate_error())?)
        .ok_or_else(stage_coordinate_error)?
        / 1_000_000;
    u32::try_from(scaled.max(1)).map_err(|_| stage_coordinate_error())
}

fn safe_area_rect(
    width: u32,
    height: u32,
    ratio_width: u32,
    ratio_height: u32,
) -> Result<RectI, NativeVnHostError> {
    if ratio_width == 0 || ratio_height == 0 {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_SAFE_AREA: safe-area ratio is invalid".to_string(),
        ));
    }
    let candidate_height = u64::from(width) * u64::from(ratio_height) / u64::from(ratio_width);
    let (safe_width, safe_height) = if candidate_height <= u64::from(height) {
        (
            width,
            u32::try_from(candidate_height).map_err(|_| stage_coordinate_error())?,
        )
    } else {
        (
            u32::try_from(u64::from(height) * u64::from(ratio_width) / u64::from(ratio_height))
                .map_err(|_| stage_coordinate_error())?,
            height,
        )
    };
    Ok(RectI::new(
        ((width - safe_width) / 2) as i32,
        ((height - safe_height) / 2) as i32,
        safe_width,
        safe_height,
    ))
}

fn fixed_opacity(millionths: i64) -> Result<f32, NativeVnHostError> {
    if !(0..=1_000_000).contains(&millionths) {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_STAGE_OPACITY: stage opacity is outside zero and one".to_string(),
        ));
    }
    Ok(millionths as f32 / 1_000_000.0)
}

fn stage_blend(blend: StageBlendMode) -> Result<BlendMode, NativeVnHostError> {
    match blend {
        StageBlendMode::Normal => Ok(BlendMode::Alpha),
        StageBlendMode::Add | StageBlendMode::Multiply | StageBlendMode::Screen => {
            Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_STAGE_BLEND_UNWIRED: selected stage blend has no product GPU binding"
                    .to_string(),
            ))
        }
    }
}

fn stage_coordinate_error() -> NativeVnHostError {
    NativeVnHostError::Asset(
        "ASTRA_PLAYER_STAGE_COORDINATE: stage transform exceeds renderer coordinate limits"
            .to_string(),
    )
}

fn reject_unwired_stage_outputs(outputs: &[StageDirectorOutput]) -> Result<(), NativeVnHostError> {
    if let Some(output) = outputs.first() {
        let domain = match output {
            StageDirectorOutput::Audio(_) => "audio",
            StageDirectorOutput::Movie(_) => "movie",
            StageDirectorOutput::Effect(_) => "effect",
            StageDirectorOutput::FenceCompleted { .. } => "completion",
        };
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_PRESENTATION_OUTPUT_UNWIRED: product stage {domain} output has no host execution binding"
        )));
    }
    Ok(())
}

fn stage_director_error(error: astra_vn_package::VnError) -> NativeVnHostError {
    NativeVnHostError::Asset(format!("{}: {error}", error.code()))
}

fn missing_texture(asset_id: &str) -> NativeVnHostError {
    NativeVnHostError::Asset(format!(
        "ASTRA_PLAYER_ASSET_MISSING: cooked texture {asset_id} is not mounted"
    ))
}

fn scene_texture_ids(draw: &[SceneCommand]) -> BTreeSet<String> {
    draw.iter()
        .filter_map(|command| match command {
            SceneCommand::Sprite { texture_id, .. } => Some(texture_id.clone()),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn append_text_layout(
    owner: &mut TextRenderResourceOwner,
    provider: &CosmicTextLayoutProvider,
    font_families: &[String],
    localization: &VnLocalizationTable,
    layout_ids: &mut BTreeSet<String>,
    lifecycle: &mut Vec<SceneCommand>,
    draw: &mut Vec<SceneCommand>,
    layout_id: &str,
    localization_key: &str,
    x: u32,
    y: u32,
    max_width: u32,
    font_size: f32,
    max_lines: u32,
    rgba: [u8; 4],
) -> Result<(), NativeVnHostError> {
    let text = resolve_localized(localization, localization_key)?;
    let request = text_request(
        localization_key,
        text,
        &localization.locale,
        font_families,
        max_width,
        font_size,
        max_lines,
    );
    let layout = provider.layout(&request)?;
    let commands = owner.update_layout(layout_id, &layout, rgba)?;
    for command in commands {
        match command {
            SceneCommand::UploadGlyph { .. } | SceneCommand::ReleaseResource { .. } => {
                lifecycle.push(command);
            }
            command => draw.push(translate_text_command(command, x, y)?),
        }
    }
    if !layout_ids.insert(layout_id.to_string()) {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_LAYOUT_DUPLICATE: layout id {layout_id} was emitted twice"
        )));
    }
    Ok(())
}

fn text_request(
    key: &str,
    text: &str,
    locale: &str,
    font_families: &[String],
    max_width: u32,
    font_size: f32,
    max_lines: u32,
) -> TextLayoutRequest {
    let line_height = font_size * 1.3;
    TextLayoutRequest {
        key: key.to_string(),
        runs: vec![TextRun {
            text: text.to_string(),
            language: locale.to_string(),
            script: None,
            direction: TextDirection::Auto,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: max_width.max(1) as f32,
            max_height: Some(line_height * max_lines.max(1) as f32),
            max_lines: Some(max_lines.max(1)),
            font_size,
            line_height,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::EllipsisEnd,
        },
        font_families: font_families.to_vec(),
        features: Vec::new(),
    }
}

fn translate_text_command(
    mut command: SceneCommand,
    x: u32,
    y: u32,
) -> Result<SceneCommand, NativeVnHostError> {
    let x = i32::try_from(x).map_err(|_| {
        NativeVnHostError::Asset("ASTRA_PLAYER_LAYOUT_COORDINATE: x exceeds i32".to_string())
    })?;
    let y = i32::try_from(y).map_err(|_| {
        NativeVnHostError::Asset("ASTRA_PLAYER_LAYOUT_COORDINATE: y exceeds i32".to_string())
    })?;
    match &mut command {
        SceneCommand::GlyphRun { glyphs, .. } => {
            for glyph in glyphs {
                glyph.x = glyph.x.checked_add(x).ok_or_else(|| {
                    NativeVnHostError::Asset(
                        "ASTRA_PLAYER_LAYOUT_COORDINATE: glyph x overflowed".to_string(),
                    )
                })?;
                glyph.y = glyph.y.checked_add(y).ok_or_else(|| {
                    NativeVnHostError::Asset(
                        "ASTRA_PLAYER_LAYOUT_COORDINATE: glyph y overflowed".to_string(),
                    )
                })?;
            }
        }
        SceneCommand::PushClip { rect } => {
            rect.x = rect.x.checked_add(x).ok_or_else(|| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_LAYOUT_COORDINATE: clip x overflowed".to_string(),
                )
            })?;
            rect.y = rect.y.checked_add(y).ok_or_else(|| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_LAYOUT_COORDINATE: clip y overflowed".to_string(),
                )
            })?;
        }
        SceneCommand::PopClip => {}
        _ => {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_LAYOUT_COMMAND: text owner emitted an unexpected draw command"
                    .to_string(),
            ))
        }
    }
    Ok(command)
}

fn system_page_localization_key(page: SystemPageKind) -> Result<&'static str, NativeVnHostError> {
    match page {
        SystemPageKind::Title => Ok("system.title"),
        SystemPageKind::Save => Ok("system.save"),
        SystemPageKind::Load => Ok("system.load"),
        SystemPageKind::Config => Ok("system.config"),
        SystemPageKind::Gallery => Ok("system.gallery"),
        SystemPageKind::Replay => Ok("system.replay"),
        SystemPageKind::VoiceReplay => Ok("system.voice_replay"),
        SystemPageKind::RouteChart => Ok("system.route_chart"),
        SystemPageKind::Backlog => Ok("system.backlog"),
        SystemPageKind::LocalizationPreview => Ok("system.localization_preview"),
        SystemPageKind::Unknown => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_SYSTEM_PAGE: unknown system page".to_string(),
        )),
    }
}

fn resolve_localized<'a>(
    localization: &'a VnLocalizationTable,
    key: &str,
) -> Result<&'a str, NativeVnHostError> {
    localization
        .resolve(key)
        .map_err(|error| NativeVnHostError::Localization(error.to_string()))
}

fn validate_story_text(
    compiled: &CompiledStory,
    localization: &VnLocalizationTable,
    provider: &CosmicTextLayoutProvider,
    font_families: &[String],
) -> Result<(), NativeVnHostError> {
    let mut keys = BTreeSet::new();
    for command in compiled
        .states
        .values()
        .flat_map(|state| &state.scenes)
        .flat_map(|scene| &scene.commands)
    {
        match command {
            CompiledCommand::Dialogue { key, speaker, .. } => {
                keys.insert(key.clone());
                if let Some(speaker) = speaker {
                    keys.insert(format!("speaker.{speaker}"));
                }
            }
            CompiledCommand::Choice { key, options, .. } => {
                keys.insert(key.clone());
                keys.extend(options.iter().map(|option| option.key.clone()));
            }
            CompiledCommand::SystemPage { page, .. } => {
                keys.insert(system_page_localization_key(*page)?.to_string());
                keys.insert("system.back".to_string());
            }
            CompiledCommand::Presentation { command, .. } => match command {
                PresentationCommand::Dialogue { key, speaker, .. } => {
                    keys.insert(key.clone());
                    if let Some(speaker) = speaker {
                        keys.insert(format!("speaker.{speaker}"));
                    }
                }
                PresentationCommand::Choice { key, options } => {
                    keys.insert(key.clone());
                    keys.extend(options.iter().map(|option| option.key.clone()));
                }
                PresentationCommand::SystemPage { page } => {
                    keys.insert(system_page_localization_key(*page)?.to_string());
                    keys.insert("system.back".to_string());
                }
                PresentationCommand::SystemOption { option } => {
                    keys.insert(option.key.clone());
                }
                PresentationCommand::Stage(_)
                | PresentationCommand::Extension(_)
                | PresentationCommand::Marker { .. } => {}
            },
            _ => {}
        }
    }
    for key in keys {
        let text = resolve_localized(localization, &key)?;
        provider.layout(&text_request(
            &format!("preflight.{key}"),
            text,
            &localization.locale,
            font_families,
            4096,
            28.0,
            16,
        ))?;
    }
    Ok(())
}

fn validate_story_presentation(
    compiled: &CompiledStory,
    textures: &BTreeMap<String, TextureFrame>,
    manifest: &VnPresentationProviderManifest,
    profile: &str,
) -> Result<(), NativeVnHostError> {
    for command in compiled
        .states
        .values()
        .flat_map(|state| &state.scenes)
        .flat_map(|scene| &scene.commands)
    {
        let CompiledCommand::Presentation { command, .. } = command else {
            continue;
        };
        match command {
            PresentationCommand::Stage(stage) => {
                validate_stage_command_policy(stage, manifest, profile)?;
                match stage {
                    StageCommand::Background { asset: asset_id, .. }
                    | StageCommand::Show {
                        asset: asset_id, ..
                    } => {
                        if !textures.contains_key(asset_id) {
                            return Err(missing_texture(asset_id));
                        }
                    }
                    StageCommand::Configure { .. }
                    | StageCommand::Hide { .. }
                    | StageCommand::Move { .. } => {}
                    StageCommand::DeclareLayer { blend, .. } => {
                        if *blend != StageBlendMode::Normal {
                            return Err(NativeVnHostError::Asset(
                                "ASTRA_PLAYER_STAGE_BLEND_UNWIRED: selected stage blend has no product GPU binding"
                                    .to_string(),
                            ));
                        }
                    }
                    StageCommand::Camera { rotation, .. } => {
                        if rotation.millionths != 0 {
                            return Err(NativeVnHostError::Asset(
                                "ASTRA_PLAYER_CAMERA_ROTATION_UNWIRED: retained sprite renderer does not yet execute rotated camera geometry"
                                    .to_string(),
                            ));
                        }
                    }
                    unsupported => {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_PRESENTATION_UNSUPPORTED: typed stage command {} has no product renderer binding",
                            unsupported.kind()
                        )))
                    }
                }
            }
            PresentationCommand::Extension(extension) => {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_EXTENSION_PROVIDER_UNWIRED: command {} requires provider {}",
                    extension.command, extension.provider_id
                )))
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_stage_command_policy(
    command: &StageCommand,
    manifest: &VnPresentationProviderManifest,
    profile: &str,
) -> Result<(), NativeVnHostError> {
    let preset = match command {
        StageCommand::Background { preset, .. }
        | StageCommand::Show { preset, .. }
        | StageCommand::Hide { preset, .. }
        | StageCommand::Move { preset, .. }
        | StageCommand::Camera { preset, .. } => preset.as_deref(),
        StageCommand::Transition { preset, .. } => Some(preset.as_str()),
        _ => None,
    };
    if let Some(preset) = preset {
        manifest
            .resolve_preset(profile, command.kind(), preset)
            .map_err(|diagnostic| {
                NativeVnHostError::Package(format!("{}: {}", diagnostic.code, diagnostic.message))
            })?;
    }
    Ok(())
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
    #[error("localization failed: {0}")]
    Localization(String),
    #[error("player input failed: {0}")]
    Input(String),
    #[error("runtime evidence failed: {0}")]
    RuntimeEvidence(String),
    #[error("provider binding failed: {0}")]
    ProviderBinding(String),
    #[error("player save failed: {0}")]
    Save(String),
}
