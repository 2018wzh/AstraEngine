mod preview;
pub use preview::NativeVnPreview;
mod execution;
pub use execution::NativeVnRuntimeExecution;
mod media_scope;
mod presentation;
mod product_save;
mod runtime;
mod save_catalog;
mod ui_clip;
mod viewport;
use runtime::NativeVnRuntimeHost;
mod audio;
mod error;
mod lifecycle;
mod open;
mod query;
mod query_media;
mod save_ops;
mod scene;
mod stage;
mod textures;
mod ui_effects;
mod ui_frame;
mod ui_input;
mod ui_layout;
pub use error::NativeVnHostError;
use ui_layout::*;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use unicode_segmentation::UnicodeSegmentation;

use astra_core::Hash256;
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
    PlayerDecodeKind, PlayerDecodeLifecyclePlan, PlayerHostCommand, PlayerHostCommandBatch,
    PlayerHostCommandError, PlayerHostResourceId, PlayerSaveTransactionPlan, PlayerTimelineTask,
    PlayerTimelineTaskAction,
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
use astra_vn::{NativeVnStepCommand, NativeVnStepInput, NativeVnStepOutput};
use astra_vn::{VnHostError, VnHostLimits};
#[cfg(test)]
use astra_vn_core::SystemActionEffect;
use astra_vn_core::{
    CompiledCommand, CompiledStory, MovieLoopMode, PresentationCommand, ReadingMode,
    SaveCompletionPolicy, StageBlendMode, StageClipPolicy, StageCommand, StageFitMode,
    StageLayerKind, State, SystemPageKind, SystemUiProfilePolicy, TimelineCommand, VnAudioBus,
    VnAudioControlAction, VnPlayerCommand, VnRunConfig, VnRuntimeState, VnWaitKind,
    VN_RUNTIME_STATE_SCHEMA,
};
use astra_vn_package::{
    decode_compiled_project, load_localization as load_package_localization,
    load_player_locale_config, load_presentation_provider_manifest, CompiledVnProject,
    PresentationInterruptPolicy, ProductStageDirector, ProductStageState, StageDirectorOutput,
    TextAdvanceDisposition, VnLocalizationTable, VnPresentationProviderManifest,
    VnSystemUiProfileManifest,
};
use astra_vn_policy::LuauUiControllerHost;
use astra_vn_ui::{
    resolve_binding, SaveSlotViewModel, VnUiAction, VnUiBindingError, VnUiBindingRequest,
    VnUiControllerEffect, VnUiControllerUpdate, VnUiModelContext, VnUiSessionState,
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
    presentation_failed: bool,
    compiled_project_hash: Hash256,
    media_scope: astra_runtime::TaskScope,
    video_scopes: BTreeMap<String, astra_runtime::TaskScope>,
    host: NativeVnRuntimeHost,
    session_id: String,
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
    next_step_mode: astra_runtime::TickMode,
    width: u32,
    height: u32,
    output_extent: astra_media_core::Extent2D,
    ui_viewport: UiViewport,
    textures: BTreeMap<String, TextureFrame>,
    texture_dimensions: BTreeMap<String, (u32, u32)>,
    asset_store: Arc<PackageAssetStore>,
    image_prefetcher: PackageImagePrefetcher,
    image_prefetch_windows: BTreeMap<String, Vec<String>>,
    image_prefetch_inflight: BTreeSet<String>,
    image_prefetch_failure: Option<String>,
    live_texture_ids: BTreeSet<String>,
    gpu_resources_invalidated: bool,
    live_texture_bytes: BTreeMap<String, u64>,
    pending_gpu_lifecycle: Vec<SceneCommand>,
    texture_last_used: BTreeMap<String, u64>,
    texture_cpu_last_used: BTreeMap<String, u64>,
    texture_use_clock: u64,
    texture_cpu_bytes: u64,
    texture_cpu_budget_bytes: u64,
    resident_texture_bytes: u64,
    gpu_texture_budget_bytes: u64,
    live_layout_ids: BTreeSet<String>,
    scene_draw: Vec<SceneCommand>,
    director_transition_snapshot: Option<DirectorTransitionSnapshot>,
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
    story: Arc<CompiledStory>,
    ui_blueprints: astra_ui_core::UiBlueprintBundle,
    ui_view_localization_keys: BTreeMap<String, BTreeSet<String>>,
    ui_bindings: astra_ui_core::UiBindingManifest,
    ui_backend: AstraYakuiBackend<BlueprintYakuiRenderer>,
    ui_backend_started: bool,
    ui_themes: BTreeMap<String, Arc<UiThemeManifest>>,
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

#[derive(Debug, Clone)]
struct DirectorTransitionSnapshot {
    descriptor_id: String,
    source_state: ProductStageState,
    source_draw: Vec<SceneCommand>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SavedDirectorTransitionSnapshot {
    descriptor_id: String,
    source_state: ProductStageState,
}

#[derive(Clone, PartialEq)]
struct NativeVnUiFrameReuseKey {
    session_id: String,
    generation: u64,
    instance_id: String,
    viewport: UiViewport,
    theme_revision: u64,
    model_schema: String,
    model_revision: u64,
    text_reveal_progress: Option<(u32, u32)>,
}

impl NativeVnUiFrameReuseKey {
    // `fixed_time_ns` is intentionally excluded. A time-driven view must return
    // `repaint_after_ns`; only outputs that explicitly declare no repaint are
    // admitted to this cache. Controller animation progress advances the
    // explicit model revision.
    fn from_request(
        request: &UiFrameRequest,
        instance_id: &str,
        text_reveal_progress: Option<(u32, u32)>,
    ) -> Self {
        Self {
            session_id: request.session_id.clone(),
            generation: request.generation,
            instance_id: instance_id.to_string(),
            viewport: request.viewport.clone(),
            theme_revision: request.theme.revision,
            model_schema: request.model_schema.clone(),
            model_revision: request.model_revision,
            text_reveal_progress,
        }
    }

    fn matches(
        &self,
        request: &UiFrameRequest,
        instance_id: &str,
        text_reveal_progress: Option<(u32, u32)>,
    ) -> bool {
        self.session_id == request.session_id
            && self.generation == request.generation
            && self.instance_id == instance_id
            && self.viewport == request.viewport
            && self.theme_revision == request.theme.revision
            && self.model_schema == request.model_schema
            && self.model_revision == request.model_revision
            && self.text_reveal_progress == text_reveal_progress
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
    /// Authored audio identity. Completion fences and `audio_control` address
    /// this identity, never the compiler-generated command identity.
    pub target_id: String,
    pub command: String,
    pub attributes: BTreeMap<String, String>,
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: astra_byte_source::OwnedByteBuffer,
    pub encoded_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnAudioPreloadRequest {
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: astra_byte_source::OwnedByteBuffer,
    pub encoded_length: u64,
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
    pub(crate) scope: astra_runtime::TaskScope,
    pub layer: String,
    pub asset_id: String,
    pub codec: String,
    pub encoded_bytes: astra_byte_source::OwnedByteBuffer,
    pub encoded_length: u64,
    pub alpha_millionths: i64,
    pub looping: bool,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnStepEvidence {
    pub schema: String,
    pub fixed_step: u64,
    pub coverage_reached: std::collections::BTreeSet<String>,
    pub presentation_count: usize,
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
    /// True when no text is revealing or the current text is fully visible.
    pub text_reveal_complete: bool,
    pub backlog_count: usize,
    pub occupied_save_slot_count: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NativeVnPlayerSavePayload {
    schema: String,
    slot: String,
    session_id: String,
    runtime: astra_runtime::SaveBlob,
    stage_director: ProductStageDirector,
    director_transition_snapshot: Option<SavedDirectorTransitionSnapshot>,
    step_evidence: NativeVnStepEvidence,
    product_media_snapshot_json: Option<Vec<u8>>,
    save_metadata: SavedNativeVnSaveMetadata,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NativeVnPlayerSaveEnvelope {
    schema: String,
    payload: NativeVnPlayerSavePayload,
}

#[derive(Debug, Clone)]
struct NativeVnSaveMetadata {
    slot_id: String,
    thumbnail_asset: String,
    thumbnail: TextureFrame,
    timestamp_text: String,
    playtime_text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SavedNativeVnSaveMetadata {
    slot_id: String,
    thumbnail_asset: String,
    thumbnail_width: u32,
    thumbnail_height: u32,
    thumbnail_rgba8: Vec<u8>,
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
    product_context: astra_package::PackageProductContext,
    package_hash: Hash256,
    presentation: ProductPresentationBinding,
    system_ui_policy: SystemUiProfilePolicy,
    policy_actions: crate::PreparedPolicyActions,
}

struct NativeVnHostCacheBudget {
    asset_bytes: u64,
    glyph_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnHostOpenOptions {
    pub max_asset_cache_bytes: u64,
    pub max_glyph_cache_bytes: u64,
    pub runtime_execution: NativeVnRuntimeExecution,
    pub policy_actions: crate::PreparedPolicyActions,
}

struct NativeVnHostRuntimeOptions {
    cache_budget: NativeVnHostCacheBudget,
    execution: NativeVnRuntimeExecution,
}

#[cfg(test)]
mod host_tests;
