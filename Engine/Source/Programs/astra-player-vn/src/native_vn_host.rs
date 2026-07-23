use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use astra_core::{Hash256, SchemaVersion};
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, TextDirection,
    TextLayoutConfig, TextLayoutProvider, TextLayoutRequest, TextLayoutResult,
    TextRenderLayoutUpdate, TextRenderResourceOwner, TextRun, WrapPolicy,
};
use astra_media_core::{
    BlendMode, FilterGraph, FilterNode, FilterParam, FilterTarget, MediaError, RectI, SceneCommand,
    TextureFrame, Transform2D,
};
use astra_player_core::{
    PlayerAudioLifecyclePlan, PlayerDecodeKind, PlayerDecodeLifecyclePlan, PlayerDecodedAudio,
    PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandError, PlayerHostResourceId,
    PlayerSaveTransactionPlan, PlayerTimelineTask, PlayerTimelineTaskAction,
};
use astra_plugin::{ProductRuntimeHost, RuntimeHostError, RuntimeHostSchemaRegistry};
use astra_plugin_abi::{
    GameRuntimeSessionId, RuntimeOpenRequest, RuntimeOutputDomain, RuntimePrepareRequest,
    RuntimeProbeRequest, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections,
    RuntimeSectionCodec, RuntimeSectionPayload, RuntimeStepInput, RuntimeStepMode,
    ValidatedRuntimeProviderSelection, NATIVE_VN_PROVIDER_ID,
};
use astra_ui_core::{
    UiBackend, UiBlueprintBundle, UiBlueprintFrameModel, UiBlueprintModalFrameModel, UiButtonState,
    UiFrameRequest, UiInputDisposition, UiInputDispositionKind, UiInputEvent, UiInputEventKind,
    UiInputFrame, UiInsets, UiNodeBlueprint, UiPerformanceBudget, UiPerformanceGate,
    UiPerformanceReport, UiPerformanceSample, UiPoint, UiSemanticNode, UiSemanticRole,
    UiSemanticSnapshot, UiThemeManifest, UiThemeValue, UiValidationError, UiValue, UiValueExpr,
    UiViewport, ValidateUi, MAX_EFFECTS_PER_CALL, MAX_MODAL_DEPTH,
};
use astra_ui_yakui::{
    ui_frame_to_scene_commands, AstraTextMeasureRequest, AstraTextMeasureResult, AstraTextMeasurer,
    AstraYakuiBackend, BlueprintYakuiRenderer,
};
use astra_vn_core::{
    CompiledCommand, CompiledStory, MovieLoopMode, PresentationCommand, ReadingMode,
    SaveCompletionPolicy, StageBlendMode, StageClipPolicy, StageCommand, StageFitMode,
    StageLayerKind, State, SystemActionEffect, SystemPageKind, SystemUiProfilePolicy,
    TimelineCommand, VnAudioBus, VnAudioControlAction, VnAudioSync, VnPlayerCommand, VnRunConfig,
    VnRuntimeState, VnRuntimeViewState, VnWaitKind, VN_RUNTIME_STATE_SCHEMA,
    VN_RUNTIME_VIEW_STATE_SCHEMA, VN_RUNTIME_VIEW_STATE_SCHEMA_MAJOR,
};
use astra_vn_package::{
    decode_compiled_project, load_localization as load_package_localization,
    load_player_locale_config, load_presentation_provider_manifest, CompiledVnProject,
    ProductStageDirector, ProductStageState, StageDirectorOutput, VnLocalizationTable,
    VnPresentationProviderManifest, VnSystemUiProfileManifest,
};
use astra_vn_policy::LuauUiControllerHost;
use astra_vn_runtime_provider::NativeVnRuntimeProvider;
use astra_vn_ui::{
    model_to_ui_value, resolve_binding, SaveSlotViewModel, VnUiAction, VnUiBindingError,
    VnUiBindingRequest, VnUiControllerEffect, VnUiControllerUpdate, VnUiModelContext,
    VnUiSessionState,
};

use crate::package_assets::{PackageAssetStore, PackageImagePrefetcher};
use crate::ui_session::{
    controller_state_value, ActiveUiAnimation, ActiveUiController, ActiveUiModal,
};

pub const DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES: u64 = 192 * 1024 * 1024;
// The atlas itself is charged by the renderer. Keep the launch upload window
// below the resident CPU asset window: authoring a larger upload batch can
// transiently retain staging allocations and violate the profile work-set cap.
pub const DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeVnDecodedCacheBudget {
    pub asset_bytes: u64,
    pub audio_bytes: u64,
    pub glyph_bytes: u64,
}

impl NativeVnDecodedCacheBudget {
    pub fn partition(total_bytes: u64) -> Result<Self, NativeVnHostError> {
        if total_bytes < 4 {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_DECODED_CACHE_BUDGET_TOO_SMALL".into(),
            ));
        }
        // NativeVN images are retained only as a bounded working set and can be
        // repopulated by the package image prefetcher. Canonical PCM is much
        // larger than its encoded source and must be ready before an authored
        // audio start, so reserve the larger share for audio to keep decode and
        // sample-rate conversion off the presentation-critical path.
        let asset_bytes = total_bytes / 4;
        // Glyph bitmaps are compact Alpha8 payloads. Reserve one part in 192
        // for their dormant LRU so the audio prewarm keeps the authored route
        // working set resident while the three domains remain within the
        // profile-bound decoded-cache total.
        let glyph_bytes = total_bytes / 192;
        let audio_bytes = total_bytes
            .checked_sub(asset_bytes)
            .and_then(|remaining| remaining.checked_sub(glyph_bytes))
            .ok_or_else(|| {
                NativeVnHostError::Asset("ASTRA_PLAYER_DECODED_CACHE_BUDGET_TOO_SMALL".into())
            })?;
        if asset_bytes == 0 || audio_bytes == 0 || glyph_bytes == 0 {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_DECODED_CACHE_BUDGET_TOO_SMALL".into(),
            ));
        }
        Ok(Self {
            asset_bytes,
            audio_bytes,
            glyph_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VnUiHostRequest {
    Save {
        slot_id: String,
        completion: SaveCompletionPolicy,
    },
    Load {
        slot_id: String,
    },
    Delete {
        slot_id: String,
    },
}

pub struct NativeVnHostCommandSource {
    host: ProductRuntimeHost,
    session_id: GameRuntimeSessionId,
    runtime_state: Option<VnRuntimeState>,
    runtime_backlog_count: usize,
    font_families: Vec<String>,
    available_font_families: Vec<String>,
    text_resources: TextRenderResourceOwner,
    localization: VnLocalizationTable,
    localization_keys: Vec<String>,
    localizations: BTreeMap<String, VnLocalizationTable>,
    ui_text_locale: Arc<RwLock<String>>,
    ui_text_font_families: Arc<RwLock<Vec<String>>>,
    ui_text_measurer: Arc<NativeUiTextMeasurer>,
    surface: PlayerHostResourceId,
    command_sequence: u64,
    fixed_step: u64,
    session_seed: u64,
    next_step_mode: RuntimeStepMode,
    width: u32,
    height: u32,
    ui_viewport: UiViewport,
    textures: BTreeMap<String, TextureFrame>,
    asset_store: Arc<PackageAssetStore>,
    image_prefetcher: PackageImagePrefetcher,
    image_prefetch_windows: BTreeMap<String, Vec<String>>,
    image_prefetch_inflight: BTreeSet<String>,
    image_prefetch_failure: Option<String>,
    live_texture_ids: BTreeSet<String>,
    live_texture_bytes: BTreeMap<String, u64>,
    texture_last_used: BTreeMap<String, u64>,
    texture_cpu_last_used: BTreeMap<String, u64>,
    texture_use_clock: u64,
    texture_cpu_bytes: u64,
    texture_cpu_budget_bytes: u64,
    resident_texture_bytes: u64,
    gpu_texture_budget_bytes: u64,
    live_layout_ids: BTreeSet<String>,
    scene_draw: Vec<SceneCommand>,
    last_step_evidence: Option<NativeVnStepEvidence>,
    terminal_routes: std::collections::BTreeSet<String>,
    pending_timeline: Vec<PlayerTimelineTask>,
    pending_audio: Vec<NativeVnAudioOutput>,
    pending_audio_preloads: Vec<NativeVnAudioPreloadRequest>,
    audio_preload_story_ids: BTreeSet<String>,
    pending_video: Vec<NativeVnVideoRequest>,
    pending_stage_completions: Vec<String>,
    next_media_resource_id: u64,
    stage_director: ProductStageDirector,
    restored_product_media_snapshot: Option<Vec<u8>>,
    story: CompiledStory,
    ui_blueprints: astra_ui_core::UiBlueprintBundle,
    ui_view_localization_keys: BTreeMap<String, BTreeSet<String>>,
    ui_bindings: astra_ui_core::UiBindingManifest,
    ui_backend: AstraYakuiBackend<BlueprintYakuiRenderer>,
    ui_themes: BTreeMap<String, UiThemeManifest>,
    ui_profile: String,
    system_ui_policy: SystemUiProfilePolicy,
    ui_generation: u64,
    ui_input_sequence: u64,
    ui_draw: Vec<SceneCommand>,
    ui_semantics: Option<UiSemanticSnapshot>,
    ui_save_slots: BTreeMap<String, SaveSlotViewModel>,
    pending_ui_host_request: Option<VnUiHostRequest>,
    gameplay_thumbnail_capture: Option<TextureFrame>,
    pending_save_metadata: Option<NativeVnSaveMetadata>,
    pending_save_completion: Option<SaveCompletionPolicy>,
    exit_requested: bool,
    ui_controller_host: LuauUiControllerHost,
    ui_controller_sessions: BTreeMap<String, VnUiSessionState>,
    base_ui_instance_id: Option<String>,
    base_ui_theme_id: Option<String>,
    active_ui_controller: Option<ActiveUiController>,
    ui_modals: Vec<ActiveUiModal>,
    pending_ui_focus: Option<String>,
    ui_animations: BTreeMap<String, ActiveUiAnimation>,
    ui_performance: UiPerformanceGate,
    last_ui_performance_sample: Option<UiPerformanceSample>,
    ui_frame_reuse: Option<NativeVnUiFrameReuse>,
    ui_host_performance_sampling_enabled: bool,
    last_ui_host_performance_sample: Option<NativeVnUiHostPerformanceSample>,
    shutdown_started: bool,
}

struct NativeVnUiFrameResult {
    actions: Vec<astra_ui_core::UiActionEnvelope>,
    dispositions: Vec<UiInputDisposition>,
    semantics: UiSemanticSnapshot,
    draw: Vec<SceneCommand>,
}

#[derive(Clone, PartialEq)]
struct NativeVnUiFrameReuseKey {
    session_id: String,
    generation: u64,
    instance_id: String,
    viewport: UiViewport,
    theme_hash: Hash256,
    model_schema: String,
    model_payload: Vec<u8>,
}

impl NativeVnUiFrameReuseKey {
    // `fixed_time_ns` is intentionally excluded. A time-driven view must return
    // `repaint_after_ns`; only outputs that explicitly declare no repaint are
    // admitted to this cache. Controller animation progress is already part of
    // the serialized model payload.
    fn from_request(request: &UiFrameRequest, instance_id: &str) -> Self {
        Self {
            session_id: request.session_id.clone(),
            generation: request.generation,
            instance_id: instance_id.to_string(),
            viewport: request.viewport.clone(),
            theme_hash: request.theme.content_hash,
            model_schema: request.model_schema.clone(),
            model_payload: request.model_payload.clone(),
        }
    }

    fn matches(&self, request: &UiFrameRequest, instance_id: &str) -> bool {
        self.session_id == request.session_id
            && self.generation == request.generation
            && self.instance_id == instance_id
            && self.viewport == request.viewport
            && self.theme_hash == request.theme.content_hash
            && self.model_schema == request.model_schema
            && self.model_payload == request.model_payload
    }
}

struct NativeVnUiFrameReuse {
    key: NativeVnUiFrameReuseKey,
    pointer_position: Option<UiPoint>,
    performance: UiPerformanceSample,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeVnUiHostPerformanceSample {
    pub model_binding_ns: u64,
    pub controller_ns: u64,
    pub frame_model_ns: u64,
    pub text_scene_ns: u64,
    pub text_layout_ns: u64,
    pub text_resource_ns: u64,
    pub text_compose_ns: u64,
    pub action_dispatch_ns: u64,
    pub present_scene_ns: u64,
    pub runtime_host_step_ns: u64,
    pub runtime_output_decode_ns: u64,
    pub runtime_render_ns: u64,
    pub stage_prepare_ns: u64,
    pub stage_scene_ns: u64,
    pub stage_texture_ns: u64,
    pub stage_command_ns: u64,
    pub stage_lifecycle_ns: u64,
    pub scene_compose_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnAudioRequest {
    pub command_id: String,
    pub command: String,
    pub attributes: BTreeMap<String, String>,
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: Arc<[u8]>,
    pub encoded_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnAudioPreloadRequest {
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: Arc<[u8]>,
    pub encoded_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnAudioControlRequest {
    pub command_id: String,
    pub action: String,
    pub target: String,
    pub duration_ms: Option<u32>,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeVnAudioOutput {
    Start(NativeVnAudioRequest),
    Control(NativeVnAudioControlRequest),
}

enum NativeVnOrderedRuntimeOutput {
    AudioStart(NativeVnAudioRequest),
    Presentation(PresentationCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnVideoRequest {
    pub layer: String,
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: Arc<[u8]>,
    pub encoded_hash: Hash256,
    pub alpha_millionths: i64,
    pub looping: bool,
    pub fence: Option<String>,
    pub fallback_asset_id: Option<String>,
    pub allow_fallback: bool,
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
    pub pending_wait_command_id: Option<String>,
    pub pending_wait_await_id: Option<String>,
    pub pending_choice_ids: Vec<String>,
    pub terminal_route_ids: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnProductObservationEvidence {
    pub schema: String,
    pub ui_profile: String,
    pub locale: String,
    pub active_system_page: Option<SystemPageKind>,
    pub focused_semantic_id: Option<String>,
    pub auto_enabled: bool,
    pub skip_mode: astra_vn_core::SkipMode,
    pub reading_mode: ReadingMode,
    pub audio_enabled: bool,
    pub skip_allowed: bool,
    pub system_config: BTreeMap<String, String>,
    pub backlog_count: usize,
    pub occupied_save_slot_count: usize,
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
    stage_director: ProductStageDirector,
    step_evidence: NativeVnStepEvidence,
    draw_commands_json: Vec<u8>,
    draw_commands_hash: Hash256,
    product_media_snapshot_json: Option<Vec<u8>>,
    product_media_snapshot_hash: Option<Hash256>,
    save_metadata: NativeVnSaveMetadata,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NativeVnPlayerSaveEnvelope {
    schema: String,
    payload_hash: Hash256,
    payload: NativeVnPlayerSavePayload,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NativeVnSaveMetadata {
    slot_id: String,
    thumbnail_asset: String,
    thumbnail: TextureFrame,
    timestamp_text: String,
    playtime_text: String,
}

struct ProductPresentationBinding {
    asset_store: Arc<PackageAssetStore>,
    localization: VnLocalizationTable,
    localizations: BTreeMap<String, VnLocalizationTable>,
    text_provider: Arc<CosmicTextLayoutProvider>,
    font_families: Vec<String>,
    manifest: VnPresentationProviderManifest,
}

struct ProductPackageBinding {
    runtime_provider: ValidatedRuntimeProviderSelection,
    package_hash: Hash256,
    package_section_ids: Vec<String>,
    presentation: ProductPresentationBinding,
    system_ui_policy: SystemUiProfilePolicy,
}

struct NativeVnHostCacheBudget {
    asset_bytes: u64,
    glyph_bytes: usize,
}

impl NativeVnHostCommandSource {
    pub fn from_package(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        let budget = NativeVnDecodedCacheBudget::partition(DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES)?;
        Self::from_package_with_asset_cache(
            package,
            config,
            width,
            height,
            surface,
            budget.asset_bytes,
            budget.glyph_bytes,
        )
    }

    pub fn from_package_with_asset_cache(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        max_asset_cache_bytes: u64,
        max_glyph_cache_bytes: u64,
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
        let compiled = decode_compiled_project(package)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        let system_ui_manifest = package
            .container()
            .decode_postcard::<VnSystemUiProfileManifest>("vn.system_ui_profile_manifest")
            .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        let validation = system_ui_manifest.validate();
        if validation.status != astra_vn_core::SystemStoryValidationStatus::Pass {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_SYSTEM_UI_PROFILE_BLOCKED: package system UI policy is invalid"
                    .into(),
            ));
        }
        let system_ui_policy = system_ui_manifest
            .profiles
            .get(&config.profile)
            .cloned()
            .ok_or_else(|| {
                NativeVnHostError::Package(format!(
                    "ASTRA_PLAYER_SYSTEM_UI_PROFILE_MISSING: profile {} has no declared policy",
                    config.profile
                ))
            })?;
        let asset_store = PackageAssetStore::index(package, max_asset_cache_bytes)?;
        let presentation_manifest =
            load_presentation_provider_manifest(package, &config.profile)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        validate_story_presentation(
            &compiled,
            &asset_store,
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
        let localizations = locale_config
            .available_locales
            .iter()
            .map(|locale| {
                load_package_localization(package, locale, 16 * 1024 * 1024)
                    .map(|table| (locale.clone(), table))
                    .map_err(|error| NativeVnHostError::Localization(error.to_string()))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let localization = localizations.get(&config.locale).cloned().ok_or_else(|| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_LOCALE_TABLE_MISSING: declared locale has no loaded table".into(),
            )
        })?;
        let text_provider = Arc::new(CosmicTextLayoutProvider::from_package(
            package,
            "media.font_manifest",
            FontBindingContext {
                target: runtime_provider.target().to_string(),
                profile: config.profile.clone(),
                default_locale: config.locale.clone(),
            },
            TextLayoutConfig::production_defaults(),
        )?);
        let font_families = text_provider
            .identity()?
            .fonts
            .into_iter()
            .map(|font| font.family)
            .collect::<Vec<_>>();
        validate_story_text(&compiled, &localization, &text_provider, &font_families)?;
        text_provider.clear_layout_cache()?;
        let max_glyph_cache_bytes = usize::try_from(max_glyph_cache_bytes).map_err(|_| {
            NativeVnHostError::Asset("ASTRA_PLAYER_GLYPH_CACHE_BUDGET_PLATFORM_OVERFLOW".into())
        })?;
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
                    asset_store,
                    localization,
                    localizations,
                    text_provider,
                    font_families,
                    manifest: presentation_manifest,
                },
                system_ui_policy,
            },
            NativeVnHostCacheBudget {
                asset_bytes: max_asset_cache_bytes,
                glyph_bytes: max_glyph_cache_bytes,
            },
        )
    }

    fn open(
        compiled: CompiledVnProject,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        binding: ProductPackageBinding,
        cache_budget: NativeVnHostCacheBudget,
    ) -> Result<Self, NativeVnHostError> {
        if compiled.story.story_manifest.stories.is_empty() {
            return Err(NativeVnHostError::EmptyStory);
        }
        let terminal_routes = compiled
            .story
            .route_graph
            .nodes
            .iter()
            .filter(|node| node.terminal)
            .map(|node| node.id.clone())
            .collect();
        let compiled_bytes = postcard::to_allocvec(&compiled.story)
            .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
        let compiled_section = RuntimeSectionPayload {
            section_id: "vn.story".to_string(),
            schema: "astra.vn.story".to_string(),
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
        let ui_profile = config.profile.clone();
        if compiled.themes.is_empty() {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_UI_THEME_MISSING: compiled project has no packaged UI theme".into(),
            ));
        }
        let ui_text_locale = Arc::new(RwLock::new(
            binding.presentation.localization.locale.clone(),
        ));
        let ordered_font_families = ordered_ui_font_families(
            &binding.presentation.font_families,
            &binding.presentation.localization.locale,
        );
        let ui_text_font_families = Arc::new(RwLock::new(ordered_font_families.clone()));
        let ui_text_measurer = Arc::new(NativeUiTextMeasurer {
            provider: Arc::clone(&binding.presentation.text_provider),
            font_families: Arc::clone(&ui_text_font_families),
            locale: Arc::clone(&ui_text_locale),
            frame_layouts: Mutex::new(BTreeMap::new()),
        });
        let ui_renderer = BlueprintYakuiRenderer::new(compiled.ui_blueprints.clone())?
            .with_image_resource_provider(binding.presentation.asset_store.clone())
            .with_text_measurer(ui_text_measurer.clone());
        let ui_backend = AstraYakuiBackend::new(ui_renderer, compiled.project_hash)?;
        let mut ui_controller_host = LuauUiControllerHost::with_default_budget()
            .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        let mut unique_controller_sources = compiled
            .controller_sources
            .values()
            .cloned()
            .collect::<BTreeSet<_>>();
        for source in std::mem::take(&mut unique_controller_sources) {
            ui_controller_host
                .register_source(source)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        }
        let registered_controller_ids = ui_controller_host
            .manifests()
            .map(|manifest| manifest.id.clone())
            .collect::<BTreeSet<_>>();
        if registered_controller_ids != compiled.controller_ids {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_UI_CONTROLLER_SET: packaged controller source set does not match bindings"
                    .into(),
            ));
        }
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
        let localization_keys = binding
            .presentation
            .localization
            .strings
            .keys()
            .cloned()
            .collect();
        let ui_view_localization_keys = blueprint_view_localization_keys(&compiled.ui_blueprints);
        let image_prefetch_windows =
            image_prefetch_windows(&compiled.story, &binding.presentation.asset_store)?;
        let image_prefetcher =
            PackageImagePrefetcher::start(Arc::clone(&binding.presentation.asset_store))?;
        Ok(Self {
            host,
            session_id: open.session_id,
            runtime_state: None,
            runtime_backlog_count: 0,
            font_families: ordered_font_families,
            available_font_families: binding.presentation.font_families,
            text_resources: TextRenderResourceOwner::with_retained_glyph_cache(
                8_192,
                cache_budget.glyph_bytes,
            )?,
            localization: binding.presentation.localization,
            localization_keys,
            localizations: binding.presentation.localizations,
            ui_text_locale,
            ui_text_font_families,
            ui_text_measurer,
            surface,
            command_sequence: 0,
            fixed_step: 0,
            session_seed: 0,
            next_step_mode: RuntimeStepMode::Live,
            width,
            height,
            ui_viewport: UiViewport {
                physical_width: width,
                physical_height: height,
                scale_factor: 1.0,
                font_scale: 1.0,
                safe_area_points: UiInsets {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                },
            },
            textures: BTreeMap::new(),
            asset_store: binding.presentation.asset_store,
            image_prefetcher,
            image_prefetch_windows,
            image_prefetch_inflight: BTreeSet::new(),
            image_prefetch_failure: None,
            live_texture_ids: BTreeSet::new(),
            live_texture_bytes: BTreeMap::new(),
            texture_last_used: BTreeMap::new(),
            texture_cpu_last_used: BTreeMap::new(),
            texture_use_clock: 0,
            texture_cpu_bytes: 0,
            texture_cpu_budget_bytes: cache_budget.asset_bytes,
            resident_texture_bytes: 0,
            gpu_texture_budget_bytes: cache_budget
                .asset_bytes
                .min(DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES),
            live_layout_ids: BTreeSet::new(),
            scene_draw: Vec::new(),
            last_step_evidence: None,
            terminal_routes,
            pending_timeline: Vec::new(),
            pending_audio: Vec::new(),
            pending_audio_preloads: Vec::new(),
            audio_preload_story_ids: BTreeSet::new(),
            pending_video: Vec::new(),
            pending_stage_completions: Vec::new(),
            next_media_resource_id: 10_000,
            stage_director,
            restored_product_media_snapshot: None,
            story: compiled.story,
            ui_blueprints: compiled.ui_blueprints,
            ui_view_localization_keys,
            ui_bindings: compiled.ui_bindings,
            ui_backend,
            ui_themes: compiled.themes,
            ui_profile,
            system_ui_policy: binding.system_ui_policy.clone(),
            ui_generation: 1,
            ui_input_sequence: 0,
            ui_draw: Vec::new(),
            ui_semantics: None,
            ui_save_slots: save_slots_for_policy(&binding.system_ui_policy),
            pending_ui_host_request: None,
            gameplay_thumbnail_capture: None,
            pending_save_metadata: None,
            pending_save_completion: None,
            exit_requested: false,
            ui_controller_host,
            ui_controller_sessions: BTreeMap::new(),
            base_ui_instance_id: None,
            base_ui_theme_id: None,
            active_ui_controller: None,
            ui_modals: Vec::new(),
            pending_ui_focus: None,
            ui_animations: BTreeMap::new(),
            ui_performance: UiPerformanceGate::new(UiPerformanceBudget::production()),
            last_ui_performance_sample: None,
            ui_frame_reuse: None,
            ui_host_performance_sampling_enabled: false,
            last_ui_host_performance_sample: None,
            shutdown_started: false,
        })
    }

    pub fn last_step_evidence(&self) -> Option<&NativeVnStepEvidence> {
        self.last_step_evidence.as_ref()
    }

    pub fn ui_performance_report(&self) -> UiPerformanceReport {
        self.ui_performance.report()
    }

    pub fn take_last_ui_performance_sample(&mut self) -> Option<UiPerformanceSample> {
        self.last_ui_performance_sample.take()
    }

    pub fn set_ui_host_performance_sampling_enabled(&mut self, enabled: bool) {
        self.ui_host_performance_sampling_enabled = enabled;
        if !enabled {
            self.last_ui_host_performance_sample = None;
        }
    }

    pub fn take_last_ui_host_performance_sample(
        &mut self,
    ) -> Option<NativeVnUiHostPerformanceSample> {
        self.last_ui_host_performance_sample.take()
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

    pub fn take_video_requests(&mut self) -> Vec<NativeVnVideoRequest> {
        std::mem::take(&mut self.pending_video)
    }

    pub fn take_stage_completions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_stage_completions)
    }

    pub fn tick_presentation(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<PlayerHostCommandBatch>, NativeVnHostError> {
        if !self.stage_director.requires_frame_tick() {
            return Ok(None);
        }
        let mut next = self.stage_director.clone();
        let outputs = next.tick(delta_ns).map_err(stage_director_error)?;
        let completions = outputs
            .into_iter()
            .map(|output| match output {
                StageDirectorOutput::FenceCompleted { id, .. } => Ok(id),
                _ => Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_STAGE_TICK_OUTPUT: frame tick emitted a non-completion output"
                        .into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let previous = std::mem::replace(&mut self.stage_director, next);
        match self.render(&[], 0) {
            Ok(batch) => {
                self.pending_stage_completions.extend(completions);
                Ok(Some(batch))
            }
            Err(error) => {
                self.stage_director = previous;
                Err(error)
            }
        }
    }

    pub fn take_ui_host_request(&mut self) -> Option<VnUiHostRequest> {
        self.pending_ui_host_request.take()
    }

    pub fn should_capture_gameplay_surface(&self, event: &UiInputEventKind) -> bool {
        self.runtime_state.as_ref().is_some_and(|state| {
            state.system_stack.is_empty()
                && matches!(
                    event,
                    UiInputEventKind::PointerButton {
                        button: astra_ui_core::UiPointerButton::Secondary,
                        state: UiButtonState::Pressed,
                        ..
                    }
                )
        })
    }

    pub fn prepare_surface_capture(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::CaptureSurface {
                sequence: self.next_command_sequence()?,
                surface: self.surface,
            },
        ])?)
    }

    pub fn has_gameplay_thumbnail_capture(&self) -> bool {
        self.gameplay_thumbnail_capture.is_some()
    }

    pub fn cache_gameplay_surface(
        &mut self,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    ) -> Result<(), NativeVnHostError> {
        const THUMBNAIL_WIDTH: u32 = 160;
        const THUMBNAIL_HEIGHT: u32 = 120;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_CAPTURE_DIMENSIONS: captured gameplay surface dimensions are invalid"
                    .into(),
            ));
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| {
                NativeVnHostError::Save(
                    "ASTRA_PLAYER_SAVE_CAPTURE_SIZE: captured gameplay surface size overflowed"
                        .into(),
                )
            })?;
        if rgba8.len() != expected {
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_SAVE_CAPTURE_BYTES: captured gameplay surface expected {expected} bytes but received {}",
                rgba8.len()
            )));
        }
        let source = image::RgbaImage::from_raw(width, height, rgba8).ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_CAPTURE_IMAGE: captured gameplay surface could not form an RGBA image"
                    .into(),
            )
        })?;
        let thumbnail = image::imageops::resize(
            &source,
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        );
        let rgba8 = thumbnail.into_raw();
        self.gameplay_thumbnail_capture = Some(
            TextureFrame::from_rgba8(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT, rgba8.into()).map_err(
                |error| {
                    NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_CAPTURE_TEXTURE: {error}"))
                },
            )?,
        );
        tracing::debug!(
            event = "player.vn.save.thumbnail_cached",
            width = THUMBNAIL_WIDTH,
            height = THUMBNAIL_HEIGHT,
            "cached a bounded gameplay thumbnail from the final composed surface"
        );
        Ok(())
    }

    pub fn prepare_save_metadata(
        &mut self,
        slot_id: &str,
        timestamp_text: String,
        playtime_ms: u64,
    ) -> Result<(), NativeVnHostError> {
        if !self.ui_save_slots.contains_key(slot_id) {
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            )));
        }
        if timestamp_text.trim().is_empty() || timestamp_text.len() > 64 {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_TIMESTAMP: save timestamp must be a bounded non-empty string"
                    .into(),
            ));
        }
        let thumbnail = self.gameplay_thumbnail_capture.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_THUMBNAIL_MISSING: no gameplay surface was captured before opening the save UI"
                    .into(),
            )
        })?;
        self.pending_save_metadata = Some(NativeVnSaveMetadata {
            slot_id: slot_id.to_string(),
            thumbnail_asset: format!("astra.internal.save_thumbnail.{slot_id}"),
            thumbnail,
            timestamp_text,
            playtime_text: format_playtime(playtime_ms),
        });
        if self.pending_save_completion.is_none()
            && self.system_ui_policy.quick_slot_id.as_deref() == Some(slot_id)
        {
            self.pending_save_completion = Some(SaveCompletionPolicy::Stay);
        }
        Ok(())
    }

    pub fn exit_requested(&self) -> bool {
        self.exit_requested
    }

    pub fn ui_semantics(&self) -> Option<&UiSemanticSnapshot> {
        self.ui_semantics.as_ref()
    }

    pub fn product_observation_evidence(
        &self,
    ) -> Result<NativeVnProductObservationEvidence, NativeVnHostError> {
        let state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_STATE_MISSING: product observation requires runtime state".into(),
            )
        })?;
        Ok(NativeVnProductObservationEvidence {
            schema: "astra.player_vn_product_observation.v2".into(),
            ui_profile: self.ui_profile.clone(),
            locale: state.locale.clone(),
            active_system_page: state.system_stack.last().map(|frame| frame.page),
            focused_semantic_id: self.ui_semantics.as_ref().and_then(|snapshot| {
                snapshot
                    .nodes
                    .iter()
                    .find(|node| node.focused)
                    .map(|node| node.id.clone())
            }),
            auto_enabled: state.system.auto_enabled,
            skip_mode: state.system.skip_mode,
            reading_mode: state.system.reading_mode,
            audio_enabled: state.system.audio_enabled,
            skip_allowed: state.system.skip_allowed,
            system_config: state.system.config.clone(),
            backlog_count: self.runtime_backlog_count,
            occupied_save_slot_count: self
                .ui_save_slots
                .values()
                .filter(|slot| slot.occupied)
                .count(),
        })
    }

    pub fn mark_save_committed(
        &mut self,
        slot_id: &str,
    ) -> Result<Option<PlayerHostCommandBatch>, NativeVnHostError> {
        let metadata = self.pending_save_metadata.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_MISSING: committed save has no prepared metadata"
                    .into(),
            )
        })?;
        if metadata.slot_id != slot_id {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: prepared metadata belongs to another slot".into(),
            ));
        }
        {
            let slot = self.ui_save_slots.get_mut(slot_id).ok_or_else(|| {
                NativeVnHostError::Save(format!(
                    "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
                ))
            })?;
            slot.occupied = true;
            slot.thumbnail_asset = Some(metadata.thumbnail_asset.clone());
            slot.has_thumbnail = true;
            slot.timestamp_text = Some(metadata.timestamp_text.clone());
            slot.playtime_text = Some(metadata.playtime_text.clone());
            slot.metadata_text = Some(format!(
                "{} | {}",
                metadata.timestamp_text, metadata.playtime_text
            ));
            slot.can_load = true;
            slot.migration_status = "current".to_string();
        }
        self.ui_backend
            .renderer_mut()
            .upsert_image_resource(metadata.thumbnail_asset.clone(), metadata.thumbnail.clone())?;
        self.textures
            .insert(metadata.thumbnail_asset, metadata.thumbnail);
        match self.pending_save_completion.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_COMPLETION_MISSING: committed save has no declared completion policy".into(),
            )
        })? {
            SaveCompletionPolicy::Stay => Ok(None),
            SaveCompletionPolicy::ReturnSystem => self
                .command(VnPlayerCommand::ReturnSystem)
                .map(Some),
        }
    }

    pub fn mark_save_failed(&mut self, slot_id: &str) -> Result<(), NativeVnHostError> {
        let metadata = self.pending_save_metadata.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_MISSING: failed save has no prepared metadata".into(),
            )
        })?;
        if metadata.slot_id != slot_id {
            self.pending_save_metadata = Some(metadata);
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: failed save belongs to another slot".into(),
            ));
        }
        self.pending_save_completion.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_COMPLETION_MISSING: failed save has no declared completion policy"
                    .into(),
            )
        })?;
        tracing::warn!(
            event = "vn.save.persistence_failed",
            slot_id,
            "save persistence failed; the current system page remains open"
        );
        Ok(())
    }

    pub fn mark_save_deleted(&mut self, slot_id: &str) -> Result<(), NativeVnHostError> {
        let slot = self.ui_save_slots.get_mut(slot_id).ok_or_else(|| {
            NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            ))
        })?;
        let thumbnail_asset = slot.thumbnail_asset.take();
        slot.occupied = false;
        slot.can_load = false;
        slot.has_thumbnail = false;
        slot.title_key = None;
        slot.timestamp_text = None;
        slot.playtime_text = None;
        slot.metadata_text = None;
        slot.migration_status = "empty".to_string();
        if let Some(thumbnail_asset) = thumbnail_asset {
            self.ui_backend
                .renderer_mut()
                .remove_image_resource(&thumbnail_asset);
            self.remove_texture(&thumbnail_asset)?;
        }
        Ok(())
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
        self.prepare_audio_asset_decode(
            &request.asset_id,
            &request.codec,
            Arc::clone(&request.encoded_bytes),
            request.encoded_hash,
        )
    }

    pub fn prepare_audio_preload_decode(
        &mut self,
        request: &NativeVnAudioPreloadRequest,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        self.prepare_audio_asset_decode(
            &request.asset_id,
            &request.codec,
            Arc::clone(&request.encoded_bytes),
            request.encoded_hash,
        )
    }

    fn prepare_audio_asset_decode(
        &mut self,
        asset_id: &str,
        codec: &str,
        encoded_bytes: Arc<[u8]>,
        encoded_hash: Hash256,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        if encoded_bytes.is_empty() || Hash256::from_sha256(&encoded_bytes) != encoded_hash {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_AUDIO_ENCODED_HASH: {}",
                asset_id
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
                codec: codec.to_string(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                bytes: encoded_bytes.as_ref().to_vec(),
            }])?,
            close: PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseDecode {
                sequence: self.next_command_sequence()?,
                session,
            }])?,
        })
    }

    pub fn prepare_video_decode(
        &mut self,
        request: &NativeVnVideoRequest,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        if request.encoded_bytes.is_empty()
            || Hash256::from_sha256(&request.encoded_bytes) != request.encoded_hash
        {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_VIDEO_ENCODED_HASH: {}",
                request.asset_id
            )));
        }
        let session = self.next_media_resource()?;
        Ok(PlayerDecodeLifecyclePlan {
            session,
            open: PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenDecode {
                sequence: self.next_command_sequence()?,
                session,
                kind: PlayerDecodeKind::Video,
            }])?,
            decode: PlayerHostCommandBatch::new(vec![PlayerHostCommand::Decode {
                sequence: self.next_command_sequence()?,
                request_sequence: 1,
                session,
                kind: PlayerDecodeKind::Video,
                codec: request.codec.clone(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                bytes: request.encoded_bytes.as_ref().to_vec(),
            }])?,
            close: PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseDecode {
                sequence: self.next_command_sequence()?,
                session,
            }])?,
        })
    }

    pub fn bind_decoded_video_frame(
        &mut self,
        request: &NativeVnVideoRequest,
        frame: TextureFrame,
        complete: bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.store_texture(
            request.asset_id.clone(),
            frame,
            &BTreeSet::from([request.asset_id.clone()]),
        )?;
        if complete {
            if let Some(fence) = &request.fence {
                self.pending_stage_completions.push(fence.clone());
            }
        }
        self.render(&[], 0)
    }

    pub(crate) fn complete_video_fence(&mut self, request: &NativeVnVideoRequest) {
        if let Some(fence) = &request.fence {
            self.pending_stage_completions.push(fence.clone());
        }
    }

    pub(crate) fn rehydrate_video_request(
        &self,
        snapshot: &crate::NativeVnVideoStreamSnapshot,
    ) -> Result<NativeVnVideoRequest, NativeVnHostError> {
        let asset = self.asset_store.load_media(&snapshot.asset_id)?;
        if asset.hash != snapshot.encoded_hash {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_VIDEO_RESTORE_HASH_MISMATCH: {}",
                snapshot.asset_id
            )));
        }
        Ok(NativeVnVideoRequest {
            layer: snapshot.layer.clone(),
            asset_id: snapshot.asset_id.clone(),
            codec: asset.codec.clone(),
            encoded_bytes: Arc::clone(&asset.bytes),
            encoded_hash: asset.hash,
            alpha_millionths: snapshot.alpha_millionths,
            looping: snapshot.looping,
            fence: snapshot.fence.clone(),
            fallback_asset_id: snapshot.fallback_asset_id.clone(),
            allow_fallback: snapshot.allow_fallback,
        })
    }

    pub fn bind_video_fallback(
        &mut self,
        request: &NativeVnVideoRequest,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let fallback = request.fallback_asset_id.as_deref().ok_or_else(|| {
            NativeVnHostError::Asset("ASTRA_PLAYER_VIDEO_FALLBACK_MISSING".into())
        })?;
        if !self.textures.contains_key(fallback) && !self.asset_store.contains_image(fallback) {
            return Err(missing_texture(fallback));
        }
        if let Some(fence) = &request.fence {
            self.pending_stage_completions.push(fence.clone());
        }
        self.render(&[], 0)
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
        audio: astra_player_core::PlayerMixedAudio,
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
                samples: audio.samples,
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
        self.save_with_product_media_snapshot(slot, None)
    }

    pub fn save_with_product_media_snapshot(
        &mut self,
        slot: impl Into<String>,
        product_media_snapshot_json: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, NativeVnHostError> {
        let slot = slot.into();
        if slot.trim().is_empty() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_INVALID: save slot must not be empty".into(),
            ));
        }
        if self.runtime_state.is_none() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_STATE_MISSING: runtime has not launched".into(),
            ));
        }
        let step_evidence = self.last_step_evidence.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_EVIDENCE_MISSING: runtime has no completed step".into(),
            )
        })?;
        let sections = self.host.save(RuntimeSaveRequest {
            session_id: self.session_id.clone(),
            slot: slot.clone(),
        })?;
        let runtime_state = saved_runtime_state(&sections)?;
        let retained_draw = std::iter::once(SceneCommand::rect(
            "vn.frame.clear",
            0,
            0,
            self.width,
            self.height,
            [8, 10, 16, 255],
        ))
        .chain(self.scene_draw.iter().cloned())
        .chain(self.ui_draw.iter().cloned())
        .filter(|command| {
            !matches!(
                command,
                SceneCommand::UploadTexture { .. }
                    | SceneCommand::UploadGlyph { .. }
                    | SceneCommand::ReleaseResource { .. }
            )
        })
        .collect::<Vec<_>>();
        let draw_commands_json = serde_json::to_vec(&retained_draw)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        let product_media_snapshot_hash = product_media_snapshot_json
            .as_ref()
            .map(|bytes| Hash256::from_sha256(bytes));
        let save_metadata = self.pending_save_metadata.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_MISSING: save serialization requires prepared metadata"
                    .into(),
            )
        })?;
        if save_metadata.slot_id != slot {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: prepared metadata belongs to another slot".into(),
            ));
        }
        let payload = NativeVnPlayerSavePayload {
            schema: "astra.player.native_vn_save_payload.v4".into(),
            slot,
            sections,
            runtime_state,
            stage_director: self.stage_director.clone(),
            step_evidence,
            draw_commands_hash: Hash256::from_sha256(&draw_commands_json),
            draw_commands_json,
            product_media_snapshot_json,
            product_media_snapshot_hash,
            save_metadata,
        };
        let payload_bytes = postcard::to_allocvec(&payload)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        postcard::to_allocvec(&NativeVnPlayerSaveEnvelope {
            schema: "astra.player.native_vn_save.v4".into(),
            payload_hash: Hash256::from_sha256(&payload_bytes),
            payload,
        })
        .map_err(|error| NativeVnHostError::Save(error.to_string()))
    }

    pub fn restore(&mut self, bytes: &[u8]) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let envelope = decode_save_envelope(bytes)?;
        if envelope.payload.sections.session_id != self.session_id {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SESSION_MISMATCH: save belongs to another runtime session"
                    .into(),
            ));
        }
        validate_save_metadata(&envelope.payload.save_metadata, &envelope.payload.slot)?;
        let restored_locale = envelope.payload.runtime_state.locale.clone();
        if !self.localizations.contains_key(&restored_locale) {
            return Err(NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_RESTORE_LOCALE_UNAVAILABLE: locale {restored_locale} is not packaged"
            )));
        }
        validate_saved_runtime_state(&envelope.payload.sections)?;
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
        self.fixed_step = report.restored_fixed_step;
        self.session_seed = report.session_seed;
        self.next_step_mode = RuntimeStepMode::RestoreContinuation;
        if Hash256::from_sha256(&envelope.payload.draw_commands_json)
            != envelope.payload.draw_commands_hash
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_INTEGRITY: presentation command hash mismatch".into(),
            ));
        }
        match (
            envelope.payload.product_media_snapshot_json.as_ref(),
            envelope.payload.product_media_snapshot_hash,
        ) {
            (Some(bytes), Some(expected)) if Hash256::from_sha256(bytes) == expected => {}
            (None, None) => {}
            _ => {
                return Err(NativeVnHostError::Save(
                    "ASTRA_PLAYER_SAVE_INTEGRITY: product media snapshot hash mismatch".into(),
                ));
            }
        }
        let _: Vec<SceneCommand> = serde_json::from_slice(&envelope.payload.draw_commands_json)
            .map_err(|error| {
                NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
            })?;
        let save_metadata = envelope.payload.save_metadata;
        self.runtime_state = Some(envelope.payload.runtime_state);
        self.runtime_backlog_count = self
            .runtime_state
            .as_ref()
            .map_or(0, |state| state.backlog.len());
        self.activate_locale(&restored_locale)?;
        self.stage_director = envelope.payload.stage_director;
        self.last_step_evidence = Some(envelope.payload.step_evidence);
        self.restored_product_media_snapshot = envelope.payload.product_media_snapshot_json;
        self.apply_save_metadata(save_metadata)?;
        self.ui_controller_sessions.clear();
        self.base_ui_instance_id = None;
        self.base_ui_theme_id = None;
        self.active_ui_controller = None;
        self.ui_modals.clear();
        self.pending_ui_focus = None;
        self.ui_semantics = None;
        self.ui_animations.clear();
        self.ui_text_measurer.begin_frame()?;
        let mut restore_lifecycle = Vec::new();
        for layout_id in self.live_layout_ids.iter().cloned().collect::<Vec<_>>() {
            restore_lifecycle.extend(self.text_resources.remove_layout(&layout_id)?);
        }
        self.live_layout_ids.clear();
        self.ui_generation = self
            .ui_generation
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        // The UI model has changed under the same session identity. The Yakui
        // backend emits an ordered release/resync transaction so the retained
        // Scene2D resources remain synchronized with this restored state.
        self.ui_backend
            .context_restored(&format!("vn.ui.{}", self.session_id.0), self.ui_generation)
            .map_err(NativeVnHostError::Ui)?;
        let mut batch = self.render(&[], 0)?;
        let [PlayerHostCommand::PresentScene { commands, .. }] = batch.commands.as_mut_slice()
        else {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_RESTORE_PRESENTATION_BATCH: restore render must emit exactly one scene"
                    .into(),
            ));
        };
        commands.splice(0..0, restore_lifecycle);
        Ok(batch)
    }

    pub fn prepare_save_transaction(
        &mut self,
        slot: impl Into<String>,
        transaction: PlayerHostResourceId,
    ) -> Result<PlayerSaveTransactionPlan, NativeVnHostError> {
        let slot = slot.into();
        self.prepare_save_transaction_with_product_media_snapshot(slot, transaction, None)
    }

    pub fn prepare_save_transaction_with_product_media_snapshot(
        &mut self,
        slot: impl Into<String>,
        transaction: PlayerHostResourceId,
        product_media_snapshot_json: Option<Vec<u8>>,
    ) -> Result<PlayerSaveTransactionPlan, NativeVnHostError> {
        let slot = slot.into();
        let bytes =
            self.save_with_product_media_snapshot(slot.clone(), product_media_snapshot_json)?;
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

    pub fn take_restored_product_media_snapshot(&mut self) -> Option<Vec<u8>> {
        self.restored_product_media_snapshot.take()
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

    pub fn list_saves(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::ListSaves {
                sequence: self.next_command_sequence()?,
            },
        ])?)
    }

    pub fn ingest_save_catalog_entry(
        &mut self,
        expected_slot: &str,
        bytes: &[u8],
    ) -> Result<(), NativeVnHostError> {
        let envelope = decode_save_envelope(bytes)?;
        if envelope.payload.slot != expected_slot
            || envelope.payload.sections.session_id != self.session_id
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_CATALOG_IDENTITY: catalog entry does not belong to the requested slot and session"
                    .into(),
            ));
        }
        validate_save_metadata(&envelope.payload.save_metadata, expected_slot)?;
        self.apply_save_metadata(envelope.payload.save_metadata)
    }

    fn apply_save_metadata(
        &mut self,
        metadata: NativeVnSaveMetadata,
    ) -> Result<(), NativeVnHostError> {
        {
            let slot = self
                .ui_save_slots
                .get_mut(&metadata.slot_id)
                .ok_or_else(|| {
                    NativeVnHostError::Save(
                        "ASTRA_PLAYER_SAVE_METADATA_SLOT: metadata references an undeclared slot"
                            .into(),
                    )
                })?;
            slot.occupied = true;
            slot.thumbnail_asset = Some(metadata.thumbnail_asset.clone());
            slot.has_thumbnail = true;
            slot.timestamp_text = Some(metadata.timestamp_text.clone());
            slot.playtime_text = Some(metadata.playtime_text.clone());
            slot.metadata_text = Some(format!(
                "{} | {}",
                metadata.timestamp_text, metadata.playtime_text
            ));
            slot.can_load = true;
            slot.migration_status = "current".into();
        }
        self.ui_backend
            .renderer_mut()
            .upsert_image_resource(metadata.thumbnail_asset.clone(), metadata.thumbnail.clone())?;
        self.textures
            .insert(metadata.thumbnail_asset, metadata.thumbnail);
        Ok(())
    }

    pub fn delete_save(
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
            PlayerHostCommand::DeleteSave {
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
        // Decode the bounded launch working set before the first RuntimeWorld
        // presentation. `launch_default` can synchronously enter a title or
        // route state, and doing this afterwards turns its first visible stage
        // into an input-critical package read/decrypt/decode operation.
        let prewarmed_images = self.prewarm_default_gameplay_story_images()?;
        let mut batch = self.step("launch_default", serde_json::json!({}))?;
        self.queue_default_gameplay_story_audio_preloads()?;
        if !prewarmed_images.is_empty() {
            let [PlayerHostCommand::PresentScene { commands, .. }] = batch.commands.as_mut_slice()
            else {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_IMAGE_GPU_PREWARM_PRESENTATION_BATCH".into(),
                ));
            };
            let mut uploads = Vec::with_capacity(prewarmed_images.len());
            let mut upload_bytes = 0u64;
            for asset_id in prewarmed_images {
                if self.live_texture_ids.insert(asset_id.clone()) {
                    let frame = self.texture(&asset_id)?.clone();
                    let frame_bytes = frame.rgba8.len() as u64;
                    self.resident_texture_bytes = self
                        .resident_texture_bytes
                        .checked_add(frame_bytes)
                        .ok_or_else(|| {
                            NativeVnHostError::Asset(
                                "ASTRA_PLAYER_GPU_RESIDENT_BYTES_OVERFLOW".into(),
                            )
                        })?;
                    if self.resident_texture_bytes > self.gpu_texture_budget_bytes {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_GPU_RESIDENT_BUDGET_EXCEEDED: {} > {}",
                            self.resident_texture_bytes, self.gpu_texture_budget_bytes
                        )));
                    }
                    self.live_texture_bytes
                        .insert(asset_id.clone(), frame_bytes);
                    self.mark_texture_used(&asset_id)?;
                    upload_bytes = upload_bytes.saturating_add(frame_bytes);
                    uploads.push(SceneCommand::UploadTexture {
                        resource_id: asset_id,
                        frame,
                    });
                }
            }
            let upload_count = uploads.len();
            commands.splice(0..0, uploads);
            tracing::info!(
                event = "player.image.gpu_prewarm.queued",
                upload_count,
                upload_bytes,
                "queued the bounded decoded image prefix before interactive presentation"
            );
        }
        Ok(batch)
    }

    pub fn take_audio_preload_requests(&mut self) -> Vec<NativeVnAudioPreloadRequest> {
        std::mem::take(&mut self.pending_audio_preloads)
    }

    fn queue_current_story_audio_preloads(&mut self) -> Result<(), NativeVnHostError> {
        let Some(story_id) = self
            .runtime_state
            .as_ref()
            .and_then(|state| state.cursor.as_ref())
            .map(|cursor| cursor.story_id.clone())
        else {
            return Ok(());
        };
        self.queue_story_audio_preloads(&story_id)
    }

    fn queue_default_gameplay_story_audio_preloads(&mut self) -> Result<(), NativeVnHostError> {
        let Some(story_id) = self.default_gameplay_story_id() else {
            return Ok(());
        };
        self.queue_story_audio_preloads(&story_id)
    }

    fn default_gameplay_story_id(&self) -> Option<String> {
        let system_story_ids = self
            .story
            .system_story_manifest
            .entries
            .values()
            .map(|entry| entry.story_id.as_str())
            .collect::<BTreeSet<_>>();
        self.story
            .story_manifest
            .stories
            .iter()
            .find(|story| !system_story_ids.contains(story.id.as_str()))
            .map(|story| story.id.clone())
    }

    fn prewarm_default_gameplay_story_images(&mut self) -> Result<Vec<String>, NativeVnHostError> {
        const MAX_ENTRY_IMAGE_PRELOADS: usize = 4_096;
        // The launch batch is outside the interactive frame budget.  Fill the
        // complete profile-bound resident window here so the first authored
        // stage cannot synchronously decrypt/decode a background after input.
        const MAX_ENTRY_GPU_PREWARM_BYTES: u64 = DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES;

        let mut seen = BTreeSet::new();
        let mut asset_ids = Vec::new();
        let system_story_ids = self
            .story
            .system_story_manifest
            .entries
            .values()
            .map(|entry| entry.story_id.as_str())
            .collect::<BTreeSet<_>>();
        // A title/system action can jump straight into a gameplay route.  Those
        // target states take precedence over manifest order, so the first
        // physical title input never discovers a cold route image on its
        // presentation-critical frame.
        let mut ordered_state_ids = Vec::new();
        if let Some(state_id) = default_runtime_launch_state(&self.story.stories) {
            ordered_state_ids.push(state_id);
        }
        ordered_state_ids.extend(system_action_gameplay_entry_states(
            &self.story.states,
            &self.story.system_story_manifest.actions,
            &system_story_ids,
        ));
        // A jump target itself can be a short title-transition state. Its
        // presentation successor is still part of the first physical action,
        // so rank that bounded look-ahead before the authored whole-story
        // traversal below. This is the same validated graph used by the
        // asynchronous steady-state prefetcher, but it runs before input.
        let entry_state_ids = ordered_state_ids.clone();
        if let Some(story_id) = self.default_gameplay_story_id() {
            let story = self
                .story
                .story_manifest
                .stories
                .iter()
                .find(|story| story.id == story_id)
                .ok_or_else(|| {
                    NativeVnHostError::Asset(
                        "ASTRA_PLAYER_IMAGE_PREWARM_STORY_MANIFEST_MISSING".into(),
                    )
                })?;
            ordered_state_ids.extend(
                story
                    .states
                    .iter()
                    .filter(|state_id| state_id.as_str() == "state.prologue")
                    .cloned(),
            );
            ordered_state_ids.extend(
                story
                    .states
                    .iter()
                    .filter(|state_id| state_id.as_str() != "state.prologue")
                    .cloned(),
            );
        }
        if ordered_state_ids.is_empty() {
            return Ok(Vec::new());
        }
        for state_id in &entry_state_ids {
            let entry_assets = self.image_prefetch_windows.get(state_id).ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_IMAGE_PREWARM_WINDOW_MISSING: {state_id}"
                ))
            })?;
            for asset_id in entry_assets {
                if seen.insert(asset_id.clone()) {
                    asset_ids.push(asset_id.clone());
                }
            }
        }
        for state_id in &ordered_state_ids {
            let state = self.story.states.get(state_id).ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_IMAGE_PREWARM_STATE_MISSING: {state_id}"
                ))
            })?;
            for command in state.scenes.iter().flat_map(|scene| scene.commands.iter()) {
                let CompiledCommand::Presentation {
                    command: PresentationCommand::Stage(stage),
                    ..
                } = command
                else {
                    continue;
                };
                let asset_id = match stage {
                    StageCommand::Preload { asset }
                    | StageCommand::Background { asset, .. }
                    | StageCommand::Show { asset, .. } => Some(asset),
                    StageCommand::Movie { fallback, .. } => fallback.as_ref(),
                    _ => None,
                };
                if let Some(asset_id) = asset_id.filter(|id| self.asset_store.contains_image(id)) {
                    if seen.insert(asset_id.clone()) {
                        asset_ids.push(asset_id.clone());
                    }
                }
            }
        }
        if asset_ids.len() > MAX_ENTRY_IMAGE_PRELOADS {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_IMAGE_PREWARM_COUNT_EXCEEDED: {} > {MAX_ENTRY_IMAGE_PRELOADS}",
                asset_ids.len()
            )));
        }
        let requested = asset_ids.len();
        let retained = self.asset_store.prewarm_image_prefix(&asset_ids)?;
        let retained_asset_ids = asset_ids.into_iter().take(retained).collect::<Vec<_>>();
        let retained_set = retained_asset_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut gpu_prewarms = Vec::new();
        let mut gpu_prewarm_bytes = 0u64;
        for asset_id in retained_asset_ids {
            let frame = self.asset_store.load_image(&asset_id)?;
            let frame_bytes = frame.rgba8.len() as u64;
            // CPU-ready frames and GPU uploads have independent budgets. Keep
            // every retained entry in the bounded decoded window so authored
            // transition frames never re-decrypt on their first use, but only
            // submit the smaller GPU resident prefix at launch.
            self.store_texture(asset_id.clone(), frame, &retained_set)?;
            if gpu_prewarm_bytes
                .checked_add(frame_bytes)
                .is_none_or(|total| total > MAX_ENTRY_GPU_PREWARM_BYTES)
            {
                // A large authored image must not terminate the whole bounded
                // window.  Keep scanning so later small entry assets can still
                // become resident; oversize frames remain CPU-ready and are
                // uploaded by the normal LRU when they are actually visible.
                continue;
            }
            gpu_prewarm_bytes += frame_bytes;
            gpu_prewarms.push(asset_id);
        }
        tracing::info!(
            event = "player.image.prewarm.completed",
            requested_count = requested,
            retained_count = retained,
            gpu_prewarm_count = gpu_prewarms.len(),
            gpu_prewarm_bytes,
            cache_bytes = self.asset_store.cache_bytes(),
            "prewarmed an authored-order image prefix within the decoded asset cache budget"
        );
        Ok(gpu_prewarms)
    }

    fn queue_story_audio_preloads(&mut self, story_id: &str) -> Result<(), NativeVnHostError> {
        const MAX_ENTRY_AUDIO_PRELOADS: usize = 4_096;

        if self.audio_preload_story_ids.contains(story_id) {
            return Ok(());
        }
        let mut asset_ids = BTreeSet::new();
        for state in self
            .story
            .states
            .values()
            .filter(|state| state.story_id == story_id)
        {
            for scene in &state.scenes {
                for command in &scene.commands {
                    let CompiledCommand::Presentation {
                        command: PresentationCommand::Stage(StageCommand::Audio(cue)),
                        ..
                    } = command
                    else {
                        continue;
                    };
                    if matches!(cue.bus, VnAudioBus::Bgm | VnAudioBus::Se) {
                        asset_ids.insert(cue.asset.clone());
                    }
                }
            }
        }
        if asset_ids.len() > MAX_ENTRY_AUDIO_PRELOADS {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_AUDIO_PRELOAD_COUNT_EXCEEDED: {} > {MAX_ENTRY_AUDIO_PRELOADS}",
                asset_ids.len()
            )));
        }
        let mut requests = Vec::with_capacity(asset_ids.len());
        for asset_id in asset_ids {
            if !self.asset_store.contains_audio(&asset_id) {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_AUDIO_PRELOAD_ASSET_KIND: {asset_id}"
                )));
            }
            let asset = self.asset_store.load_media(&asset_id)?;
            requests.push(NativeVnAudioPreloadRequest {
                asset_id,
                codec: asset.codec,
                encoded_bytes: asset.bytes,
                encoded_hash: asset.hash,
            });
        }
        let pending_hashes = self
            .pending_audio_preloads
            .iter()
            .map(|request| request.encoded_hash)
            .collect::<BTreeSet<_>>();
        self.pending_audio_preloads.extend(
            requests
                .into_iter()
                .filter(|request| !pending_hashes.contains(&request.encoded_hash)),
        );
        self.audio_preload_story_ids.insert(story_id.to_string());
        Ok(())
    }

    pub fn dispatch_ui_event(
        &mut self,
        kind: UiInputEventKind,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let event = self.next_ui_event(kind)?;
        self.dispatch_ui_events(vec![event])
    }

    fn next_ui_event(&mut self, kind: UiInputEventKind) -> Result<UiInputEvent, NativeVnHostError> {
        self.ui_input_sequence = self
            .ui_input_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(UiInputEvent {
            sequence: self.ui_input_sequence,
            kind,
        })
    }

    fn dispatch_ui_events(
        &mut self,
        events: Vec<UiInputEvent>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.last_ui_performance_sample = None;
        self.last_ui_host_performance_sample = self
            .ui_host_performance_sampling_enabled
            .then(NativeVnUiHostPerformanceSample::default);
        self.apply_ui_resize_events(&events)?;
        let fallback_events = events.clone();
        if !self.has_active_ui_surface() {
            let semantic_snapshot_hash = self
                .ui_semantics
                .as_ref()
                .map(|snapshot| snapshot.hash)
                .unwrap_or_else(|| astra_core::Hash256::from_sha256(b""));
            return if let Some(action) =
                self.bubbled_ui_action(&fallback_events, semantic_snapshot_hash)
            {
                self.dispatch_ui_action(&action)
            } else if let Some(command) = self.bubbled_ui_command(&fallback_events) {
                self.command(command)
            } else {
                self.present_current_scene(self.ui_draw.clone())
            };
        }
        if let Some(action) = self.retained_ui_activation(&events)? {
            let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
            let result = self.dispatch_ui_action(&action);
            let duration = performance_phase_duration(started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.action_dispatch_ns = sample.action_dispatch_ns.saturating_add(duration);
            }
            return result;
        }
        let frame = self.render_ui(events)?;
        if frame.actions.len() > 1 {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_ACTION_AMBIGUOUS: one input frame emitted multiple actions".into(),
            ));
        }
        if let Some(action) = frame.actions.first() {
            let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
            let result = self.dispatch_ui_action(action);
            let duration = performance_phase_duration(started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.action_dispatch_ns = sample.action_dispatch_ns.saturating_add(duration);
            }
            return result;
        }
        let consumed = frame
            .dispositions
            .iter()
            .any(|item| item.disposition == UiInputDispositionKind::Consumed);
        if !consumed {
            if let Some(action) = self.bubbled_ui_action(&fallback_events, frame.semantics.hash) {
                return self.dispatch_ui_action(&action);
            }
            if let Some(command) = self.bubbled_ui_command(&fallback_events) {
                return self.command(command);
            }
        }
        let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
        let result = self.present_current_scene(frame.draw);
        let duration = performance_phase_duration(started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.present_scene_ns = sample.present_scene_ns.saturating_add(duration);
        }
        result
    }

    fn retained_ui_activation(
        &mut self,
        events: &[UiInputEvent],
    ) -> Result<Option<astra_ui_core::UiActionEnvelope>, NativeVnHostError> {
        let [event] = events else {
            return Ok(None);
        };
        let explicit_target = match &event.kind {
            UiInputEventKind::AccessibilityAction {
                semantic_id,
                action,
                ..
            } if matches!(action.as_str(), "activate" | "invoke") => Some(semantic_id.as_str()),
            UiInputEventKind::Keyboard {
                logical_key,
                state: UiButtonState::Pressed,
                repeat: false,
                ..
            } if matches!(logical_key.as_str(), "Enter" | " " | "Space") => None,
            UiInputEventKind::Navigation {
                action: astra_ui_core::UiNavigationAction::Activate,
            } => None,
            UiInputEventKind::PointerButton {
                button: astra_ui_core::UiPointerButton::Primary,
                state: UiButtonState::Pressed,
                position,
            } if self.ui_frame_reuse.is_some() => {
                let semantics = self.ui_semantics.as_ref().ok_or_else(|| {
                    NativeVnHostError::Input(
                        "ASTRA_PLAYER_UI_RETAINED_SEMANTICS_MISSING: active UI surface has no semantic snapshot"
                            .into(),
                    )
                })?;
                let target = retained_pointer_activation_target(semantics, *position);
                let Some(target) = target else {
                    return Ok(None);
                };
                Some(target.id.as_str())
            }
            _ => return Ok(None),
        };
        let semantics = self.ui_semantics.as_ref().ok_or_else(|| {
            NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_RETAINED_SEMANTICS_MISSING: active UI surface has no semantic snapshot"
                    .into(),
            )
        })?;
        let target = if let Some(explicit_target) = explicit_target {
            semantics
                .nodes
                .iter()
                .find(|node| node.id == explicit_target)
        } else {
            semantics.nodes.iter().find(|node| node.focused)
        };
        let Some(target) = target else {
            return Ok(None);
        };
        if !target.enabled
            || target.hidden
            || !target
                .actions
                .contains(&astra_ui_core::UiSemanticAction::Activate)
        {
            return Ok(None);
        }
        self.ui_backend
            .renderer_mut()
            .resolve_retained_activation(semantics.hash, &target.id, event.sequence)
            .map(Some)
            .map_err(NativeVnHostError::Ui)
    }

    fn apply_ui_resize_events(&mut self, events: &[UiInputEvent]) -> Result<(), NativeVnHostError> {
        let Some(viewport) = events.iter().rev().find_map(|event| match &event.kind {
            UiInputEventKind::Resize { viewport } => Some(viewport),
            _ => None,
        }) else {
            return Ok(());
        };
        viewport.validate()?;
        let mut next_stage_director = self.stage_director.clone();
        next_stage_director
            .resize_viewport(astra_vn_package::StageViewport {
                width: viewport.physical_width,
                height: viewport.physical_height,
            })
            .map_err(stage_director_error)?;
        self.ensure_stage_textures(next_stage_director.state())?;
        let next_scene_draw = stage_scene_commands(
            next_stage_director.state(),
            &self.textures,
            viewport.physical_width,
            viewport.physical_height,
        )?;
        self.width = viewport.physical_width;
        self.height = viewport.physical_height;
        self.ui_viewport = viewport.clone();
        self.stage_director = next_stage_director;
        self.scene_draw = next_scene_draw;
        Ok(())
    }

    fn has_active_ui_surface(&self) -> bool {
        self.runtime_state.as_ref().is_some_and(|state| {
            state.pending_choice.is_some()
                || !state.system_stack.is_empty()
                || state.pending_wait.as_ref().map(|wait| wait.kind) == Some(VnWaitKind::Dialogue)
        })
    }

    fn bubbled_ui_command(&self, events: &[UiInputEvent]) -> Option<VnPlayerCommand> {
        let state = self.runtime_state.as_ref()?;
        for event in events.iter().rev() {
            match &event.kind {
                UiInputEventKind::Keyboard {
                    logical_key,
                    state: UiButtonState::Pressed,
                    ..
                } if state.pending_choice.is_none()
                    && state.system_stack.is_empty()
                    && matches!(logical_key.as_str(), "Enter" | " " | "Space") =>
                {
                    return Some(VnPlayerCommand::Advance);
                }
                UiInputEventKind::Navigation {
                    action: astra_ui_core::UiNavigationAction::Activate,
                } if state.pending_choice.is_none() && state.system_stack.is_empty() => {
                    return Some(VnPlayerCommand::Advance);
                }
                UiInputEventKind::PointerButton {
                    button: astra_ui_core::UiPointerButton::Primary,
                    state: UiButtonState::Pressed,
                    ..
                } if state.pending_choice.is_none() && state.system_stack.is_empty() => {
                    return Some(VnPlayerCommand::Advance);
                }
                UiInputEventKind::Navigation {
                    action: astra_ui_core::UiNavigationAction::Cancel,
                } if !state.system_stack.is_empty() => {
                    return Some(VnPlayerCommand::ReturnSystem);
                }
                UiInputEventKind::Keyboard {
                    logical_key,
                    state: UiButtonState::Pressed,
                    ..
                } if !state.system_stack.is_empty()
                    && matches!(logical_key.as_str(), "Escape" | "Esc") =>
                {
                    return Some(VnPlayerCommand::ReturnSystem);
                }
                _ => {}
            }
        }
        None
    }

    fn bubbled_ui_action(
        &self,
        events: &[UiInputEvent],
        semantic_snapshot_hash: astra_core::Hash256,
    ) -> Option<astra_ui_core::UiActionEnvelope> {
        let state = self.runtime_state.as_ref()?;
        if !state.system_stack.is_empty() {
            return None;
        }
        let event = events.iter().rev().find(|event| {
            matches!(
                event.kind,
                UiInputEventKind::PointerButton {
                    button: astra_ui_core::UiPointerButton::Secondary,
                    state: UiButtonState::Pressed,
                    ..
                }
            )
        })?;
        let page = "quick_panel";
        tracing::debug!(
            event = "vn.ui.secondary_shortcut",
            input_sequence = event.sequence,
            semantic_target_id = "root",
            page,
            profile = %self.ui_profile,
            "secondary pointer input resolved through the UI action router"
        );
        Some(astra_ui_core::UiActionEnvelope {
            schema: "astra.ui_action_envelope.v1".into(),
            input_sequence: event.sequence,
            semantic_target_id: "root".into(),
            action_id: "vn.open_system".into(),
            arguments: BTreeMap::from([("page".into(), UiValue::String(page.into()))]),
            semantic_snapshot_hash,
        })
    }

    fn dispatch_ui_action(
        &mut self,
        action: &astra_ui_core::UiActionEnvelope,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if action.action_id.starts_with("ui.") {
            let controller = self.active_ui_controller.clone().ok_or_else(|| {
                NativeVnHostError::Input(
                    "ASTRA_PLAYER_UI_CONTROLLER_CONTEXT: local action arrived without a live controller context"
                        .into(),
                )
            })?;
            let effect = match action.action_id.as_str() {
                "ui.open_modal" => VnUiControllerEffect::OpenModal {
                    view_id: ui_string_argument(action, "view_id")?.to_string(),
                    model: action
                        .arguments
                        .get("model")
                        .cloned()
                        .unwrap_or_else(|| UiValue::Map(BTreeMap::new())),
                },
                "ui.close_modal" => VnUiControllerEffect::CloseModal,
                "ui.set_state" => VnUiControllerEffect::SetSessionState {
                    key: ui_string_argument(action, "key")?.to_string(),
                    value: action.arguments.get("value").cloned().ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: value is missing".into(),
                        )
                    })?,
                },
                unsupported => {
                    return Err(NativeVnHostError::Input(format!(
                        "ASTRA_PLAYER_UI_LOCAL_ACTION_UNSUPPORTED: action {unsupported} is not registered"
                    )))
                }
            };
            self.ui_controller_sessions
                .entry(controller.controller_id.clone())
                .or_default()
                .apply(std::slice::from_ref(&effect))
                .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
            self.apply_ui_controller_effects(&controller.controller_id, vec![effect], false)?;
            return self.present_current_scene(self.ui_draw.clone());
        }
        let typed_action = match action.action_id.as_str() {
            "vn.advance" => VnUiAction::Advance,
            "vn.choose" => VnUiAction::Choose {
                option_id: ui_string_argument(action, "option_id")?.to_string(),
            },
            "vn.open_system" => VnUiAction::OpenSystem {
                page: SystemPageKind::parse(ui_string_argument(action, "page")?),
            },
            "vn.switch_system" => VnUiAction::SwitchSystemPage {
                page: SystemPageKind::parse(ui_string_argument(action, "page")?),
            },
            "vn.return_system" => VnUiAction::ReturnSystem,
            "vn.request_exit" => VnUiAction::RequestExit,
            "vn.set_config" => VnUiAction::SetConfig {
                key: ui_string_argument(action, "key")?.to_string(),
                value: action.arguments.get("value").cloned().ok_or_else(|| {
                    NativeVnHostError::Input(
                        "ASTRA_PLAYER_UI_ACTION_ARGUMENT: value is missing".into(),
                    )
                })?,
            },
            "vn.set_auto" => VnUiAction::SetAuto {
                enabled: ui_bool_argument(action, "enabled")?,
            },
            "vn.set_skip" => VnUiAction::SetSkip {
                mode: match ui_string_argument(action, "mode")? {
                    "none" => astra_vn_core::SkipMode::None,
                    "read" => astra_vn_core::SkipMode::Read,
                    "all" => astra_vn_core::SkipMode::All,
                    _ => {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: skip mode must be none, read or all"
                                .into(),
                        ))
                    }
                },
            },
            "vn.set_reading_mode" => VnUiAction::SetReadingMode {
                mode: match ui_string_argument(action, "mode")? {
                    "hidden" => ReadingMode::Hidden,
                    "manual" => ReadingMode::Manual,
                    "fast_forward" => ReadingMode::FastForward,
                    _ => {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_READING_MODE_UNKNOWN: reading mode must be hidden, manual or fast_forward".into(),
                        ))
                    }
                },
            },
            "vn.set_audio_enabled" => VnUiAction::SetAudioEnabled {
                enabled: ui_bool_argument(action, "enabled")?,
            },
            "vn.invoke_system_action" => VnUiAction::InvokeSystemAction {
                action_id: ui_string_argument(action, "action_id")?.to_string(),
            },
            "vn.replay_voice" => VnUiAction::ReplayVoice {
                voice_id: ui_string_argument(action, "voice_id")?.to_string(),
            },
            "vn.request_save" => VnUiAction::RequestSave {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.request_save_confirmed" => VnUiAction::RequestSaveConfirmed {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.request_load" => VnUiAction::RequestLoad {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.request_delete_save" => VnUiAction::RequestDeleteSave {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.start_replay" => VnUiAction::StartReplay {
                replay_id: ui_string_argument(action, "replay_id")?.to_string(),
            },
            "vn.preview_gallery" => VnUiAction::PreviewGallery {
                item_id: ui_string_argument(action, "item_id")?.to_string(),
            },
            "vn.request_route_jump" => VnUiAction::RequestRouteJump {
                node_id: ui_string_argument(action, "node_id")?.to_string(),
            },
            "vn.request_backlog_jump" => VnUiAction::RequestBacklogJump {
                command_id: ui_string_argument(action, "command_id")?.to_string(),
            },
            "vn.submit_text" => VnUiAction::SubmitText {
                input_id: ui_string_argument(action, "input_id")?.to_string(),
                value: ui_string_argument(action, "value")?.to_string(),
            },
            unsupported => {
                return Err(NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_ACTION_UNSUPPORTED: action {unsupported} is not routed by the product host"
                )))
            }
        };
        if self.active_ui_controller.is_none() {
            return match typed_action {
                VnUiAction::OpenSystem { page } => {
                    self.require_system_page(page)?;
                    tracing::debug!(
                        event = "vn.ui.global_action.forwarded",
                        action_id = "vn.open_system",
                        semantic_target_id = %action.semantic_target_id,
                        input_sequence = action.input_sequence,
                        "forwarded an explicitly allowed global UI action without a page controller"
                    );
                    self.command(VnPlayerCommand::OpenSystem { page })
                }
                _ => Err(NativeVnHostError::Input(
                    "ASTRA_PLAYER_UI_CONTROLLER_CONTEXT: only the global open-system action is allowed without a live controller context"
                        .into(),
                )),
            };
        }
        let controller = self.active_ui_controller.clone().ok_or_else(|| {
            NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_CONTROLLER_CONTEXT: action arrived without a live controller context"
                    .into(),
            )
        })?;
        let effects = self
            .ui_controller_host
            .invoke_action(
                &controller.controller_id,
                &controller.model_schema,
                &controller.model,
                &typed_action,
                self.ui_controller_sessions
                    .entry(controller.controller_id.clone())
                    .or_default(),
            )
            .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
        let forwarded =
            self.apply_ui_controller_effects(&controller.controller_id, effects, true)?;
        let Some(product_action) = forwarded else {
            return self.present_current_scene(self.ui_draw.clone());
        };
        tracing::trace!(
            event = "vn.ui.action.forwarded",
            action_id = %action.action_id,
            semantic_target_id = %action.semantic_target_id,
            stable_target_id = %product_action.stable_target(),
            controller_id = %controller.controller_id,
            input_sequence = action.input_sequence,
            "forwarded a typed UI action to the product command router"
        );
        let requested_locale = match &product_action {
            VnUiAction::SetConfig { key, value } if key == "display.language" => {
                Some(ui_value_to_scalar(value)?)
            }
            _ => None,
        };
        if matches!(product_action, VnUiAction::RequestExit) {
            self.exit_requested = true;
            tracing::info!(
                event = "player.vn.exit.requested",
                controller_id = %controller.controller_id,
                input_sequence = action.input_sequence,
                "accepted a typed UI request to close the Player session"
            );
            return self.present_current_scene(self.ui_draw.clone());
        }
        let command = match product_action {
            VnUiAction::Advance => VnPlayerCommand::Advance,
            VnUiAction::Choose { option_id } => VnPlayerCommand::Choose { option_id },
            VnUiAction::OpenSystem { page } => {
                self.require_system_page(page)?;
                VnPlayerCommand::OpenSystem { page }
            }
            VnUiAction::SwitchSystemPage { page } => {
                self.require_system_page(page)?;
                VnPlayerCommand::SwitchSystemPage { page }
            }
            VnUiAction::ReturnSystem => VnPlayerCommand::ReturnSystem,
            VnUiAction::RequestExit => {
                unreachable!("exit requests are handled before runtime routing")
            }
            VnUiAction::SetConfig { key, value } => VnPlayerCommand::SetConfig {
                key,
                value: ui_value_to_scalar(&value)?,
            },
            VnUiAction::SetAuto { enabled } => VnPlayerCommand::SetAuto { enabled },
            VnUiAction::SetSkip { mode } => VnPlayerCommand::SetSkip { mode },
            VnUiAction::SetReadingMode { mode } => {
                if !self.system_ui_policy.reading_modes.contains(&mode) {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_READING_MODE_UNDECLARED: UI requested a reading mode outside the bound profile"
                            .into(),
                    ));
                }
                VnPlayerCommand::SetReadingMode { mode }
            }
            VnUiAction::SetAudioEnabled { enabled } => {
                if !self.system_ui_policy.audio_toggle {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_AUDIO_TOGGLE_UNDECLARED: UI requested audio enable state outside the bound profile"
                            .into(),
                    ));
                }
                VnPlayerCommand::SetAudioEnabled { enabled }
            }
            VnUiAction::InvokeSystemAction { action_id } => {
                if !self.system_ui_policy.custom_action_ids.contains(&action_id) {
                    return Err(NativeVnHostError::Input(format!(
                        "ASTRA_PLAYER_SYSTEM_ACTION_UNDECLARED: action {action_id} is not declared by profile {}",
                        self.ui_profile
                    )));
                }
                VnPlayerCommand::InvokeSystemAction { action_id }
            }
            VnUiAction::ReplayVoice { voice_id } => {
                VnPlayerCommand::ReplayVoice { voice: voice_id }
            }
            VnUiAction::RequestSave { slot_id } => {
                return self.queue_ui_save_request(slot_id, true)
            }
            VnUiAction::RequestSaveConfirmed { slot_id } => {
                return self.queue_ui_save_request(slot_id, true)
            }
            VnUiAction::RequestLoad { slot_id } => {
                return self.queue_ui_save_request(slot_id, false)
            }
            VnUiAction::RequestDeleteSave { slot_id } => {
                return self.queue_ui_delete_request(slot_id)
            }
            VnUiAction::StartReplay { replay_id } => VnPlayerCommand::StartReplay { replay_id },
            VnUiAction::PreviewGallery { item_id } => VnPlayerCommand::PreviewGallery { item_id },
            VnUiAction::RequestRouteJump { node_id } => VnPlayerCommand::JumpRoute { node_id },
            VnUiAction::RequestBacklogJump { command_id } => {
                VnPlayerCommand::JumpBacklog { command_id }
            }
            VnUiAction::SubmitText { input_id, value } => {
                VnPlayerCommand::SubmitText { input_id, value }
            }
        };
        let previous_locale = if let Some(locale) = requested_locale.as_deref() {
            let previous = self.localization.locale.clone();
            self.activate_locale(locale)?;
            Some(previous)
        } else {
            None
        };
        match self.command(command) {
            Ok(batch) => {
                if let Some(locale) = requested_locale {
                    tracing::info!(
                        event = "player.vn.locale.changed",
                        locale,
                        "activated a packaged locale through the typed config action"
                    );
                }
                Ok(batch)
            }
            Err(error) => {
                if let Some(previous) = previous_locale {
                    self.activate_locale(&previous)?;
                }
                Err(error)
            }
        }
    }

    fn activate_locale(&mut self, locale: &str) -> Result<(), NativeVnHostError> {
        let localization = self.localizations.get(locale).cloned().ok_or_else(|| {
            NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_LOCALE_UNDECLARED: locale {locale} is not packaged"
            ))
        })?;
        *self.ui_text_locale.write().map_err(|_| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_LOCALE_LOCK: UI text locale state was poisoned".into(),
            )
        })? = locale.to_string();
        let font_families = ordered_ui_font_families(&self.available_font_families, locale);
        *self.ui_text_font_families.write().map_err(|_| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_FONT_FALLBACK_LOCK: UI font fallback state was poisoned".into(),
            )
        })? = font_families.clone();
        self.font_families = font_families;
        self.localization = localization;
        self.localization_keys = self.localization.strings.keys().cloned().collect();
        Ok(())
    }

    fn apply_ui_controller_effects(
        &mut self,
        origin_controller_id: &str,
        effects: Vec<VnUiControllerEffect>,
        allow_forward: bool,
    ) -> Result<Option<VnUiAction>, NativeVnHostError> {
        let mut queue = effects
            .into_iter()
            .map(|effect| (origin_controller_id.to_string(), effect))
            .collect::<VecDeque<_>>();
        let mut processed = 0usize;
        let mut forwarded = None;
        while let Some((controller_id, effect)) = queue.pop_front() {
            processed += 1;
            if processed > MAX_EFFECTS_PER_CALL {
                return Err(NativeVnHostError::Input(
                    "ASTRA_PLAYER_UI_CONTROLLER_EFFECT_LIMIT: recursive controller effects exceeded the per-call limit"
                        .into(),
                ));
            }
            match effect {
                VnUiControllerEffect::Forward { action } if allow_forward => {
                    if forwarded.replace(action).is_some() {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_CONTROLLER_FORWARD_DUPLICATE: an action callback may forward at most one product action"
                                .into(),
                        ));
                    }
                }
                VnUiControllerEffect::Forward { .. } => {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_UI_CONTROLLER_FORWARD_AUTHORITY: lifecycle callbacks cannot forward product actions"
                            .into(),
                    ));
                }
                VnUiControllerEffect::OpenModal { view_id, model } => {
                    if self.ui_modals.len() >= MAX_MODAL_DEPTH {
                        return Err(NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_DEPTH: modal stack exceeds {MAX_MODAL_DEPTH}"
                        )));
                    }
                    let view = self.ui_blueprints.views.get(&view_id).ok_or_else(|| {
                        NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_VIEW_MISSING: view {view_id} is not packaged"
                        ))
                    })?;
                    if self.base_ui_theme_id.as_deref() != Some(view.theme_id.as_str()) {
                        return Err(NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_THEME: modal view {view_id} must use the active profile theme"
                        )));
                    }
                    let matching = self
                        .ui_controller_host
                        .manifests()
                        .filter(|manifest| manifest.view == view_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    if matching.len() != 1 {
                        return Err(NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_CONTROLLER_BINDING: view {view_id} resolves to {} controllers instead of exactly one",
                            matching.len()
                        )));
                    }
                    let manifest = matching.into_iter().next().ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_MODAL_CONTROLLER_BINDING: modal controller disappeared"
                                .into(),
                        )
                    })?;
                    if manifest.model_schema != view.model_schema {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_MODAL_MODEL_SCHEMA: modal controller and view schemas differ"
                                .into(),
                        ));
                    }
                    let instance_id = format!(
                        "modal.{}.{}.{}",
                        self.ui_generation,
                        self.ui_modals.len(),
                        view_id
                    );
                    let modal = ActiveUiModal {
                        instance_id,
                        controller_id: manifest.id.clone(),
                        view_id,
                        model_schema: manifest.model_schema.clone(),
                        model,
                    };
                    self.ui_modals.push(modal.clone());
                    let open_effects = self
                        .ui_controller_host
                        .invoke_open(
                            &modal.controller_id,
                            &modal.model_schema,
                            &modal.model,
                            self.ui_controller_sessions
                                .entry(modal.controller_id.clone())
                                .or_default(),
                        )
                        .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
                    queue.extend(
                        open_effects
                            .into_iter()
                            .map(|effect| (modal.controller_id.clone(), effect)),
                    );
                }
                VnUiControllerEffect::CloseModal => {
                    if self.ui_modals.pop().is_none() {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_MODAL_UNDERFLOW: close_modal requires a live modal"
                                .into(),
                        ));
                    }
                    self.pending_ui_focus = None;
                    self.ui_animations.clear();
                }
                VnUiControllerEffect::Focus { semantic_id } => {
                    if self
                        .pending_ui_focus
                        .as_ref()
                        .is_some_and(|pending| pending != &semantic_id)
                    {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_FOCUS_CONFLICT: one effect transaction requested multiple focus targets"
                                .into(),
                        ));
                    }
                    self.pending_ui_focus = Some(semantic_id);
                }
                VnUiControllerEffect::SetSessionState { .. } => {}
                VnUiControllerEffect::Animation {
                    target_id,
                    preset_id,
                } => {
                    let controller = self
                        .ui_controller_host
                        .manifest(&controller_id)
                        .ok_or_else(|| {
                            NativeVnHostError::Input(
                                "ASTRA_PLAYER_UI_ANIMATION_CONTROLLER: origin controller is not registered"
                                    .into(),
                            )
                        })?;
                    let view = self
                        .ui_blueprints
                        .views
                        .get(&controller.view)
                        .ok_or_else(|| {
                            NativeVnHostError::Input(
                                "ASTRA_PLAYER_UI_ANIMATION_VIEW: origin view is not packaged"
                                    .into(),
                            )
                        })?;
                    let theme = self.ui_themes.get(&view.theme_id).ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ANIMATION_THEME: origin theme is not packaged".into(),
                        )
                    })?;
                    let motion = theme.tokens.get(&preset_id).ok_or_else(|| {
                        NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_ANIMATION_PRESET: preset {preset_id} is absent from theme {}",
                            theme.id
                        ))
                    })?;
                    let UiThemeValue::Motion { duration_ms, .. } = motion else {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ANIMATION_PRESET: animation preset must reference a motion token"
                                .into(),
                        ));
                    };
                    let fixed_time_ns = self.fixed_step.saturating_mul(16_666_667);
                    self.ui_animations.insert(
                        target_id.clone(),
                        ActiveUiAnimation {
                            target_id,
                            preset_id,
                            started_at_ns: fixed_time_ns,
                            duration_ns: u64::from(*duration_ms).saturating_mul(1_000_000),
                        },
                    );
                }
                VnUiControllerEffect::Trace { event, fields } => {
                    tracing::debug!(
                        target: "astra_player_vn::ui_controller",
                        event = "vn.ui.controller.trace",
                        controller_id = %controller_id,
                        controller_event = %event,
                        field_count = fields.len()
                    );
                }
            }
        }
        Ok(forwarded)
    }

    fn queue_ui_save_request(
        &mut self,
        slot_id: String,
        saving: bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = self.ui_save_slots.get(&slot_id).ok_or_else(|| {
            NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            ))
        })?;
        if saving && !slot.can_write {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_READ_ONLY: {slot_id}"
            )));
        }
        if !saving && (!slot.occupied || !slot.can_load) {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_EMPTY: {slot_id}"
            )));
        }
        if self.pending_ui_host_request.is_some() {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_HOST_REQUEST_CONFLICT: a host request is already pending".into(),
            ));
        }
        self.pending_ui_host_request = Some(if saving {
            self.pending_save_completion = Some(self.system_ui_policy.save_completion);
            VnUiHostRequest::Save {
                slot_id,
                completion: self.system_ui_policy.save_completion,
            }
        } else {
            VnUiHostRequest::Load { slot_id }
        });
        self.present_current_scene(self.ui_draw.clone())
    }

    fn require_system_page(&self, page: SystemPageKind) -> Result<(), NativeVnHostError> {
        if page == SystemPageKind::Unknown || !self.system_ui_policy.allowed_pages.contains(&page) {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SYSTEM_PAGE_UNDECLARED: page {page:?} is not allowed by profile {}",
                self.ui_profile
            )));
        }
        Ok(())
    }

    fn queue_ui_delete_request(
        &mut self,
        slot_id: String,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = self.ui_save_slots.get(&slot_id).ok_or_else(|| {
            NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            ))
        })?;
        if !slot.occupied {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_EMPTY: {slot_id}"
            )));
        }
        if self.pending_ui_host_request.is_some() {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_HOST_REQUEST_BUSY: another save operation is pending".into(),
            ));
        }
        self.pending_ui_host_request = Some(VnUiHostRequest::Delete { slot_id });
        self.present_current_scene(self.ui_draw.clone())
    }

    fn present_current_scene(
        &mut self,
        ui_draw: Vec<SceneCommand>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let mut commands = vec![SceneCommand::rect(
            "vn.frame.clear",
            0,
            0,
            self.width,
            self.height,
            [8, 10, 16, 255],
        )];
        commands.extend(self.scene_draw.iter().cloned());
        commands.extend(ui_draw.iter().cloned());
        // Retain only the UI layer. Keeping the fully composed frame here caused
        // every resize/focus repaint to recursively append the previous clear and
        // scene layers, producing duplicate resource identities on the WGPU path.
        self.ui_draw = ui_draw;
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentScene {
                sequence: self.next_command_sequence()?,
                surface: self.surface,
                width: self.width,
                height: self.height,
                clear_rgba: [8, 10, 16, 255],
                commands,
                semantics: self.ui_semantics.clone(),
            },
        ])?)
    }

    fn accumulate_ui_host_performance(&mut self, incoming: NativeVnUiHostPerformanceSample) {
        let Some(sample) = self.last_ui_host_performance_sample.as_mut() else {
            return;
        };
        sample.model_binding_ns = sample
            .model_binding_ns
            .saturating_add(incoming.model_binding_ns);
        sample.controller_ns = sample.controller_ns.saturating_add(incoming.controller_ns);
        sample.frame_model_ns = sample
            .frame_model_ns
            .saturating_add(incoming.frame_model_ns);
        sample.text_scene_ns = sample.text_scene_ns.saturating_add(incoming.text_scene_ns);
        sample.text_layout_ns = sample
            .text_layout_ns
            .saturating_add(incoming.text_layout_ns);
        sample.text_resource_ns = sample
            .text_resource_ns
            .saturating_add(incoming.text_resource_ns);
        sample.text_compose_ns = sample
            .text_compose_ns
            .saturating_add(incoming.text_compose_ns);
        sample.action_dispatch_ns = sample
            .action_dispatch_ns
            .saturating_add(incoming.action_dispatch_ns);
        sample.present_scene_ns = sample
            .present_scene_ns
            .saturating_add(incoming.present_scene_ns);
        sample.runtime_host_step_ns = sample
            .runtime_host_step_ns
            .saturating_add(incoming.runtime_host_step_ns);
        sample.runtime_output_decode_ns = sample
            .runtime_output_decode_ns
            .saturating_add(incoming.runtime_output_decode_ns);
        sample.runtime_render_ns = sample
            .runtime_render_ns
            .saturating_add(incoming.runtime_render_ns);
        sample.stage_prepare_ns = sample
            .stage_prepare_ns
            .saturating_add(incoming.stage_prepare_ns);
        sample.stage_scene_ns = sample
            .stage_scene_ns
            .saturating_add(incoming.stage_scene_ns);
        sample.stage_texture_ns = sample
            .stage_texture_ns
            .saturating_add(incoming.stage_texture_ns);
        sample.stage_command_ns = sample
            .stage_command_ns
            .saturating_add(incoming.stage_command_ns);
        sample.stage_lifecycle_ns = sample
            .stage_lifecycle_ns
            .saturating_add(incoming.stage_lifecycle_ns);
        sample.scene_compose_ns = sample
            .scene_compose_ns
            .saturating_add(incoming.scene_compose_ns);
    }

    fn render_ui(
        &mut self,
        events: Vec<UiInputEvent>,
    ) -> Result<NativeVnUiFrameResult, NativeVnHostError> {
        let model_binding_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let mut host_performance = NativeVnUiHostPerformanceSample::default();
        let state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::Input("ASTRA_PLAYER_STATE: runtime has not launched".into())
        })?;
        let save_slots = self.ui_save_slots.values().cloned().collect::<Vec<_>>();
        let context = VnUiModelContext {
            runtime: state,
            story: &self.story,
            save_slots: &save_slots,
            localization_keys: &self.localization_keys,
        };
        let (surface, system_page, model) = if state.pending_choice.is_some() {
            (
                "choice".to_string(),
                None,
                model_to_ui_value(&context.build_choice()?)?,
            )
        } else if let Some(frame) = state.system_stack.last() {
            let page_model = context.build_system_page(frame.page)?;
            (
                "system".to_string(),
                Some(system_page_binding_key(frame.page)?),
                page_model.to_ui_value()?,
            )
        } else {
            let message = context.build_message()?;
            let surface = message
                .window
                .clone()
                .unwrap_or_else(|| "message".to_string());
            (surface, None, model_to_ui_value(&message)?)
        };
        let binding = resolve_binding(
            &self.ui_bindings,
            VnUiBindingRequest {
                command_id: state
                    .pending_wait
                    .as_ref()
                    .map(|wait| wait.command_id.as_str()),
                system_page,
                surface: Some(&surface),
                profile: &self.ui_profile,
            },
        )?
        .clone();
        let (active_view_id, active_model_schema) = self
            .ui_blueprints
            .views
            .get(&binding.view_id)
            .map(|view| (view.id.clone(), view.model_schema.clone()))
            .ok_or_else(|| {
                NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_VIEW_MISSING: view {} is not packaged",
                    binding.view_id
                ))
            })?;
        let theme = self
            .ui_themes
            .get(&binding.theme_id)
            .cloned()
            .ok_or_else(|| {
                NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_THEME_MISSING: binding references unpackaged theme {}",
                    binding.theme_id
                ))
            })?;
        let controller_manifest = self
            .ui_controller_host
            .manifest(&binding.controller_id)
            .ok_or_else(|| {
                NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_CONTROLLER_MISSING: controller {} is not packaged",
                    binding.controller_id
                ))
            })?;
        if controller_manifest.view != binding.view_id
            || controller_manifest.model_schema != active_model_schema
        {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_CONTROLLER_BINDING: controller manifest does not match the selected view"
                .into(),
            ));
        }
        // A dialogue wait changes its model but does not create a new UI
        // surface. Keeping the controller instance scoped to the surface lets
        // its retained state, font resources, and layout caches survive a
        // normal `advance` action. Choice and system-page instances remain
        // distinct because their semantic action sets can change structurally.
        let instance_source = if state.pending_choice.is_some() {
            state
                .pending_wait
                .as_ref()
                .map(|wait| {
                    format!(
                        "{}.{}",
                        wait.command_id,
                        wait.await_id.as_deref().unwrap_or("unbound")
                    )
                })
                .ok_or_else(|| {
                    NativeVnHostError::Input("ASTRA_PLAYER_UI_CHOICE_INSTANCE_WAIT_MISSING".into())
                })?
        } else {
            system_page
                .map(str::to_owned)
                .unwrap_or_else(|| surface.to_owned())
        };
        let instance_id = format!(
            "{}.{}.{}.{}",
            self.ui_generation, binding.view_id, binding.controller_id, instance_source
        );
        let active_model = model.clone();
        let active_controller_id = binding.controller_id.clone();
        let active_changed = self
            .base_ui_instance_id
            .as_ref()
            .is_none_or(|active| active != &instance_id);
        if active_changed {
            self.ui_modals.clear();
            self.pending_ui_focus = None;
            self.ui_animations.clear();
            self.base_ui_instance_id = Some(instance_id.clone());
        }
        self.base_ui_theme_id = Some(binding.theme_id.clone());
        host_performance.model_binding_ns = performance_phase_duration(model_binding_started)?;
        let controller_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let lifecycle_effects = if active_changed {
            self.ui_controller_host.invoke_open(
                &active_controller_id,
                &active_model_schema,
                &active_model,
                self.ui_controller_sessions
                    .entry(active_controller_id.clone())
                    .or_default(),
            )
        } else {
            self.ui_controller_host.invoke_update(
                &active_controller_id,
                &active_model_schema,
                &active_model,
                &VnUiControllerUpdate {
                    fixed_time_ns: self.fixed_step.saturating_mul(16_666_667),
                    delta_ns: 16_666_667,
                    generation: self.ui_generation,
                },
                self.ui_controller_sessions
                    .entry(active_controller_id.clone())
                    .or_default(),
            )
        }
        .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
        self.apply_ui_controller_effects(&active_controller_id, lifecycle_effects, false)?;

        let modal_updates = self.ui_modals.clone();
        for modal in modal_updates {
            if !self
                .ui_modals
                .iter()
                .any(|live| live.instance_id == modal.instance_id)
            {
                continue;
            }
            let effects = self
                .ui_controller_host
                .invoke_update(
                    &modal.controller_id,
                    &modal.model_schema,
                    &modal.model,
                    &VnUiControllerUpdate {
                        fixed_time_ns: self.fixed_step.saturating_mul(16_666_667),
                        delta_ns: 16_666_667,
                        generation: self.ui_generation,
                    },
                    self.ui_controller_sessions
                        .entry(modal.controller_id.clone())
                        .or_default(),
                )
                .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
            self.apply_ui_controller_effects(&modal.controller_id, effects, false)?;
        }

        host_performance.controller_ns = performance_phase_duration(controller_started)?;
        let frame_model_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let fixed_time_ns = self.fixed_step.saturating_mul(16_666_667);
        let state_value = controller_state_value(
            self.ui_controller_sessions
                .entry(active_controller_id.clone())
                .or_default()
                .values(),
            self.ui_animations.values().map(|animation| {
                (
                    animation.target_id.clone(),
                    animation.progress_millionths(fixed_time_ns),
                )
            }),
        );
        let modal_frames = self
            .ui_modals
            .iter()
            .map(|modal| {
                let state = self
                    .ui_controller_sessions
                    .get(&modal.controller_id)
                    .map(VnUiSessionState::values)
                    .cloned()
                    .unwrap_or_default();
                UiBlueprintModalFrameModel {
                    view_id: modal.view_id.clone(),
                    model_schema: modal.model_schema.clone(),
                    model: modal.model.clone(),
                    state: controller_state_value(&state, std::iter::empty()),
                }
            })
            .collect::<Vec<_>>();
        let localization = frame_localization_subset(
            &self.localization.strings,
            &self.ui_view_localization_keys,
            &binding.view_id,
            &model,
            &state_value,
            &modal_frames,
        )?;
        let frame = UiBlueprintFrameModel {
            schema: "astra.ui_blueprint_frame_model.v1".to_string(),
            view_id: binding.view_id,
            model,
            state: state_value,
            modals: modal_frames,
            focus_request: self.pending_ui_focus.take(),
            localization,
        };
        let model_payload = postcard::to_allocvec(&frame)
            .map_err(|error| NativeVnHostError::Serialize(error.to_string()))?;
        let request = UiFrameRequest {
            schema: "astra.ui_frame_request.v1".to_string(),
            session_id: format!("vn.ui.{}", self.session_id.0),
            generation: self.ui_generation,
            viewport: self.ui_viewport.clone(),
            fixed_time_ns: self.fixed_step.saturating_mul(16_666_667),
            input: UiInputFrame {
                schema: "astra.ui_input_frame.v1".to_string(),
                events,
            },
            theme,
            model_schema: active_model_schema.clone(),
            model_payload,
        };
        let active = self.ui_modals.last().map_or(
            ActiveUiController {
                instance_id: instance_id.clone(),
                controller_id: active_controller_id.clone(),
                view_id: active_view_id.clone(),
                model_schema: active_model_schema.clone(),
                model: active_model.clone(),
            },
            |modal| ActiveUiController {
                instance_id: modal.instance_id.clone(),
                controller_id: modal.controller_id.clone(),
                view_id: modal.view_id.clone(),
                model_schema: modal.model_schema.clone(),
                model: modal.model.clone(),
            },
        );
        self.active_ui_controller = Some(active);
        let stable_frame = request.input.events.is_empty() && !active_changed;
        host_performance.frame_model_ns = performance_phase_duration(frame_model_started)?;
        if !active_changed {
            if let (Some(reuse), Some(semantics)) =
                (self.ui_frame_reuse.as_mut(), self.ui_semantics.as_ref())
            {
                if reuse.key.matches(&request, &instance_id) {
                    if let Some(pointer_position) = reusable_pointer_position(
                        reuse.pointer_position,
                        &request.input.events,
                        semantics,
                    ) {
                        reuse.pointer_position = pointer_position;
                        let mut performance = reuse.performance.clone();
                        performance.update_layout_ns = 0;
                        performance.paint_conversion_ns = 0;
                        performance.texture_update_bytes = 0;
                        self.ui_performance.record(performance.clone(), true)?;
                        self.last_ui_performance_sample = Some(performance);
                        tracing::trace!(
                            event = "player.vn.ui.frame.reused",
                            view_id = %active_view_id,
                            generation = self.ui_generation,
                            input_count = request.input.events.len(),
                            semantic_hash = %semantics.hash,
                            "reused an unchanged NativeVN UI frame"
                        );
                        let result = NativeVnUiFrameResult {
                            actions: Vec::new(),
                            dispositions: request
                                .input
                                .events
                                .iter()
                                .map(|event| UiInputDisposition {
                                    sequence: event.sequence,
                                    disposition: UiInputDispositionKind::Bubble,
                                    semantic_target_id: None,
                                })
                                .collect(),
                            semantics: semantics.clone(),
                            // `ui_draw` is the last complete submission and can contain
                            // one-shot resource lifecycle commands. Replaying those commands
                            // on a pointer-only frame would re-upload already live glyphs or
                            // textures, which the renderer correctly rejects. The retained
                            // resource owner remains authoritative until a new UI layout emits
                            // an explicit lifecycle delta.
                            draw: reusable_ui_draw_commands(&self.ui_draw),
                        };
                        self.accumulate_ui_host_performance(host_performance);
                        return Ok(result);
                    }
                }
            }
        }
        let reuse_key = NativeVnUiFrameReuseKey::from_request(&request, &instance_id);
        let pointer_position = pointer_position_after_events(
            self.ui_frame_reuse
                .as_ref()
                .and_then(|reuse| reuse.pointer_position),
            &request.input.events,
        );
        self.ui_text_measurer.begin_frame()?;
        let mut output = self.ui_backend.render_frame(request)?;
        let measured_text_layouts = self.ui_text_measurer.take_frame_layouts()?;
        let text_scene_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        if !output.diagnostics.is_empty() {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_UI_DIAGNOSTIC: UI frame returned {} diagnostics",
                output.diagnostics.len()
            )));
        }
        self.ui_performance
            .record(output.performance.clone(), stable_frame)?;
        self.last_ui_performance_sample = Some(output.performance.clone());
        let root_bounds = output
            .semantics
            .nodes
            .iter()
            .find(|node| node.id == "root")
            .map(|node| node.bounds_points);
        tracing::debug!(
            event = "player.vn.ui.frame_layout",
            view_id = %active_view_id,
            generation = self.ui_generation,
            active_changed,
            semantic_count = output.semantics.nodes.len(),
            primitive_count = output.render.primitives.len(),
            texture_upload_count = output.render.textures.uploads.len(),
            root_min_x = root_bounds.map_or(-1.0, |bounds| bounds.min.x),
            root_min_y = root_bounds.map_or(-1.0, |bounds| bounds.min.y),
            root_max_x = root_bounds.map_or(-1.0, |bounds| bounds.max.x),
            root_max_y = root_bounds.map_or(-1.0, |bounds| bounds.max.y),
            "rendered a traceable AstraVN UI semantic generation"
        );
        let draw = ui_frame_to_scene_commands(&output.render)?;
        let mut next_layout_ids = BTreeSet::new();
        let mut pending_text = Vec::new();
        let body_font_size = (self.height as f32 / 30.0).clamp(18.0, 34.0);
        let text_layout_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        append_ui_semantic_text(
            &self.localization,
            &measured_text_layouts,
            &mut next_layout_ids,
            &mut pending_text,
            &output.semantics,
            body_font_size,
        )?;
        host_performance.text_layout_ns = performance_phase_duration(text_layout_started)?;
        let text_resource_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let removals = self
            .live_layout_ids
            .difference(&next_layout_ids)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let updates = pending_text
            .iter()
            .map(|pending| TextRenderLayoutUpdate {
                layout_id: &pending.layout_id,
                layout: pending.layout.as_ref(),
                shared_layout: Some(&pending.layout),
                rgba: pending.rgba,
                translation: (pending.aligned_x, pending.aligned_y),
            })
            .collect::<Vec<_>>();
        let text_frame = self.text_resources.update_frame(&updates, &removals)?;
        host_performance.text_resource_ns = performance_phase_duration(text_resource_started)?;
        let text_compose_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let mut text_draw = Vec::new();
        for (pending, draw) in pending_text.iter().zip(text_frame.layouts) {
            if pending.layout_id != draw.layout_id {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_LAYOUT_ORDER: text resource owner changed frame draw order"
                        .into(),
                ));
            }
            text_draw.extend(draw.commands);
        }
        let mut composed_draw = text_frame.lifecycle;
        composed_draw.extend(draw);
        composed_draw.extend(text_draw);
        self.live_layout_ids = next_layout_ids;
        host_performance.text_compose_ns = performance_phase_duration(text_compose_started)?;
        if self
            .ui_animations
            .values()
            .any(|animation| animation.progress_millionths(fixed_time_ns) < 1_000_000)
        {
            output.repaint_after_ns = Some(
                output
                    .repaint_after_ns
                    .unwrap_or(16_666_667)
                    .min(16_666_667),
            );
        }
        self.ui_semantics = Some(output.semantics.clone());
        self.ui_frame_reuse = if output.repaint_after_ns.is_none() && output.actions.is_empty() {
            Some(NativeVnUiFrameReuse {
                key: reuse_key,
                pointer_position,
                performance: output.performance.clone(),
            })
        } else {
            None
        };
        host_performance.text_scene_ns = performance_phase_duration(text_scene_started)?;
        self.accumulate_ui_host_performance(host_performance);
        Ok(NativeVnUiFrameResult {
            actions: output.actions,
            dispositions: output.dispositions,
            semantics: output.semantics,
            draw: composed_draw,
        })
    }

    pub fn release_resources(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_REPEATED: resource shutdown already started".to_string(),
            ));
        }
        for (asset_id, result) in self.image_prefetcher.shutdown()? {
            self.image_prefetch_inflight.remove(&asset_id);
            if let Err(error) = result {
                self.image_prefetch_failure = Some(format!(
                    "ASTRA_PLAYER_IMAGE_PREFETCH_FAILED: asset_hash={}, cause={error}",
                    Hash256::from_sha256(asset_id.as_bytes())
                ));
            }
        }
        if !self.image_prefetch_inflight.is_empty() {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_IMAGE_PREFETCH_INFLIGHT_AT_SHUTDOWN".into(),
            ));
        }
        self.ui_text_measurer.begin_frame()?;
        self.ui_frame_reuse = None;
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
        self.live_texture_bytes.clear();
        self.texture_last_used.clear();
        self.texture_cpu_last_used.clear();
        self.textures.clear();
        self.texture_cpu_bytes = 0;
        self.resident_texture_bytes = 0;
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
                semantics: None,
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
        if let Some(error) = self.image_prefetch_failure {
            return Err(NativeVnHostError::Asset(error));
        }
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
        self.poll_image_prefetch()?;
        let fixed_step = self
            .fixed_step
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        let runtime_step_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let output = self.host.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step,
            delta_ns: 16_666_667,
            session_seed: self.session_seed,
            mode: self.next_step_mode,
            action: action.to_string(),
            payload,
        })?;
        let runtime_host_step_ns = performance_phase_duration(runtime_step_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.runtime_host_step_ns = sample
                .runtime_host_step_ns
                .saturating_add(runtime_host_step_ns);
        }
        let output_decode_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        self.fixed_step = fixed_step;
        self.next_step_mode = RuntimeStepMode::Live;
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
        let runtime_view = output
            .outputs
            .iter()
            .find(|envelope| {
                envelope.domain == RuntimeOutputDomain::Trace
                    && envelope.schema == VN_RUNTIME_VIEW_STATE_SCHEMA
            })
            .ok_or_else(|| {
                NativeVnHostError::RuntimeEvidence(
                    "ASTRA_PLAYER_VN_VIEW_STATE_MISSING: runtime view state trace is required"
                        .into(),
                )
            })?
            .decode_postcard::<VnRuntimeViewState>(
                RuntimeOutputDomain::Trace,
                VN_RUNTIME_VIEW_STATE_SCHEMA,
                SchemaVersion::new(VN_RUNTIME_VIEW_STATE_SCHEMA_MAJOR, 0, 0),
            )
            .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
        if runtime_view.schema != VN_RUNTIME_VIEW_STATE_SCHEMA {
            return Err(NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_VIEW_STATE_SCHEMA: runtime view state schema is invalid".into(),
            ));
        }
        self.runtime_backlog_count = runtime_view.backlog_count;
        self.runtime_state = Some(runtime_view.state);
        self.schedule_current_image_prefetch()?;
        self.queue_current_story_audio_preloads()?;
        let runtime_state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_STATE_MISSING: runtime state trace is required".into(),
            )
        })?;
        self.last_step_evidence = Some(NativeVnStepEvidence {
            schema: "astra.player_vn_step_evidence.v2".to_string(),
            fixed_step,
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
            pending_wait_command_id: runtime_state
                .pending_wait
                .as_ref()
                .map(|wait| wait.command_id.clone()),
            pending_wait_await_id: runtime_state
                .pending_wait
                .as_ref()
                .and_then(|wait| wait.await_id.clone()),
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
        let mut ordered_outputs = Vec::new();
        let mut presentation_count = 0_usize;
        for envelope in &output.outputs {
            match envelope.domain {
                RuntimeOutputDomain::Audio if envelope.schema == "astra.vn.audio_command.v2" => {
                    let command = envelope
                        .decode_postcard::<astra_vn_core::VnAudioCommand>(
                            RuntimeOutputDomain::Audio,
                            "astra.vn.audio_command.v2",
                            SchemaVersion::new(2, 0, 0),
                        )
                        .map_err(|error| NativeVnHostError::RuntimeEvidence(error.to_string()))?;
                    let asset_id = command.cue.asset.clone();
                    let asset = self.asset_store.load_media(&asset_id)?;
                    let command_kind = match command.cue.bus {
                        VnAudioBus::Voice => "voice",
                        VnAudioBus::Bgm => "bgm",
                        VnAudioBus::Se => "se",
                        VnAudioBus::Movie => "movie",
                    };
                    let mut attributes = BTreeMap::from([
                        ("asset".to_string(), asset_id.clone()),
                        ("loop".to_string(), command.cue.looped.to_string()),
                        ("fade".to_string(), command.cue.fade_ms.to_string()),
                    ]);
                    match &command.cue.sync {
                        VnAudioSync::None => {}
                        VnAudioSync::Text => {
                            attributes.insert("sync".to_string(), "text".to_string());
                        }
                        VnAudioSync::Fence(fence) => {
                            attributes.insert("sync".to_string(), "fence".to_string());
                            attributes.insert("fence".to_string(), fence.clone());
                        }
                    }
                    ordered_outputs.push(NativeVnOrderedRuntimeOutput::AudioStart(
                        NativeVnAudioRequest {
                            command_id: command.command_id,
                            command: command_kind.to_string(),
                            attributes,
                            asset_id,
                            codec: asset.codec.clone(),
                            encoded_bytes: Arc::clone(&asset.bytes),
                            encoded_hash: asset.hash,
                        },
                    ));
                }
                RuntimeOutputDomain::Presentation => {
                    let command = envelope
                        .decode_postcard(
                            RuntimeOutputDomain::Presentation,
                            "astra.vn.presentation_command.v2",
                            SchemaVersion::new(2, 0, 0),
                        )
                        .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
                    presentation_count += 1;
                    ordered_outputs.push(NativeVnOrderedRuntimeOutput::Presentation(command));
                }
                _ => {}
            }
        }
        tracing::trace!(
            event = "player.vn.runtime.step_applied",
            fixed_step,
            presentation_count,
            pending_wait_command_id = runtime_state
                .pending_wait
                .as_ref()
                .map(|wait| wait.command_id.as_str())
                .unwrap_or("none"),
            pending_choice_count = runtime_state
                .pending_choice
                .as_ref()
                .map_or(0, |choice| choice.options.len()),
            pending_choice_enabled_count = runtime_state
                .pending_choice
                .as_ref()
                .map_or(0, |choice| choice.enabled_option_ids.len()),
            "applied runtime output before presentation rendering"
        );
        let runtime_output_decode_ns = performance_phase_duration(output_decode_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.runtime_output_decode_ns = sample
                .runtime_output_decode_ns
                .saturating_add(runtime_output_decode_ns);
        }
        let render_started = performance_phase_started(self.ui_host_performance_sampling_enabled);
        let result = self.render(&ordered_outputs, presentation_count);
        let runtime_render_ns = performance_phase_duration(render_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.runtime_render_ns = sample.runtime_render_ns.saturating_add(runtime_render_ns);
        }
        result
    }

    fn render(
        &mut self,
        ordered_outputs: &[NativeVnOrderedRuntimeOutput],
        presentation_count: usize,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let stage_prepare_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let stage_commands = ordered_outputs
            .iter()
            .filter_map(|output| match output {
                NativeVnOrderedRuntimeOutput::Presentation(PresentationCommand::Stage(stage)) => {
                    Some(stage)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let (next_stage_director, stage_outputs) = if stage_commands.is_empty() {
            (None, Vec::new())
        } else {
            let (director, outputs) = self
                .stage_director
                .prepare_batch(stage_commands)
                .map_err(stage_director_error)?;
            (Some(director), outputs)
        };
        let mut stage_outputs = stage_outputs.into_iter();
        let mut next_audio = Vec::new();
        for output in ordered_outputs {
            if let NativeVnOrderedRuntimeOutput::AudioStart(request) = output {
                next_audio.push(NativeVnAudioOutput::Start(request.clone()));
                continue;
            }
            let NativeVnOrderedRuntimeOutput::Presentation(command) = output else {
                continue;
            };
            if let PresentationCommand::Stage(stage) = command {
                let outputs = stage_outputs.next().ok_or_else(|| {
                    NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_STAGE_BATCH_OUTPUT_MISSING: {}",
                        stage.kind()
                    ))
                })?;
                for output in outputs {
                    match output {
                        StageDirectorOutput::Preload { asset } => {
                            if !self.textures.contains_key(&asset)
                                && !self.asset_store.contains_media(&asset)
                                && !self.asset_store.contains_image(&asset)
                            {
                                return Err(NativeVnHostError::Asset(format!(
                                    "ASTRA_PLAYER_PRELOAD_ASSET_MISSING: {asset}"
                                )));
                            }
                        }
                        StageDirectorOutput::Audio(_) => {}
                        StageDirectorOutput::AudioControl(control) => {
                            next_audio.push(NativeVnAudioOutput::Control(
                                NativeVnAudioControlRequest {
                                    command_id: control.id,
                                    action: match &control.action {
                                        VnAudioControlAction::Pause => "pause",
                                        VnAudioControlAction::Resume => "resume",
                                        VnAudioControlAction::Stop => "stop",
                                        VnAudioControlAction::FadeStop { .. } => "fade_stop",
                                    }
                                    .to_string(),
                                    target: control.target,
                                    duration_ms: match &control.action {
                                        VnAudioControlAction::FadeStop { duration_ms, .. } => {
                                            Some(*duration_ms)
                                        }
                                        _ => None,
                                    },
                                    fence: match control.action {
                                        VnAudioControlAction::FadeStop { fence, .. } => Some(fence),
                                        _ => None,
                                    },
                                },
                            ));
                        }
                        StageDirectorOutput::AudioBusEnabled { bus, enabled } => {
                            let target = match bus {
                                VnAudioBus::Bgm => "bgm",
                                VnAudioBus::Se => "se",
                                VnAudioBus::Voice => "voice",
                                VnAudioBus::Movie => "movie_audio",
                            };
                            next_audio.push(NativeVnAudioOutput::Control(
                                NativeVnAudioControlRequest {
                                    command_id: format!("audio.bus.{target}.enabled"),
                                    action: if enabled { "enable_bus" } else { "disable_bus" }
                                        .to_string(),
                                    target: target.to_string(),
                                    duration_ms: None,
                                    fence: None,
                                },
                            ));
                        }
                        StageDirectorOutput::Movie(movie) => {
                            let asset = self.asset_store.load_media(&movie.asset)?;
                            self.pending_video.push(NativeVnVideoRequest {
                                layer: movie.layer,
                                asset_id: movie.asset,
                                codec: asset.codec.clone(),
                                encoded_bytes: Arc::clone(&asset.bytes),
                                encoded_hash: asset.hash,
                                alpha_millionths: movie.alpha.millionths,
                                looping: matches!(movie.loop_mode, MovieLoopMode::Loop),
                                fence: movie.fence,
                                fallback_asset_id: movie.fallback,
                                allow_fallback: next_stage_director
                                    .as_ref()
                                    .ok_or_else(|| {
                                        NativeVnHostError::Asset(
                                            "ASTRA_PLAYER_STAGE_BATCH_DIRECTOR_MISSING: movie output requires a prepared stage transaction"
                                                .into(),
                                        )
                                    })?
                                    .state()
                                    .profile
                                    != "advanced-vn",
                            });
                        }
                        StageDirectorOutput::Effect(_) => {}
                        StageDirectorOutput::FenceCompleted { id, .. } => {
                            self.pending_stage_completions.push(id);
                        }
                    }
                }
            }
        }
        if stage_outputs.next().is_some() {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_STAGE_BATCH_OUTPUT_EXCESS: stage output count exceeded command count"
                    .into(),
            ));
        }
        let stage_prepare_ns = performance_phase_duration(stage_prepare_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.stage_prepare_ns = sample.stage_prepare_ns.saturating_add(stage_prepare_ns);
        }
        let stage_scene_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let mut lifecycle = Vec::new();
        let next_stage_scene = if let Some(director) = next_stage_director.as_ref() {
            let stage_texture_started =
                performance_phase_started(self.ui_host_performance_sampling_enabled);
            self.ensure_stage_textures(director.state())?;
            let stage_texture_ns = performance_phase_duration(stage_texture_started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.stage_texture_ns = sample.stage_texture_ns.saturating_add(stage_texture_ns);
            }
            let stage_command_started =
                performance_phase_started(self.ui_host_performance_sampling_enabled);
            let scene_draw =
                stage_scene_commands(director.state(), &self.textures, self.width, self.height)?;
            let stage_command_ns = performance_phase_duration(stage_command_started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.stage_command_ns = sample.stage_command_ns.saturating_add(stage_command_ns);
            }

            let stage_lifecycle_started =
                performance_phase_started(self.ui_host_performance_sampling_enabled);
            let texture_ids = scene_texture_ids(&scene_draw);
            for asset_id in &texture_ids {
                self.mark_texture_used(asset_id)?;
            }
            let missing_ids = texture_ids
                .difference(&self.live_texture_ids)
                .cloned()
                .collect::<Vec<_>>();
            let missing_bytes = missing_ids.iter().try_fold(0u64, |total, asset_id| {
                total
                    .checked_add(self.texture(asset_id)?.rgba8.len() as u64)
                    .ok_or_else(|| {
                        NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_BYTES_OVERFLOW".into())
                    })
            })?;
            self.evict_gpu_textures_for(missing_bytes, &texture_ids, &mut lifecycle)?;
            for asset_id in missing_ids {
                let frame = self.texture(&asset_id)?.clone();
                self.resident_texture_bytes = self
                    .resident_texture_bytes
                    .checked_add(frame.rgba8.len() as u64)
                    .ok_or_else(|| {
                        NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_BYTES_OVERFLOW".into())
                    })?;
                self.live_texture_ids.insert(asset_id.clone());
                self.live_texture_bytes
                    .insert(asset_id.clone(), frame.rgba8.len() as u64);
                lifecycle.push(SceneCommand::UploadTexture {
                    resource_id: asset_id,
                    frame,
                });
            }
            let stage_lifecycle_ns = performance_phase_duration(stage_lifecycle_started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.stage_lifecycle_ns =
                    sample.stage_lifecycle_ns.saturating_add(stage_lifecycle_ns);
            }
            Some((scene_draw, texture_ids))
        } else {
            None
        };
        let stage_scene_ns = performance_phase_duration(stage_scene_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.stage_scene_ns = sample.stage_scene_ns.saturating_add(stage_scene_ns);
        }

        for command in ordered_outputs.iter().filter_map(|output| match output {
            NativeVnOrderedRuntimeOutput::Presentation(command) => Some(command),
            NativeVnOrderedRuntimeOutput::AudioStart(_) => None,
        }) {
            match command {
                PresentationCommand::Dialogue { .. }
                | PresentationCommand::Choice { .. }
                | PresentationCommand::SystemPage { .. }
                | PresentationCommand::SystemOption { .. } => {}
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
        let ui_frame = if self.runtime_state.as_ref().is_some_and(|state| {
            state.pending_choice.is_some()
                || !state.system_stack.is_empty()
                || state.pending_wait.as_ref().map(|wait| wait.kind) == Some(VnWaitKind::Dialogue)
        }) {
            Some(self.render_ui(Vec::new())?)
        } else {
            None
        };
        let scene_compose_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        if ui_frame.is_none() && !self.live_layout_ids.is_empty() {
            let layout_ids = self.live_layout_ids.iter().cloned().collect::<Vec<_>>();
            let removal_refs = layout_ids.iter().map(String::as_str).collect::<Vec<_>>();
            lifecycle.extend(
                self.text_resources
                    .update_frame(&[], &removal_refs)?
                    .lifecycle,
            );
            self.live_layout_ids.clear();
            self.ui_text_measurer.begin_frame()?;
        }
        lifecycle.push(SceneCommand::rect(
            "vn.frame.clear",
            0,
            0,
            self.width,
            self.height,
            [8, 10, 16, 255],
        ));
        lifecycle.extend(
            next_stage_scene
                .as_ref()
                .map_or(self.scene_draw.as_slice(), |(scene_draw, _)| {
                    scene_draw.as_slice()
                })
                .iter()
                .cloned(),
        );
        let ui_draw = ui_frame.map_or_else(Vec::new, |frame| frame.draw);
        lifecycle.extend(ui_draw.iter().cloned());
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        tracing::trace!(
            event = "player.vn.runtime.command.emit",
            sequence = self.command_sequence,
            presentation_count,
            scene_command_count = lifecycle.len(),
            "emitted AstraVN Player host command"
        );
        if let Some((scene_draw, _texture_ids)) = next_stage_scene {
            self.scene_draw = scene_draw;
        }
        if let Some(stage_director) = next_stage_director {
            self.stage_director = stage_director;
        }
        self.ui_draw = ui_draw;
        if !next_audio.is_empty() {
            tracing::trace!(
                event = "player.vn.audio_sequence.queued",
                fixed_step = self.fixed_step,
                media_command_count = next_audio.len(),
                "queued audio starts and controls in Runtime output order"
            );
        }
        self.pending_audio.extend(next_audio);
        let batch = PlayerHostCommandBatch::new(vec![PlayerHostCommand::PresentScene {
            sequence: self.command_sequence,
            surface: self.surface,
            width: self.width,
            height: self.height,
            clear_rgba: [8, 10, 16, 255],
            commands: lifecycle,
            semantics: self.ui_semantics.clone(),
        }])?;
        let scene_compose_ns = performance_phase_duration(scene_compose_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.scene_compose_ns = sample.scene_compose_ns.saturating_add(scene_compose_ns);
        }
        Ok(batch)
    }

    fn ensure_stage_textures(
        &mut self,
        state: &ProductStageState,
    ) -> Result<(), NativeVnHostError> {
        let mut required = state
            .entities
            .values()
            .filter(|entity| entity.visible)
            .map(|entity| entity.asset.clone())
            .collect::<BTreeSet<_>>();
        for movie in state.movies.values() {
            if !self.textures.contains_key(&movie.asset) {
                if let Some(fallback) = &movie.fallback {
                    required.insert(fallback.clone());
                }
            }
        }
        for asset_id in &required {
            if !self.textures.contains_key(asset_id) {
                let cache_hit = self.asset_store.is_image_cached(asset_id)?;
                let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
                let frame = self.asset_store.load_image(asset_id)?;
                let duration_ns = performance_phase_duration(started)?;
                tracing::debug!(
                    event = "player.stage_texture.materialized",
                    asset_hash = %Hash256::from_sha256(asset_id.as_bytes()),
                    cache_hit,
                    duration_ns,
                    byte_count = frame.rgba8.len(),
                    "materialized a stage texture for the current presentation state"
                );
                self.store_texture(asset_id.clone(), frame, &required)?;
            }
        }
        Ok(())
    }

    fn store_texture(
        &mut self,
        asset_id: String,
        frame: TextureFrame,
        protected: &BTreeSet<String>,
    ) -> Result<(), NativeVnHostError> {
        let incoming_bytes = frame.rgba8.len() as u64;
        if incoming_bytes == 0 || incoming_bytes > self.texture_cpu_budget_bytes {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_CPU_TEXTURE_ENTRY_BUDGET".into(),
            ));
        }
        if let Some(previous) = self.textures.remove(&asset_id) {
            self.texture_cpu_bytes = self
                .texture_cpu_bytes
                .checked_sub(previous.rgba8.len() as u64)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_UNDERFLOW".into())
                })?;
        }
        self.texture_use_clock = self
            .texture_use_clock
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        self.texture_cpu_last_used
            .insert(asset_id.clone(), self.texture_use_clock);
        self.texture_cpu_bytes = self
            .texture_cpu_bytes
            .checked_add(incoming_bytes)
            .ok_or_else(|| {
                NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_OVERFLOW".into())
            })?;
        self.textures.insert(asset_id, frame);

        while self.texture_cpu_bytes > self.texture_cpu_budget_bytes {
            let candidate = self
                .textures
                .keys()
                .filter(|candidate| !protected.contains(*candidate))
                .map(|candidate| {
                    (
                        self.texture_cpu_last_used
                            .get(candidate)
                            .copied()
                            .unwrap_or(0),
                        candidate.clone(),
                    )
                })
                .min();
            let Some((_, candidate)) = candidate else {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_CPU_TEXTURE_BUDGET_PINNED".into(),
                ));
            };
            let evicted = self.textures.remove(&candidate).ok_or_else(|| {
                NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_EVICTION_MISSING".into())
            })?;
            self.texture_cpu_bytes = self
                .texture_cpu_bytes
                .checked_sub(evicted.rgba8.len() as u64)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_UNDERFLOW".into())
                })?;
            self.texture_cpu_last_used.remove(&candidate);
            tracing::debug!(
                event = "player.image.cpu_residency.evicted",
                asset_hash = %Hash256::from_sha256(candidate.as_bytes()),
                resident_bytes = self.texture_cpu_bytes,
                budget_bytes = self.texture_cpu_budget_bytes,
                "evicted an inactive decoded stage texture within the profile-bound CPU budget"
            );
        }
        Ok(())
    }

    fn remove_texture(
        &mut self,
        asset_id: &str,
    ) -> Result<Option<TextureFrame>, NativeVnHostError> {
        let removed = self.textures.remove(asset_id);
        if let Some(frame) = &removed {
            self.texture_cpu_bytes = self
                .texture_cpu_bytes
                .checked_sub(frame.rgba8.len() as u64)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_UNDERFLOW".into())
                })?;
            self.texture_cpu_last_used.remove(asset_id);
        }
        Ok(removed)
    }

    fn mark_texture_used(&mut self, asset_id: &str) -> Result<(), NativeVnHostError> {
        self.texture_use_clock = self
            .texture_use_clock
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        self.texture_last_used
            .insert(asset_id.to_string(), self.texture_use_clock);
        self.texture_cpu_last_used
            .insert(asset_id.to_string(), self.texture_use_clock);
        Ok(())
    }

    fn evict_gpu_textures_for(
        &mut self,
        incoming_bytes: u64,
        protected: &BTreeSet<String>,
        lifecycle: &mut Vec<SceneCommand>,
    ) -> Result<(), NativeVnHostError> {
        if incoming_bytes > self.gpu_texture_budget_bytes {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_GPU_TEXTURE_OVER_BUDGET: {incoming_bytes} > {}",
                self.gpu_texture_budget_bytes
            )));
        }
        let mut evicted_count = 0u64;
        let mut evicted_bytes = 0u64;
        while self
            .resident_texture_bytes
            .checked_add(incoming_bytes)
            .is_none_or(|total| total > self.gpu_texture_budget_bytes)
        {
            let candidate = self
                .live_texture_ids
                .iter()
                .filter(|asset_id| !protected.contains(*asset_id))
                .map(|asset_id| {
                    (
                        self.texture_last_used.get(asset_id).copied().unwrap_or(0),
                        asset_id.clone(),
                    )
                })
                .min();
            let Some((_, asset_id)) = candidate else {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_GPU_TEXTURE_BUDGET_PINNED".into(),
                ));
            };
            let bytes = self
                .live_texture_bytes
                .get(&asset_id)
                .copied()
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_METADATA_MISSING".into())
                })?;
            self.resident_texture_bytes = self
                .resident_texture_bytes
                .checked_sub(bytes)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_BYTES_UNDERFLOW".into())
                })?;
            self.live_texture_ids.remove(&asset_id);
            self.live_texture_bytes.remove(&asset_id);
            self.texture_last_used.remove(&asset_id);
            lifecycle.push(SceneCommand::ReleaseResource {
                resource_id: asset_id,
            });
            evicted_count += 1;
            evicted_bytes = evicted_bytes.saturating_add(bytes);
        }
        if evicted_count > 0 {
            tracing::debug!(
                event = "player.image.gpu_residency.evicted",
                evicted_count,
                evicted_bytes,
                resident_bytes = self.resident_texture_bytes,
                budget_bytes = self.gpu_texture_budget_bytes,
                "evicted least-recently-used stage textures within the profile-bound budget"
            );
        }
        Ok(())
    }

    pub fn decoded_asset_cache_bytes(&self) -> u64 {
        self.asset_store.cache_bytes()
    }

    fn poll_image_prefetch(&mut self) -> Result<(), NativeVnHostError> {
        if let Some(error) = &self.image_prefetch_failure {
            return Err(NativeVnHostError::Asset(error.clone()));
        }
        for (asset_id, result) in self.image_prefetcher.drain_completions()? {
            self.image_prefetch_inflight.remove(&asset_id);
            if let Err(error) = result {
                let error = format!(
                    "ASTRA_PLAYER_IMAGE_PREFETCH_FAILED: asset_hash={}, cause={error}",
                    Hash256::from_sha256(asset_id.as_bytes())
                );
                self.image_prefetch_failure = Some(error.clone());
                return Err(NativeVnHostError::Asset(error));
            }
        }
        Ok(())
    }

    fn schedule_current_image_prefetch(&mut self) -> Result<(), NativeVnHostError> {
        self.poll_image_prefetch()?;
        let Some(state_id) = self
            .runtime_state
            .as_ref()
            .and_then(|state| state.cursor.as_ref())
            .map(|cursor| cursor.state_id.as_str())
        else {
            return Ok(());
        };
        let Some(assets) = self.image_prefetch_windows.get(state_id) else {
            self.asset_store.pin_image_working_set(&[])?;
            return Ok(());
        };
        self.asset_store.pin_image_working_set(assets)?;
        for asset_id in assets {
            if self.textures.contains_key(asset_id)
                || self.image_prefetch_inflight.contains(asset_id)
                || self.asset_store.is_image_cached(asset_id)?
            {
                continue;
            }
            if !self.image_prefetcher.try_schedule(asset_id.clone())? {
                break;
            }
            self.image_prefetch_inflight.insert(asset_id.clone());
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
        ("presentation", "astra.renderer.wgpu", "renderer2d.wgpu"),
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

fn validate_saved_runtime_state(sections: &RuntimeSaveSections) -> Result<(), NativeVnHostError> {
    saved_runtime_state(sections).map(|_| ())
}

fn saved_runtime_state(
    sections: &RuntimeSaveSections,
) -> Result<VnRuntimeState, NativeVnHostError> {
    let [section] = sections.sections.as_slice() else {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_RUNTIME_SECTION_SET: exactly one runtime.world section is required"
                .into(),
        ));
    };
    if section.section_id != "runtime.world"
        || section.schema != "astra.runtime.save_blob.v2"
        || section.version != SchemaVersion::new(2, 0, 0)
        || section.codec != RuntimeSectionCodec::Raw
        || Hash256::from_sha256(&section.bytes) != section.hash
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_INTEGRITY: runtime section contract or hash mismatch".into(),
        ));
    }
    let snapshot = astra_runtime::read_runtime_save(
        &astra_runtime::SaveBlob(section.bytes.clone()),
        &astra_core::SchemaMigrationRegistry::default(),
    )
    .map_err(|error| NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}")))?;
    let mut states = snapshot
        .actors
        .actor_snapshots()
        .into_iter()
        .flat_map(|actor| snapshot.actors.component_snapshots(actor.actor_id))
        .filter(|component| component.payload.schema().as_str() == VN_RUNTIME_STATE_SCHEMA)
        .map(|component| {
            component
                .payload
                .decode::<VnRuntimeState>()
                .map_err(|error| {
                    NativeVnHostError::Save(format!(
                        "ASTRA_PLAYER_SAVE_RUNTIME_STATE_INVALID: {error}"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if states.len() != 1 {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_RUNTIME_STATE_SET: runtime save must contain exactly one VN state"
                .into(),
        ));
    }
    Ok(states.remove(0))
}

fn player_timeline_task(
    task: astra_vn_core::VnTimelineTask,
) -> Result<PlayerTimelineTask, NativeVnHostError> {
    let (task_id, target, action, duration_ms, fence) = match task.command {
        TimelineCommand::Start(spec) => {
            let duration_ms = spec
                .tracks
                .iter()
                .flat_map(|track| track.keyframes.iter())
                .map(|keyframe| u64::from(keyframe.time_ms))
                .max();
            let target = spec.tracks.first().map(|track| track.target.clone());
            (
                spec.id,
                target,
                PlayerTimelineTaskAction::Start,
                duration_ms,
                spec.fence,
            )
        }
        TimelineCommand::Cancel { id, .. } => {
            (id, None, PlayerTimelineTaskAction::Cancel, None, None)
        }
    };
    Ok(PlayerTimelineTask {
        schema: "astra.player_timeline_task.v1".to_string(),
        task_id,
        target,
        action,
        duration_ms,
        fence,
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

fn image_prefetch_windows(
    story: &CompiledStory,
    asset_store: &PackageAssetStore,
) -> Result<BTreeMap<String, Vec<String>>, NativeVnHostError> {
    const LOOKAHEAD_ASSET_COUNT: usize = 16;

    let authored_successors = story
        .stories
        .iter()
        .flat_map(|authored_story| authored_story.states.windows(2))
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect::<BTreeMap<_, _>>();
    let state_ids = story.states.keys().cloned().collect::<BTreeSet<_>>();
    let mut windows = BTreeMap::new();
    for authored_story in &story.stories {
        for state_id in &authored_story.states {
            let mut seen = BTreeSet::new();
            let mut visited_states = BTreeSet::new();
            let mut pending_states = VecDeque::from([state_id.clone()]);
            let mut assets = Vec::new();
            'states: while let Some(candidate_id) = pending_states.pop_front() {
                if !visited_states.insert(candidate_id.clone()) {
                    continue;
                }
                let state = story.states.get(&candidate_id).ok_or_else(|| {
                    NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_IMAGE_PREFETCH_STATE_MISSING: {candidate_id}"
                    ))
                })?;
                for scene in &state.scenes {
                    for command in &scene.commands {
                        let CompiledCommand::Presentation {
                            command: PresentationCommand::Stage(stage),
                            ..
                        } = command
                        else {
                            continue;
                        };
                        let asset = match stage {
                            StageCommand::Preload { asset }
                            | StageCommand::Background { asset, .. }
                            | StageCommand::Show { asset, .. } => Some(asset),
                            StageCommand::Movie { fallback, .. } => fallback.as_ref(),
                            _ => None,
                        };
                        let Some(asset) = asset.filter(|asset| asset_store.contains_image(asset))
                        else {
                            continue;
                        };
                        if seen.insert(asset.clone()) {
                            assets.push(asset.clone());
                            if assets.len() == LOOKAHEAD_ASSET_COUNT {
                                break 'states;
                            }
                        }
                    }
                }
                for successor in state_prefetch_successors(state, &authored_successors) {
                    let successor_state = astra_vn_core::resolve_target(&successor, &state_ids);
                    if !story.states.contains_key(&successor_state) {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_IMAGE_PREFETCH_TARGET_MISSING: {successor}"
                        )));
                    }
                    if !visited_states.contains(&successor_state) {
                        pending_states.push_back(successor_state);
                    }
                }
            }
            windows.insert(state_id.clone(), assets);
        }
    }
    Ok(windows)
}

fn system_action_gameplay_entry_states(
    states: &BTreeMap<String, State>,
    actions: &BTreeMap<String, astra_vn_core::SystemActionProgram>,
    system_story_ids: &BTreeSet<&str>,
) -> Vec<String> {
    let state_ids = states.keys().cloned().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    actions
        .values()
        .flat_map(|program| program.effects.iter())
        .filter_map(|effect| match effect {
            SystemActionEffect::Jump { target } => Some(target),
            _ => None,
        })
        // System actions use the same author-facing target grammar as story
        // commands. A raw lookup discards legal label aliases, leaving the
        // launch prewarm unable to cover the first physical title action.
        .map(|target| astra_vn_core::resolve_target(target, &state_ids))
        .filter(|state_id| {
            states
                .get(state_id)
                .is_some_and(|state| !system_story_ids.contains(state.story_id.as_str()))
        })
        .filter(|state_id| seen.insert(state_id.clone()))
        .collect()
}

fn default_runtime_launch_state(stories: &[astra_vn_core::Story]) -> Option<String> {
    let story = stories
        .iter()
        .find(|story| story.id == "story.main")
        .or_else(|| stories.first())?;
    story
        .states
        .iter()
        .find(|state_id| state_id.as_str() == "state.prologue")
        .or_else(|| story.states.first())
        .cloned()
}

fn state_prefetch_successors(
    state: &State,
    authored_successors: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut successors = Vec::new();
    let mut replaces_fallthrough = false;
    let mut preserves_fallthrough = false;
    for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
        match command {
            CompiledCommand::Choice { options, .. } => {
                replaces_fallthrough = true;
                successors.extend(options.iter().map(|option| option.target.clone()));
            }
            CompiledCommand::Jump { target, .. } => {
                replaces_fallthrough = true;
                successors.push(target.clone());
            }
            CompiledCommand::Branch {
                then_target,
                else_target,
                ..
            } => {
                replaces_fallthrough = true;
                successors.push(then_target.clone());
                successors.push(else_target.clone());
            }
            CompiledCommand::Call { target, .. } => {
                preserves_fallthrough = true;
                successors.push(target.clone());
            }
            _ => {}
        }
    }
    if !replaces_fallthrough || preserves_fallthrough {
        if let Some(successor) = authored_successors.get(&state.id) {
            successors.push(successor.clone());
        }
    }
    let mut seen = BTreeSet::new();
    successors.retain(|successor| seen.insert(successor.clone()));
    successors
}

fn stage_scene_commands(
    state: &ProductStageState,
    textures: &BTreeMap<String, TextureFrame>,
    width: u32,
    height: u32,
) -> Result<Vec<SceneCommand>, NativeVnHostError> {
    let mut commands = Vec::new();
    let rotated = state.camera.rotation.millionths != 0;
    if rotated {
        let radians = (state.camera.rotation.millionths as f32 / 1_000_000.0).to_radians();
        let (sin, cos) = radians.sin_cos();
        let center_x = width as f32 / 2.0;
        let center_y = height as f32 / 2.0;
        commands.push(SceneCommand::PushTransform {
            transform: Transform2D {
                m11: cos,
                m12: sin,
                m21: -sin,
                m22: cos,
                tx: center_x - cos * center_x + sin * center_y,
                ty: center_y - sin * center_x - cos * center_y,
            },
        });
    }
    if let Some(color) = state.backdrop_color {
        commands.push(SceneCommand::rect(
            "vn.scene.backdrop",
            0,
            0,
            width,
            height,
            color,
        ));
    }
    let mut entities = state
        .entities
        .values()
        .filter(|entity| {
            entity.visible
                && state
                    .layers
                    .get(&entity.layer)
                    .is_some_and(|layer| layer.visible)
        })
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
                entity.fit,
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
    if state.shade_opacity.millionths > 0 {
        let shade_alpha = u8::try_from(
            state
                .shade_opacity
                .millionths
                .checked_mul(255)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_SHADE_ALPHA_OVERFLOW".into())
                })?
                / 1_000_000,
        )
        .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_SHADE_ALPHA_RANGE".into()))?;
        commands.push(SceneCommand::rect(
            "vn.scene.shade",
            0,
            0,
            width,
            height,
            [
                state.shade_color[0],
                state.shade_color[1],
                state.shade_color[2],
                shade_alpha,
            ],
        ));
    }
    let mut movies = state.movies.values().collect::<Vec<_>>();
    movies.sort_by(|left, right| {
        let left_z = state
            .layers
            .get(&left.layer)
            .map_or(i32::MIN, |layer| layer.z);
        let right_z = state
            .layers
            .get(&right.layer)
            .map_or(i32::MIN, |layer| layer.z);
        left_z.cmp(&right_z).then(left.layer.cmp(&right.layer))
    });
    for movie in movies {
        let layer = state.layers.get(&movie.layer).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_STAGE_LAYER_UNKNOWN: movie {} references an undeclared layer",
                movie.asset
            ))
        })?;
        let frame = if let Some(frame) = textures.get(&movie.asset) {
            frame
        } else {
            let fallback = movie
                .fallback
                .as_deref()
                .ok_or_else(|| missing_texture(&movie.asset))?;
            textures
                .get(fallback)
                .ok_or_else(|| missing_texture(fallback))?
        };
        commands.push(SceneCommand::VideoFrame {
            id: format!("vn.movie.{}", movie.layer),
            frame: frame.clone(),
            destination: RectI::new(0, 0, width, height),
            opacity: fixed_opacity(movie.alpha.millionths)?,
            blend: stage_blend(layer.blend)?,
            presentation_time_ns: state.elapsed_ns,
        });
    }
    let mut effects = state.effects.values().collect::<Vec<_>>();
    effects.sort_by(|left, right| left.target.cmp(&right.target));
    for effect in effects {
        commands.push(SceneCommand::FilterGraph {
            graph: effect_filter_graph(effect)?,
        });
    }
    if rotated {
        commands.push(SceneCommand::PopTransform);
    }
    Ok(commands)
}

fn effect_filter_graph(
    effect: &astra_vn_package::ProductStageEffect,
) -> Result<FilterGraph, NativeVnHostError> {
    let (kind, params) = match effect.filter.as_str() {
        "soft_glow" | "astra.filter.bloom" => (
            "astra.filter.bloom",
            BTreeMap::from([("intensity".to_string(), FilterParam::Float(0.15))]),
        ),
        "astra.filter.fade" => (
            "astra.filter.fade",
            BTreeMap::from([("amount".to_string(), FilterParam::Float(0.15))]),
        ),
        other => {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_FILTER_UNSUPPORTED: {other}"
            )))
        }
    };
    Ok(FilterGraph {
        schema: "astra.filter_graph.v1".to_string(),
        nodes: vec![FilterNode {
            id: format!("vn.effect.{}", effect.target),
            kind: kind.to_string(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params,
            deterministic: true,
            allow_cpu_fallback: true,
        }],
    })
}

fn entity_destination(
    state: &ProductStageState,
    texture: &TextureFrame,
    fit: StageFitMode,
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
    let (base_width, base_height) = match fit {
        StageFitMode::ContainHeight => {
            let height_fit = height.saturating_mul(9) / 10;
            let width_fit =
                (u64::from(height_fit) * u64::from(texture.width)) / u64::from(texture.height);
            if width_fit <= u64::from(width) {
                (width_fit, u64::from(height_fit))
            } else {
                (
                    u64::from(width),
                    u64::from(width) * u64::from(texture.height) / u64::from(texture.width),
                )
            }
        }
        StageFitMode::Native => {
            let viewport_width = u64::from(state.viewport.width);
            let viewport_height = u64::from(state.viewport.height);
            if viewport_width == 0 || viewport_height == 0 {
                return Err(stage_coordinate_error());
            }
            let scale_numerator =
                (u64::from(width) * viewport_height).min(u64::from(height) * viewport_width);
            (
                (u64::from(texture.width) * scale_numerator / viewport_width / viewport_height)
                    .max(1),
                (u64::from(texture.height) * scale_numerator / viewport_width / viewport_height)
                    .max(1),
            )
        }
    };
    let destination_width = scale_dimension(base_width, zoom)?;
    let destination_height = scale_dimension(base_height, zoom)?;
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
        StageBlendMode::Add => Ok(BlendMode::Add),
        StageBlendMode::Multiply => Ok(BlendMode::Multiply),
        StageBlendMode::Screen => Ok(BlendMode::Screen),
    }
}

fn stage_coordinate_error() -> NativeVnHostError {
    NativeVnHostError::Asset(
        "ASTRA_PLAYER_STAGE_COORDINATE: stage transform exceeds renderer coordinate limits"
            .to_string(),
    )
}

fn blueprint_view_localization_keys(
    bundle: &UiBlueprintBundle,
) -> BTreeMap<String, BTreeSet<String>> {
    bundle
        .views
        .iter()
        .map(|(id, view)| {
            let mut keys = BTreeSet::new();
            collect_node_localization_keys(&view.root, &mut keys);
            (id.clone(), keys)
        })
        .collect()
}

fn collect_node_localization_keys(node: &UiNodeBlueprint, keys: &mut BTreeSet<String>) {
    for expression in node.properties.values() {
        collect_expression_localization_keys(expression, keys);
    }
    for event in &node.events {
        for expression in event.arguments.values() {
            collect_expression_localization_keys(expression, keys);
        }
    }
    if let Some(repeat) = &node.repeat {
        collect_expression_localization_keys(&repeat.items, keys);
    }
    for child in &node.children {
        collect_node_localization_keys(child, keys);
    }
}

fn collect_expression_localization_keys(expression: &UiValueExpr, keys: &mut BTreeSet<String>) {
    match expression {
        UiValueExpr::LocalizationKey { key } => {
            keys.insert(key.clone());
        }
        UiValueExpr::Literal { .. }
        | UiValueExpr::Binding { .. }
        | UiValueExpr::AssetRef { .. }
        | UiValueExpr::ThemeToken { .. } => {}
    }
}

fn collect_value_strings(value: &UiValue, keys: &mut BTreeSet<String>) {
    match value {
        UiValue::String(value) => {
            keys.insert(value.clone());
        }
        UiValue::List(values) => {
            for value in values {
                collect_value_strings(value, keys);
            }
        }
        UiValue::Map(values) => {
            for value in values.values() {
                collect_value_strings(value, keys);
            }
        }
        UiValue::Null | UiValue::Bool(_) | UiValue::Integer(_) | UiValue::Number(_) => {}
    }
}

fn frame_localization_subset(
    dictionary: &BTreeMap<String, String>,
    view_keys: &BTreeMap<String, BTreeSet<String>>,
    view_id: &str,
    model: &UiValue,
    state: &UiValue,
    modals: &[UiBlueprintModalFrameModel],
) -> Result<BTreeMap<String, String>, NativeVnHostError> {
    let mut static_required = view_keys.get(view_id).cloned().ok_or_else(|| {
        NativeVnHostError::Localization(format!(
            "ASTRA_PLAYER_UI_LOCALIZATION_VIEW_MISSING: view {view_id} has no localization projection"
        ))
    })?;
    for modal in modals {
        static_required.extend(view_keys.get(&modal.view_id).cloned().ok_or_else(|| {
            NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_UI_LOCALIZATION_VIEW_MISSING: modal view {} has no localization projection",
                modal.view_id
            ))
        })?);
    }
    if let Some(missing) = static_required
        .iter()
        .find(|key| !dictionary.contains_key(*key))
    {
        return Err(NativeVnHostError::Localization(format!(
            "ASTRA_PLAYER_UI_LOCALIZATION_MISSING: projected key {missing} is absent"
        )));
    }
    let mut required = static_required;
    collect_value_strings(model, &mut required);
    collect_value_strings(state, &mut required);
    for modal in modals {
        collect_value_strings(&modal.model, &mut required);
        collect_value_strings(&modal.state, &mut required);
    }
    Ok(required
        .into_iter()
        .filter_map(|key| dictionary.get(&key).cloned().map(|value| (key, value)))
        .collect())
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
fn append_text_value(
    measured: &MeasuredUiTextLayout,
    layout_ids: &mut BTreeSet<String>,
    pending: &mut Vec<PendingUiTextLayout>,
    layout_id: &str,
    text: &str,
    x: u32,
    y: u32,
    max_width: u32,
    max_height: u32,
    font_size: f32,
    max_lines: u32,
    direction: TextDirection,
    rgba: [u8; 4],
    horizontal_align: UiTextAlignment,
    vertical_align: UiTextAlignment,
) -> Result<(), NativeVnHostError> {
    if measured.text != text
        || measured.font_size.to_bits() != font_size.to_bits()
        || measured.max_lines != max_lines
        || measured.direction != direction
    {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_IDENTITY: measured layout does not match semantic node {layout_id}"
        )));
    }
    let layout = Arc::clone(&measured.layout);
    // Keep the existing scene-placement contract: Yakui's semantic bounds
    // describe the widget box, while text padding is applied by the scene
    // bridge. The authoritative measured layout may therefore be wider than
    // the inner box for compact fixed-size controls; alignment remains
    // clamped to that box exactly as before the measured-layout handoff.
    let layout_width = layout.width.ceil().clamp(0.0, max_width as f32) as u32;
    let layout_height = layout.height.ceil().clamp(0.0, max_height as f32) as u32;
    let aligned_x = x.saturating_add(horizontal_align.offset(max_width, layout_width));
    let aligned_y = y.saturating_add(vertical_align.offset(max_height, layout_height));
    let aligned_x = i32::try_from(aligned_x).map_err(|_| {
        NativeVnHostError::Asset("ASTRA_PLAYER_LAYOUT_COORDINATE: x exceeds i32".into())
    })?;
    let aligned_y = i32::try_from(aligned_y).map_err(|_| {
        NativeVnHostError::Asset("ASTRA_PLAYER_LAYOUT_COORDINATE: y exceeds i32".into())
    })?;
    if !layout_ids.insert(layout_id.to_string()) {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_LAYOUT_DUPLICATE: layout id {layout_id} was emitted twice"
        )));
    }
    pending.push(PendingUiTextLayout {
        layout_id: layout_id.to_string(),
        layout,
        rgba,
        aligned_x,
        aligned_y,
    });
    Ok(())
}

struct PendingUiTextLayout {
    layout_id: String,
    layout: Arc<TextLayoutResult>,
    rgba: [u8; 4],
    aligned_x: i32,
    aligned_y: i32,
}

#[allow(clippy::too_many_arguments)]
fn append_ui_semantic_text(
    localization: &VnLocalizationTable,
    measured_layouts: &BTreeMap<String, MeasuredUiTextLayout>,
    layout_ids: &mut BTreeSet<String>,
    pending: &mut Vec<PendingUiTextLayout>,
    semantics: &UiSemanticSnapshot,
    body_font_size: f32,
) -> Result<(), NativeVnHostError> {
    for node in &semantics.nodes {
        if !matches!(
            node.role,
            UiSemanticRole::Text
                | UiSemanticRole::Button
                | UiSemanticRole::Toggle
                | UiSemanticRole::Slider
                | UiSemanticRole::Select
                | UiSemanticRole::TextInput
        ) {
            continue;
        }
        let Some(name) = node.name.as_deref() else {
            continue;
        };
        let text = localization
            .strings
            .get(name)
            .map(String::as_str)
            .unwrap_or(name);
        let x = non_negative_coord(node.bounds_points.min.x, "x")?;
        let y = non_negative_coord(node.bounds_points.min.y, "y")?;
        let width =
            non_negative_coord(node.bounds_points.max.x - node.bounds_points.min.x, "width")?
                .max(1);
        let height = non_negative_coord(
            node.bounds_points.max.y - node.bounds_points.min.y,
            "height",
        )?
        .max(1);
        let direction = parse_text_direction(node.properties.get("text.direction"))?;
        let horizontal_align = parse_ui_text_alignment(node.properties.get("text.align"))?;
        let vertical_align = parse_ui_text_alignment(node.properties.get("text.vertical_align"))?;
        let max_lines = parse_bounded_u32_property(node, "text.max_lines", 4, 1, 1_024)?;
        let font_size = parse_bounded_f32_property(
            node,
            "text.font_size",
            (body_font_size * 0.72).max(16.0),
            6.0,
            256.0,
        )?;
        let rgba = parse_ui_text_rgba(node.properties.get("text.rgba"))?;
        let measured = measured_layouts.get(&node.id).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_MISSING: Yakui did not provide the authoritative layout for {}",
                node.id
            ))
        })?;
        append_text_value(
            measured,
            layout_ids,
            pending,
            &format!("ui.text.{}", node.id),
            text,
            x.saturating_add(8),
            y.saturating_add(8),
            width.saturating_sub(16).max(1),
            height.saturating_sub(16).max(1),
            font_size,
            max_lines,
            direction,
            rgba,
            horizontal_align,
            vertical_align,
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiTextAlignment {
    Start,
    Center,
    End,
}

impl UiTextAlignment {
    fn offset(self, available: u32, content: u32) -> u32 {
        let remaining = available.saturating_sub(content);
        match self {
            Self::Start => 0,
            Self::Center => remaining / 2,
            Self::End => remaining,
        }
    }
}

fn parse_ui_text_alignment(value: Option<&String>) -> Result<UiTextAlignment, NativeVnHostError> {
    match value.map(String::as_str).unwrap_or("start") {
        "start" => Ok(UiTextAlignment::Start),
        "center" => Ok(UiTextAlignment::Center),
        "end" => Ok(UiTextAlignment::End),
        _ => Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_UI_TEXT_ALIGNMENT: alignment must be start, center or end".into(),
        )),
    }
}

fn parse_ui_text_rgba(value: Option<&String>) -> Result<[u8; 4], NativeVnHostError> {
    let Some(value) = value else {
        return Ok([255; 4]);
    };
    let channels = value
        .split(',')
        .map(|channel| {
            channel.parse::<u8>().map_err(|_| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_UI_TEXT_COLOR: text.rgba must contain four u8 channels".into(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    channels.try_into().map_err(|_| {
        NativeVnHostError::Asset(
            "ASTRA_PLAYER_UI_TEXT_COLOR: text.rgba must contain exactly four channels".into(),
        )
    })
}

fn ordered_ui_font_families(font_families: &[String], locale: &str) -> Vec<String> {
    let mut ordered = font_families.to_vec();
    ordered.sort_by_key(|family| {
        let normalized = family.to_ascii_lowercase();
        let preferred = if locale.starts_with("zh") {
            normalized.contains("sans sc")
        } else {
            normalized.contains("sans jp")
        };
        (!preferred, normalized)
    });
    ordered
}

#[cfg(test)]
mod native_vn_host_tests {
    use super::{
        default_runtime_launch_state, frame_localization_subset, ordered_ui_font_families,
        parse_ui_text_alignment, retained_pointer_activation_target, reusable_ui_draw_commands,
        save_slots_for_policy, state_prefetch_successors, system_action_gameplay_entry_states,
        NativeVnDecodedCacheBudget, ReadingMode, SaveCompletionPolicy, SystemPageKind,
        SystemUiProfilePolicy, UiBlueprintModalFrameModel, UiTextAlignment, UiValue,
        DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn reusable_ui_frame_does_not_replay_resource_lifecycle() {
        let draw = vec![
            astra_media_core::SceneCommand::ReleaseResource {
                resource_id: "glyph.cached".into(),
            },
            astra_media_core::SceneCommand::rect("ui.cached.panel", 0, 0, 8, 8, [1, 2, 3, 255]),
        ];

        assert_eq!(
            reusable_ui_draw_commands(&draw),
            vec![astra_media_core::SceneCommand::rect(
                "ui.cached.panel",
                0,
                0,
                8,
                8,
                [1, 2, 3, 255],
            )],
        );
    }

    #[test]
    fn image_prefetch_control_flow_prioritizes_branch_targets_over_linear_fallthrough() {
        let state = astra_vn_core::State {
            id: "root".into(),
            name: "root".into(),
            story_id: "story".into(),
            scenes: vec![astra_vn_core::Scene {
                id: "scene".into(),
                name: "scene".into(),
                commands: vec![astra_vn_core::CompiledCommand::Branch {
                    id: "branch".into(),
                    scope: "session".into(),
                    key: "route".into(),
                    op: astra_vn_core::BranchOp::Eq,
                    value: 1,
                    then_target: "then".into(),
                    else_target: "else".into(),
                }],
            }],
        };
        let authored = BTreeMap::from([("root".into(), "linear".into())]);
        assert_eq!(
            state_prefetch_successors(&state, &authored),
            vec!["then".to_string(), "else".to_string()]
        );
    }

    #[test]
    fn image_prefetch_control_flow_retains_call_return_fallthrough() {
        let state = astra_vn_core::State {
            id: "root".into(),
            name: "root".into(),
            story_id: "story".into(),
            scenes: vec![astra_vn_core::Scene {
                id: "scene".into(),
                name: "scene".into(),
                commands: vec![astra_vn_core::CompiledCommand::Call {
                    id: "call".into(),
                    target: "callee".into(),
                }],
            }],
        };
        let authored = BTreeMap::from([("root".into(), "linear".into())]);
        assert_eq!(
            state_prefetch_successors(&state, &authored),
            vec!["callee".to_string(), "linear".to_string()]
        );
    }

    #[test]
    fn image_prewarm_resolves_system_action_target_aliases_before_ranking_gameplay_entries() {
        let states = BTreeMap::from([
            (
                "state.title".to_string(),
                astra_vn_core::State {
                    id: "state.title".into(),
                    name: "title".into(),
                    story_id: "system.title".into(),
                    scenes: Vec::new(),
                },
            ),
            (
                "state.opening".to_string(),
                astra_vn_core::State {
                    id: "state.opening".into(),
                    name: "opening".into(),
                    story_id: "story.y".into(),
                    scenes: Vec::new(),
                },
            ),
        ]);
        let actions = BTreeMap::from([(
            "title.start".to_string(),
            astra_vn_core::SystemActionProgram {
                id: "title.start".into(),
                effects: vec![astra_vn_core::SystemActionEffect::Jump {
                    target: "opening".into(),
                }],
            },
        )]);
        let system_story_ids = BTreeSet::from(["system.title"]);

        assert_eq!(
            system_action_gameplay_entry_states(&states, &actions, &system_story_ids),
            vec!["state.opening".to_string()]
        );
    }

    #[test]
    fn image_prewarm_starts_with_the_runtime_launch_state() {
        let stories = vec![
            astra_vn_core::Story {
                id: "story.extra".into(),
                name: "extra".into(),
                states: vec!["state.extra".into()],
            },
            astra_vn_core::Story {
                id: "story.main".into(),
                name: "main".into(),
                states: vec!["state.title".into(), "state.prologue".into()],
            },
        ];
        assert_eq!(
            default_runtime_launch_state(&stories),
            Some("state.prologue".to_string())
        );
    }

    #[test]
    fn retained_pointer_activation_prefers_the_topmost_enabled_actionable_node() {
        let bounds = astra_ui_core::UiRect {
            min: astra_ui_core::UiPoint { x: 0.0, y: 0.0 },
            max: astra_ui_core::UiPoint { x: 100.0, y: 100.0 },
        };
        let node = |id: &str, actions: BTreeSet<astra_ui_core::UiSemanticAction>| {
            astra_ui_core::UiSemanticNode {
                id: id.to_string(),
                parent_id: None,
                role: astra_ui_core::UiSemanticRole::Button,
                bounds_points: bounds,
                name: None,
                description: None,
                value: None,
                enabled: true,
                hidden: false,
                focused: false,
                selected: false,
                checked: None,
                actions,
                properties: BTreeMap::new(),
            }
        };
        let snapshot = astra_ui_core::UiSemanticSnapshot {
            schema: "astra.ui_semantics.v1".to_string(),
            session_id: "test".to_string(),
            generation: 1,
            root_id: "root".to_string(),
            nodes: vec![
                node("root", BTreeSet::new()),
                node(
                    "root/advance",
                    BTreeSet::from([astra_ui_core::UiSemanticAction::Activate]),
                ),
            ],
            hash: astra_core::Hash256::from_sha256(b"retained-pointer"),
        };
        assert_eq!(
            retained_pointer_activation_target(
                &snapshot,
                astra_ui_core::UiPoint { x: 50.0, y: 50.0 },
            )
            .map(|node| node.id.as_str()),
            Some("root/advance")
        );
    }

    fn packaged_families() -> Vec<String> {
        vec![
            "Noto Sans JP".to_string(),
            "Noto Sans SC".to_string(),
            "Astra Symbols".to_string(),
        ]
    }

    #[astra_headless_test::test]
    fn locale_selects_the_matching_cjk_font_without_dropping_fallbacks() {
        let japanese = ordered_ui_font_families(&packaged_families(), "ja");
        let simplified_chinese = ordered_ui_font_families(&packaged_families(), "zh-Hans");

        assert_eq!(japanese[0], "Noto Sans JP");
        assert_eq!(simplified_chinese[0], "Noto Sans SC");
        assert_eq!(japanese.len(), 3);
        assert_eq!(simplified_chinese.len(), 3);
    }

    #[astra_headless_test::test]
    fn non_cjk_locale_uses_the_packaged_japanese_baseline_deterministically() {
        let first = ordered_ui_font_families(&packaged_families(), "en");
        let second = ordered_ui_font_families(&packaged_families(), "en");

        assert_eq!(first[0], "Noto Sans JP");
        assert_eq!(first, second);
    }

    #[astra_headless_test::test]
    fn decoded_cache_partition_preserves_the_profile_bound() {
        let budget =
            NativeVnDecodedCacheBudget::partition(DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES).unwrap();
        assert_eq!(budget.asset_bytes, 48 * 1024 * 1024);
        assert_eq!(budget.audio_bytes, 143 * 1024 * 1024);
        assert_eq!(budget.glyph_bytes, 1024 * 1024);
        assert_eq!(
            budget.asset_bytes + budget.audio_bytes + budget.glyph_bytes,
            DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES
        );
        assert!(NativeVnDecodedCacheBudget::partition(3).is_err());
    }

    #[astra_headless_test::test]
    fn semantic_text_alignment_uses_bounded_container_offsets() {
        assert_eq!(UiTextAlignment::Start.offset(754, 320), 0);
        assert_eq!(UiTextAlignment::Center.offset(754, 320), 217);
        assert_eq!(UiTextAlignment::End.offset(754, 320), 434);
        assert_eq!(UiTextAlignment::Center.offset(320, 754), 0);
    }

    #[astra_headless_test::test]
    fn invalid_semantic_text_alignment_is_blocking() {
        let invalid = "middle".to_string();
        let error = parse_ui_text_alignment(Some(&invalid)).unwrap_err();
        assert!(error.to_string().contains("ASTRA_PLAYER_UI_TEXT_ALIGNMENT"));
    }

    #[astra_headless_test::test]
    fn save_slot_view_model_is_created_only_from_the_bound_profile_policy() {
        let policy = SystemUiProfilePolicy {
            profile_id: "classic".into(),
            save_slot_ids: (1..=8).map(|index| format!("slot.{index:02}")).collect(),
            quick_slot_id: None,
            allowed_pages: BTreeSet::from([SystemPageKind::Save, SystemPageKind::Load]),
            reading_modes: BTreeSet::from([ReadingMode::Manual]),
            audio_toggle: true,
            save_completion: SaveCompletionPolicy::ReturnSystem,
            custom_action_ids: BTreeSet::new(),
        };
        let slots = save_slots_for_policy(&policy);
        assert_eq!(slots.len(), 8);
        assert!(slots.contains_key("slot.01"));
        assert!(slots.contains_key("slot.08"));
        assert!(!slots.contains_key("slot.09"));
        assert!(!slots.contains_key("slot.quick"));
    }

    #[astra_headless_test::test]
    fn frame_localization_projects_only_static_and_live_model_keys() {
        let dictionary = BTreeMap::from([
            ("ui.title".into(), "Title".into()),
            ("story.current".into(), "Current line".into()),
            ("story.unused".into(), "Unused line".into()),
        ]);
        let view_keys = BTreeMap::from([("view.main".into(), BTreeSet::from(["ui.title".into()]))]);
        let model = UiValue::Map(BTreeMap::from([(
            "text".into(),
            UiValue::String("story.current".into()),
        )]));
        let projected = frame_localization_subset(
            &dictionary,
            &view_keys,
            "view.main",
            &model,
            &UiValue::Null,
            &[],
        )
        .unwrap();
        assert_eq!(projected.len(), 2);
        assert_eq!(projected["ui.title"], "Title");
        assert_eq!(projected["story.current"], "Current line");
        assert!(!projected.contains_key("story.unused"));
    }

    #[astra_headless_test::test]
    fn frame_localization_rejects_missing_static_or_modal_projection() {
        let dictionary = BTreeMap::new();
        let view_keys =
            BTreeMap::from([("view.main".into(), BTreeSet::from(["ui.missing".into()]))]);
        let error = frame_localization_subset(
            &dictionary,
            &view_keys,
            "view.main",
            &UiValue::Null,
            &UiValue::Null,
            &[],
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("ASTRA_PLAYER_UI_LOCALIZATION_MISSING"));

        let modal = UiBlueprintModalFrameModel {
            view_id: "view.modal".into(),
            model_schema: "ui.modal.v1".into(),
            model: UiValue::Null,
            state: UiValue::Null,
        };
        let error = frame_localization_subset(
            &BTreeMap::new(),
            &BTreeMap::from([("view.main".into(), BTreeSet::new())]),
            "view.main",
            &UiValue::Null,
            &UiValue::Null,
            &[modal],
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("ASTRA_PLAYER_UI_LOCALIZATION_VIEW_MISSING"));
    }
}

struct NativeUiTextMeasurer {
    provider: Arc<CosmicTextLayoutProvider>,
    font_families: Arc<RwLock<Vec<String>>>,
    locale: Arc<RwLock<String>>,
    frame_layouts: Mutex<BTreeMap<String, MeasuredUiTextLayout>>,
}

#[derive(Clone)]
struct MeasuredUiTextLayout {
    text: String,
    font_size: f32,
    max_lines: u32,
    direction: TextDirection,
    layout: Arc<TextLayoutResult>,
}

impl NativeUiTextMeasurer {
    fn begin_frame(&self) -> Result<(), NativeVnHostError> {
        self.frame_layouts
            .lock()
            .map_err(|_| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_LOCK: frame layout state was poisoned".into(),
                )
            })?
            .clear();
        Ok(())
    }

    fn take_frame_layouts(
        &self,
    ) -> Result<BTreeMap<String, MeasuredUiTextLayout>, NativeVnHostError> {
        let mut layouts = self.frame_layouts.lock().map_err(|_| {
            NativeVnHostError::Asset(
                "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_LOCK: frame layout state was poisoned".into(),
            )
        })?;
        Ok(std::mem::take(&mut *layouts))
    }
}

impl AstraTextMeasurer for NativeUiTextMeasurer {
    fn measure(
        &self,
        request: &AstraTextMeasureRequest,
    ) -> Result<AstraTextMeasureResult, UiValidationError> {
        let direction = parse_text_direction(Some(&request.direction)).map_err(|error| {
            UiValidationError::invalid("ASTRA_UI_TEXT_DIRECTION", error.to_string())
        })?;
        let locale = self.locale.read().map_err(|_| {
            UiValidationError::invalid(
                "ASTRA_UI_TEXT_LOCALE_LOCK",
                "UI text locale state was poisoned",
            )
        })?;
        let font_families = self.font_families.read().map_err(|_| {
            UiValidationError::invalid(
                "ASTRA_UI_TEXT_FONT_FALLBACK_LOCK",
                "UI font fallback state was poisoned",
            )
        })?;
        let key = format!("ui.measure.{}", request.semantic_id);
        let layout_request = text_request(TextRequestSpec {
            key: &key,
            text: &request.text,
            locale: &locale,
            font_families: &font_families,
            max_width: request.max_width.max(1.0).round() as u32,
            font_size: request.font_size,
            max_lines: request.max_lines,
            direction,
        });
        let layout = self
            .provider
            .layout_shared(&layout_request)
            .map_err(|error| {
                UiValidationError::invalid("ASTRA_UI_TEXT_MEASURE", error.to_string())
            })?;
        self.frame_layouts
            .lock()
            .map_err(|_| {
                UiValidationError::invalid(
                    "ASTRA_UI_TEXT_MEASUREMENT_LOCK",
                    "UI text frame layout state was poisoned",
                )
            })?
            .insert(
                request.semantic_id.clone(),
                MeasuredUiTextLayout {
                    text: request.text.clone(),
                    font_size: request.font_size,
                    max_lines: request.max_lines,
                    direction,
                    layout: Arc::clone(&layout),
                },
            );
        Ok(AstraTextMeasureResult {
            width: layout.width,
            height: layout.height,
        })
    }
}

fn parse_text_direction(value: Option<&String>) -> Result<TextDirection, NativeVnHostError> {
    match value.map(String::as_str).unwrap_or("auto") {
        "auto" => Ok(TextDirection::Auto),
        "left_to_right" => Ok(TextDirection::LeftToRight),
        "right_to_left" => Ok(TextDirection::RightToLeft),
        "vertical_right_to_left" => Ok(TextDirection::VerticalRightToLeft),
        "vertical_left_to_right" => Ok(TextDirection::VerticalLeftToRight),
        _ => Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_UI_TEXT_DIRECTION: semantic text direction is invalid".into(),
        )),
    }
}

fn parse_bounded_u32_property(
    node: &UiSemanticNode,
    key: &str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, NativeVnHostError> {
    let Some(value) = node.properties.get(key) else {
        return Ok(default);
    };
    let value = value.parse::<f32>().map_err(|_| {
        NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is not numeric"
        ))
    })?;
    if !value.is_finite() || value.fract() != 0.0 || value < min as f32 || value > max as f32 {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is outside {min}..={max}"
        )));
    }
    Ok(value as u32)
}

fn parse_bounded_f32_property(
    node: &UiSemanticNode,
    key: &str,
    default: f32,
    min: f32,
    max: f32,
) -> Result<f32, NativeVnHostError> {
    let Some(value) = node.properties.get(key) else {
        return Ok(default);
    };
    let value = value.parse::<f32>().map_err(|_| {
        NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is not numeric"
        ))
    })?;
    if !value.is_finite() || value < min || value > max {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is outside {min}..={max}"
        )));
    }
    Ok(value)
}

fn non_negative_coord(value: f32, field: &str) -> Result<u32, NativeVnHostError> {
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f32 {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_BOUNDS: semantic {field} is outside render bounds"
        )));
    }
    Ok(value.round() as u32)
}

struct TextRequestSpec<'a> {
    key: &'a str,
    text: &'a str,
    locale: &'a str,
    font_families: &'a [String],
    max_width: u32,
    font_size: f32,
    max_lines: u32,
    direction: TextDirection,
}

fn text_request(spec: TextRequestSpec<'_>) -> TextLayoutRequest {
    let line_height = spec.font_size * 1.3;
    TextLayoutRequest {
        key: spec.key.to_string(),
        runs: vec![TextRun {
            text: spec.text.to_string(),
            language: spec.locale.to_string(),
            script: None,
            direction: spec.direction,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: spec.max_width.max(1) as f32,
            max_height: Some(line_height * spec.max_lines.max(1) as f32),
            max_lines: Some(spec.max_lines.max(1)),
            font_size: spec.font_size,
            line_height,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::EllipsisEnd,
        },
        font_families: spec.font_families.to_vec(),
        features: Vec::new(),
    }
}

fn performance_phase_started(enabled: bool) -> Option<Instant> {
    enabled.then(Instant::now)
}

fn performance_phase_duration(started: Option<Instant>) -> Result<u64, NativeVnHostError> {
    started.map_or(Ok(0), |started| {
        u64::try_from(started.elapsed().as_nanos()).map_err(|_| {
            NativeVnHostError::Input(
                "ASTRA_PLAYER_PERFORMANCE_DURATION_OVERFLOW: UI phase exceeded u64 nanoseconds"
                    .into(),
            )
        })
    })
}

fn reusable_pointer_position(
    mut current: Option<UiPoint>,
    events: &[UiInputEvent],
    semantics: &UiSemanticSnapshot,
) -> Option<Option<UiPoint>> {
    for event in events {
        match &event.kind {
            UiInputEventKind::FixedTime { .. } => {}
            UiInputEventKind::PointerMove { position } => {
                if !same_semantic_hit_region(semantics, current, Some(*position)) {
                    return None;
                }
                current = Some(*position);
            }
            _ => return None,
        }
    }
    Some(current)
}

fn pointer_position_after_events(
    mut current: Option<UiPoint>,
    events: &[UiInputEvent],
) -> Option<UiPoint> {
    for event in events {
        match &event.kind {
            UiInputEventKind::PointerMove { position }
            | UiInputEventKind::PointerButton { position, .. } => current = Some(*position),
            UiInputEventKind::Focus { focused: false } => current = None,
            _ => {}
        }
    }
    current
}

fn same_semantic_hit_region(
    semantics: &UiSemanticSnapshot,
    before: Option<UiPoint>,
    after: Option<UiPoint>,
) -> bool {
    semantics
        .nodes
        .iter()
        .all(|node| semantic_node_contains(node, before) == semantic_node_contains(node, after))
}

fn semantic_node_contains(node: &UiSemanticNode, point: Option<UiPoint>) -> bool {
    point.is_some_and(|point| {
        point.x >= node.bounds_points.min.x
            && point.x < node.bounds_points.max.x
            && point.y >= node.bounds_points.min.y
            && point.y < node.bounds_points.max.y
    })
}

fn retained_pointer_activation_target(
    semantics: &UiSemanticSnapshot,
    position: UiPoint,
) -> Option<&UiSemanticNode> {
    semantics.nodes.iter().rev().find(|node| {
        semantic_node_contains(node, Some(position))
            && node.enabled
            && !node.hidden
            && node
                .actions
                .contains(&astra_ui_core::UiSemanticAction::Activate)
    })
}

fn system_page_localization_key(page: SystemPageKind) -> Result<&'static str, NativeVnHostError> {
    match page {
        SystemPageKind::Title => Ok("system.title"),
        SystemPageKind::QuickPanel => Ok("system.quick_panel"),
        SystemPageKind::Save => Ok("system.save"),
        SystemPageKind::Load => Ok("system.load"),
        SystemPageKind::Config => Ok("system.config"),
        SystemPageKind::Gallery => Ok("system.gallery"),
        SystemPageKind::Replay => Ok("system.replay"),
        SystemPageKind::VoiceReplay => Ok("system.voice_replay"),
        SystemPageKind::RouteChart => Ok("system.route_chart"),
        SystemPageKind::Backlog => Ok("system.backlog"),
        SystemPageKind::LocalizationPreview => Ok("system.localization_preview"),
        SystemPageKind::Custom => Ok("system.custom"),
        SystemPageKind::Unknown => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_SYSTEM_PAGE: unknown system page".to_string(),
        )),
    }
}

fn save_slots_for_policy(policy: &SystemUiProfilePolicy) -> BTreeMap<String, SaveSlotViewModel> {
    policy
        .save_slot_ids
        .iter()
        .cloned()
        .map(|slot_id| {
            (
                slot_id.clone(),
                SaveSlotViewModel {
                    slot_id,
                    occupied: false,
                    thumbnail_asset: None,
                    has_thumbnail: false,
                    title_key: None,
                    timestamp_text: None,
                    playtime_text: None,
                    metadata_text: None,
                    can_write: true,
                    can_load: false,
                    migration_status: "empty".to_string(),
                },
            )
        })
        .collect()
}

fn format_playtime(playtime_ms: u64) -> String {
    let total_seconds = playtime_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn decode_save_envelope(bytes: &[u8]) -> Result<NativeVnPlayerSaveEnvelope, NativeVnHostError> {
    let envelope: NativeVnPlayerSaveEnvelope = postcard::from_bytes(bytes).map_err(|error| {
        NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
    })?;
    if envelope.schema != "astra.player.native_vn_save.v4"
        || envelope.payload.schema != "astra.player.native_vn_save_payload.v4"
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
    Ok(envelope)
}

fn validate_save_metadata(
    metadata: &NativeVnSaveMetadata,
    expected_slot: &str,
) -> Result<(), NativeVnHostError> {
    if metadata.slot_id != expected_slot
        || metadata.thumbnail_asset != format!("astra.internal.save_thumbnail.{expected_slot}")
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_METADATA_IDENTITY: save metadata identity does not match its slot"
                .into(),
        ));
    }
    if metadata.timestamp_text.trim().is_empty()
        || metadata.timestamp_text.len() > 64
        || metadata.playtime_text.len() != 8
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_METADATA_TEXT: save metadata text is invalid".into(),
        ));
    }
    let expected_bytes = usize::try_from(metadata.thumbnail.width)
        .ok()
        .and_then(|width| {
            usize::try_from(metadata.thumbnail.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if metadata.thumbnail.width != 160
        || metadata.thumbnail.height != 120
        || expected_bytes != Some(metadata.thumbnail.rgba8.len())
        || Hash256::from_sha256(&metadata.thumbnail.rgba8) != metadata.thumbnail.hash
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_METADATA_THUMBNAIL: save thumbnail integrity check failed".into(),
        ));
    }
    Ok(())
}

fn system_page_binding_key(page: SystemPageKind) -> Result<&'static str, NativeVnHostError> {
    match page {
        SystemPageKind::Title => Ok("title"),
        SystemPageKind::QuickPanel => Ok("quick_panel"),
        SystemPageKind::Save => Ok("save"),
        SystemPageKind::Load => Ok("load"),
        SystemPageKind::Config => Ok("config"),
        SystemPageKind::Gallery => Ok("gallery"),
        SystemPageKind::Replay => Ok("replay"),
        SystemPageKind::VoiceReplay => Ok("voice_replay"),
        SystemPageKind::RouteChart => Ok("route_chart"),
        SystemPageKind::Backlog => Ok("backlog"),
        SystemPageKind::LocalizationPreview => Ok("localization_preview"),
        SystemPageKind::Custom => Ok("custom"),
        SystemPageKind::Unknown => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_SYSTEM_PAGE: unknown system page has no UI binding".into(),
        )),
    }
}

fn ui_string_argument<'a>(
    action: &'a astra_ui_core::UiActionEnvelope,
    name: &str,
) -> Result<&'a str, NativeVnHostError> {
    match action.arguments.get(name) {
        Some(UiValue::String(value)) if !value.is_empty() => Ok(value),
        _ => Err(NativeVnHostError::Input(format!(
            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: action {} requires string argument {name}",
            action.action_id
        ))),
    }
}

fn ui_bool_argument(
    action: &astra_ui_core::UiActionEnvelope,
    name: &str,
) -> Result<bool, NativeVnHostError> {
    match action.arguments.get(name) {
        Some(UiValue::Bool(value)) => Ok(*value),
        _ => Err(NativeVnHostError::Input(format!(
            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: action {} requires boolean argument {name}",
            action.action_id
        ))),
    }
}

fn ui_value_to_scalar(value: &UiValue) -> Result<String, NativeVnHostError> {
    match value {
        UiValue::String(value) => Ok(value.clone()),
        UiValue::Bool(value) => Ok(value.to_string()),
        UiValue::Integer(value) => Ok(value.to_string()),
        UiValue::Number(value) if value.is_finite() => Ok(value.to_string()),
        _ => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: config value must be scalar".into(),
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
        let request_key = format!("preflight.{key}");
        provider.layout(&text_request(TextRequestSpec {
            key: &request_key,
            text,
            locale: &localization.locale,
            font_families,
            max_width: 4096,
            font_size: 28.0,
            max_lines: 16,
            direction: TextDirection::Auto,
        }))?;
    }
    Ok(())
}

fn validate_story_presentation(
    compiled: &CompiledStory,
    assets: &PackageAssetStore,
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
                    StageCommand::Preload { asset } => {
                        if !assets.contains_image(asset) && !assets.contains_media(asset) {
                            return Err(NativeVnHostError::Asset(format!(
                                "ASTRA_PLAYER_PRELOAD_ASSET_MISSING: {asset}"
                            )));
                        }
                    }
                    StageCommand::Background { asset: asset_id, .. }
                    | StageCommand::Show {
                        asset: asset_id, ..
                    } => {
                        if !assets.contains_image(asset_id) {
                            return Err(missing_texture(asset_id));
                        }
                    }
                    StageCommand::Configure { .. }
                    | StageCommand::Hide { .. }
                    | StageCommand::ClearLayer { .. }
                    | StageCommand::SetLayerVisibility { .. }
                    | StageCommand::Backdrop { .. }
                    | StageCommand::Shade { .. }
                    | StageCommand::SetSkipAllowed { .. }
                    | StageCommand::Move { .. }
                    | StageCommand::Audio(_)
                    | StageCommand::AudioControl(_)
                    | StageCommand::SetAudioBusEnabled { .. }
                    | StageCommand::Timeline(_)
                    | StageCommand::Transition { .. }
                    | StageCommand::Shake { .. } => {}
                    StageCommand::DeclareLayer { blend, .. } => {
                        stage_blend(*blend)?;
                    }
                    StageCommand::Camera { .. } => {}
                    StageCommand::Movie { fallback, .. } => {
                        if let Some(fallback) = fallback {
                            if !assets.contains_image(fallback) {
                                return Err(missing_texture(fallback));
                            }
                        }
                    }
                    StageCommand::Effect { filter, .. }
                        if matches!(
                            filter.as_str(),
                            "soft_glow" | "astra.filter.bloom" | "astra.filter.fade"
                        ) => {}
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

/// Returns the drawable portion of a previously submitted UI frame.
///
/// Resource lifetime commands are deliberately omitted: a cached UI frame is
/// only valid while the existing renderer residency remains live, and replaying
/// an `Upload*` command violates the retained-resource contract.
fn reusable_ui_draw_commands(draw: &[SceneCommand]) -> Vec<SceneCommand> {
    draw.iter()
        .filter(|command| {
            !matches!(
                command,
                SceneCommand::UploadTexture { .. }
                    | SceneCommand::UploadGlyph { .. }
                    | SceneCommand::ReleaseResource { .. }
            )
        })
        .cloned()
        .collect()
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
    #[error(transparent)]
    Ui(#[from] UiValidationError),
    #[error(transparent)]
    UiBinding(#[from] VnUiBindingError),
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
