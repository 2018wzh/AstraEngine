use super::*;

pub const PRODUCT_STAGE_STATE_SCHEMA: &str = "astra.vn.product_stage_state.v11";

pub(super) const MAX_FRAME_DELTA_NS: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageState {
    pub schema: String,
    pub profile: String,
    pub configured: bool,
    pub viewport: StageViewport,
    pub safe_area: AspectRatio,
    pub preloaded_assets: BTreeSet<String>,
    pub layers: BTreeMap<String, ProductStageLayer>,
    pub entities: BTreeMap<String, ProductStageEntity>,
    pub camera: ProductStageCamera,
    pub movies: BTreeMap<String, ProductStageMovie>,
    pub effects: BTreeMap<String, ProductStageEffect>,
    pub backdrop_color: Option<[u8; 4]>,
    pub shade_color: [u8; 4],
    pub shade_opacity: FixedScalar,
    pub skip_allowed: bool,
    pub transition: Option<ProductStageTransition>,
    pub audio_bus_enabled: BTreeMap<VnAudioBus, bool>,
    pub frame_index: u64,
    pub elapsed_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageLayer {
    pub id: String,
    pub kind: StageLayerKind,
    pub z: i32,
    pub blend: StageBlendMode,
    pub clip: Option<StageClipPolicy>,
    pub input: Option<String>,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageEntity {
    pub id: String,
    pub layer: String,
    pub asset: String,
    pub pose: Option<String>,
    pub fit: StageFitMode,
    pub x: FixedScalar,
    pub y: FixedScalar,
    pub opacity: FixedScalar,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageCamera {
    pub target: String,
    pub x: FixedScalar,
    pub y: FixedScalar,
    pub zoom: FixedScalar,
    pub rotation: FixedScalar,
    pub shake_x: FixedScalar,
    pub shake_y: FixedScalar,
}

impl Default for ProductStageCamera {
    fn default() -> Self {
        Self {
            target: "main".to_string(),
            x: FixedScalar::ZERO,
            y: FixedScalar::ZERO,
            zoom: FixedScalar::ONE,
            rotation: FixedScalar::ZERO,
            shake_x: FixedScalar::ZERO,
            shake_y: FixedScalar::ZERO,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageMovie {
    pub layer: String,
    pub asset: String,
    pub alpha: FixedScalar,
    pub loop_mode: MovieLoopMode,
    pub end: VnMovieEndBehavior,
    pub fence: Option<String>,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageEffect {
    pub target: String,
    pub lip_sync: bool,
    pub filter: String,
    pub fallback: String,
    pub budget_us: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductStageTransition {
    pub preset: String,
    pub descriptor_id: Option<String>,
    pub filter: Option<String>,
    pub progress: FixedScalar,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageDirectorOutput {
    Preload { asset: String },
    Audio(AudioCue),
    AudioControl(AudioControl),
    AudioBusEnabled { bus: VnAudioBus, enabled: bool },
    Movie(ProductStageMovie),
    Effect(ProductStageEffect),
    FenceCompleted { kind: String, id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductStageDirector {
    pub(super) manifest: VnPresentationProviderManifest,
    pub(super) state: ProductStageState,
    pub(super) tweens: Vec<StageTween>,
    pub(super) timelines: BTreeMap<String, ActiveTimeline>,
    pub(super) settled_timelines: BTreeSet<String>,
    pub(super) shake: Option<ActiveShake>,
    pub(super) coordinator: PresentationCoordinator,
    pub(super) queued_region_commands: BTreeMap<String, StageCommand>,
    pub(super) queued_stage_commands: VecDeque<StageCommand>,
    pub(super) next_region_sequence: u64,
    pub(super) failed: bool,
}
