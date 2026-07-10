use astra_core::Hash128;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageModel {
    pub schema: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub safe_area: SafeArea,
    pub frame_budget: FrameBudget,
    pub input_priority: Vec<InputPriority>,
    pub camera: CameraState,
    pub layers: Vec<LayerState>,
    #[serde(default)]
    pub video_layers: Vec<VideoLayerState>,
    #[serde(default)]
    pub audio_commands: Vec<AudioCommand>,
    pub text_windows: Vec<TextWindowState>,
    pub timelines: Vec<PresentationTimeline>,
    #[serde(default)]
    pub timeline_tasks: Vec<TimelineTaskState>,
}

impl StageModel {
    pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
        Self {
            schema: "astra.vn.stage_model.v1".to_string(),
            viewport_width,
            viewport_height,
            safe_area: SafeArea::default(),
            frame_budget: FrameBudget::default(),
            input_priority: Vec::new(),
            camera: CameraState::identity(),
            layers: Vec::new(),
            video_layers: Vec::new(),
            audio_commands: Vec::new(),
            text_windows: Vec::new(),
            timelines: Vec::new(),
            timeline_tasks: Vec::new(),
        }
    }

    pub fn apply(&mut self, command: StandardPresentationCommand) {
        tracing::trace!(
            event = "vn.presentation.command.apply",
            layer_count = self.layers.len(),
            timeline_count = self.timelines.len(),
            "AstraVN presentation command applied"
        );
        match command {
            StandardPresentationCommand::SetCamera(camera) => self.camera = camera,
            StandardPresentationCommand::ShowLayer {
                id,
                kind,
                asset,
                z,
                x,
                y,
            } => {
                let layer = LayerState {
                    id: id.clone(),
                    kind,
                    z,
                    visible: true,
                    asset: Some(asset),
                    x,
                    y,
                    opacity: 1.0,
                    blend: LayerBlend::Alpha,
                    clip: None,
                    mask_asset: None,
                    pose: None,
                    anchor: [0.5, 1.0],
                    transform: LayerTransform::default(),
                };
                if let Some(existing) = self.layers.iter_mut().find(|layer| layer.id == id) {
                    *existing = layer;
                } else {
                    self.layers.push(layer);
                }
                self.layers
                    .sort_by(|left, right| left.z.cmp(&right.z).then(left.id.cmp(&right.id)));
            }
            StandardPresentationCommand::HideLayer { id } => {
                if let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) {
                    layer.visible = false;
                }
            }
            StandardPresentationCommand::SetTextWindow {
                id,
                x,
                y,
                width,
                height,
            } => {
                let window = TextWindowState {
                    id: id.clone(),
                    x,
                    y,
                    width,
                    height,
                    visible: true,
                    layout: TextLayoutState::default(),
                    input_priority: 0,
                };
                if let Some(existing) = self.text_windows.iter_mut().find(|window| window.id == id)
                {
                    *existing = window;
                } else {
                    self.text_windows.push(window);
                }
                self.text_windows
                    .sort_by(|left, right| left.id.cmp(&right.id));
            }
            StandardPresentationCommand::SetVideo(video) => {
                let layer = LayerState {
                    id: video.layer_id.clone(),
                    kind: LayerKind::Movie,
                    z: video.z,
                    visible: true,
                    asset: Some(video.movie.clone()),
                    x: 0.0,
                    y: 0.0,
                    opacity: video.alpha,
                    blend: LayerBlend::Alpha,
                    clip: None,
                    mask_asset: None,
                    pose: None,
                    anchor: [0.5, 0.5],
                    transform: LayerTransform::default(),
                };
                self.upsert_layer(layer);
                if let Some(existing) = self
                    .video_layers
                    .iter_mut()
                    .find(|layer| layer.layer_id == video.layer_id)
                {
                    *existing = video;
                } else {
                    self.video_layers.push(video);
                }
                self.video_layers
                    .sort_by(|left, right| left.layer_id.cmp(&right.layer_id));
            }
            StandardPresentationCommand::PlayAudio(command) => {
                if let Some(existing) = self
                    .audio_commands
                    .iter_mut()
                    .find(|audio| audio.id == command.id)
                {
                    *existing = command;
                } else {
                    self.audio_commands.push(command);
                }
                self.audio_commands
                    .sort_by(|left, right| left.bus.cmp(&right.bus).then(left.id.cmp(&right.id)));
            }
            StandardPresentationCommand::RunTimeline(timeline) => {
                if timeline.join_policy == TimelineJoinPolicy::ReplaceTarget {
                    self.cancel_conflicting_timelines(&timeline, "replace_target");
                }
                self.timelines.push(timeline.clone());
                self.timeline_tasks.push(TimelineTaskState {
                    id: timeline.id.clone(),
                    timeline,
                    status: TimelineTaskStatus::Running,
                    cancel_reason: None,
                });
                self.timelines.sort_by(|left, right| left.id.cmp(&right.id));
                self.timeline_tasks
                    .sort_by(|left, right| left.id.cmp(&right.id));
            }
            StandardPresentationCommand::CancelTimeline { id, reason } => {
                self.set_timeline_status(&id, TimelineTaskStatus::Canceled, Some(reason));
                self.timelines.retain(|timeline| timeline.id != id);
            }
            StandardPresentationCommand::CompleteTimeline { id } => {
                self.set_timeline_status(&id, TimelineTaskStatus::Completed, None);
                self.timelines.retain(|timeline| timeline.id != id);
            }
        }
    }

    pub fn presentation_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(self).expect("stage model must serialize for hashing"),
        )
    }

    fn upsert_layer(&mut self, layer: LayerState) {
        if let Some(existing) = self
            .layers
            .iter_mut()
            .find(|existing| existing.id == layer.id)
        {
            *existing = layer;
        } else {
            self.layers.push(layer);
        }
        self.layers
            .sort_by(|left, right| left.z.cmp(&right.z).then(left.id.cmp(&right.id)));
    }

    fn cancel_conflicting_timelines(&mut self, timeline: &PresentationTimeline, reason: &str) {
        let targets = timeline.targets();
        let conflicts: Vec<_> = self
            .timelines
            .iter()
            .filter(|running| {
                running
                    .tracks
                    .iter()
                    .any(|track| targets.contains(&track.target))
            })
            .map(|running| running.id.clone())
            .collect();
        for id in &conflicts {
            self.set_timeline_status(id, TimelineTaskStatus::Canceled, Some(reason.to_string()));
        }
        self.timelines
            .retain(|running| !conflicts.iter().any(|id| id == &running.id));
    }

    fn set_timeline_status(
        &mut self,
        id: &str,
        status: TimelineTaskStatus,
        cancel_reason: Option<String>,
    ) {
        if let Some(task) = self.timeline_tasks.iter_mut().find(|task| task.id == id) {
            task.status = status;
            task.cancel_reason = cancel_reason;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SafeArea {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FrameBudget {
    pub max_draw_commands: u32,
    pub max_filter_nodes: u32,
    pub max_frame_time_us: u32,
}

impl Default for FrameBudget {
    fn default() -> Self {
        Self {
            max_draw_commands: 4096,
            max_filter_nodes: 64,
            max_frame_time_us: 16_667,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputPriority {
    pub layer_id: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraState {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
    pub rotation: f32,
}

impl CameraState {
    pub fn identity() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            rotation: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayerState {
    pub id: String,
    pub kind: LayerKind,
    pub z: i32,
    pub visible: bool,
    pub asset: Option<String>,
    pub x: f32,
    pub y: f32,
    pub opacity: f32,
    pub blend: LayerBlend,
    pub clip: Option<LayerClip>,
    pub mask_asset: Option<String>,
    pub pose: Option<String>,
    pub anchor: [f32; 2],
    pub transform: LayerTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayerBlend {
    Alpha,
    Add,
    Multiply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LayerClip {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayerTransform {
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_degrees: f32,
}

impl Default for LayerTransform {
    fn default() -> Self {
        Self {
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    Background,
    Character,
    Cg,
    Ui,
    TextWindow,
    Movie,
    Effect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextWindowState {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub visible: bool,
    pub layout: TextLayoutState,
    pub input_priority: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutState {
    pub font_asset: String,
    pub font_size: f32,
    pub line_height: f32,
    pub alignment: TextAlignment,
}

impl Default for TextLayoutState {
    fn default() -> Self {
        Self {
            font_asset: "asset:/font/ui".into(),
            font_size: 32.0,
            line_height: 1.25,
            alignment: TextAlignment::Start,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VideoLayerState {
    pub layer_id: String,
    pub movie: String,
    pub alpha: f32,
    pub loop_mode: VideoLoopMode,
    pub end_behavior: MovieEndBehavior,
    pub fallback_frame: Option<String>,
    pub z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoLoopMode {
    Once,
    Loop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MovieEndBehavior {
    Continue,
    Wait,
    HoldLastFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioCommand {
    pub id: String,
    pub bus: AudioBus,
    pub asset: String,
    pub loop_mode: AudioLoopMode,
    pub fade_ms: u32,
    pub sync: AudioSync,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AudioBus {
    Voice,
    Bgm,
    Se,
    Movie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioLoopMode {
    Once,
    Loop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioSync {
    None,
    Text,
    Fence(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum StandardPresentationCommand {
    SetCamera(CameraState),
    ShowLayer {
        id: String,
        kind: LayerKind,
        asset: String,
        z: i32,
        x: f32,
        y: f32,
    },
    HideLayer {
        id: String,
    },
    SetTextWindow {
        id: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    SetVideo(VideoLayerState),
    PlayAudio(AudioCommand),
    RunTimeline(PresentationTimeline),
    CancelTimeline {
        id: String,
        reason: String,
    },
    CompleteTimeline {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationTimeline {
    pub id: String,
    pub join_policy: TimelineJoinPolicy,
    pub tracks: Vec<TimelineTrack>,
}

impl PresentationTimeline {
    pub fn stable_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(self).expect("presentation timeline must serialize for hashing"),
        )
    }

    fn targets(&self) -> Vec<String> {
        let mut targets = self
            .tracks
            .iter()
            .map(|track| track.target.clone())
            .collect::<Vec<_>>();
        targets.sort();
        targets.dedup();
        targets
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimelineJoinPolicy {
    FireAndForget,
    BlockUntilComplete,
    ReplaceTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineTrack {
    pub target: String,
    pub property: String,
    pub keyframes: Vec<TimelineKeyframe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineKeyframe {
    pub time_ms: u32,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineTaskState {
    pub id: String,
    pub timeline: PresentationTimeline,
    pub status: TimelineTaskStatus,
    pub cancel_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimelineTaskStatus {
    Running,
    Completed,
    Canceled,
}
