use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, Hash128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AspectRatio, AudioControl, AudioCue, FixedScalar, MovieLoopMode, StageBlendMode,
    StageClipPolicy, StageCommand, StageFitMode, StageLayerKind, StagePlacement, StageViewport,
    TimelineCommand, TimelineSpec, VnAudioBus, VnError, VnMovieEndBehavior, VnPresentationEasing,
    VnPresentationProviderManifest, VnTimelineJoinPolicy,
};

pub const PRODUCT_STAGE_STATE_SCHEMA: &str = "astra.vn.product_stage_state.v6";
const MAX_FRAME_DELTA_NS: u64 = 1_000_000_000;

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

impl ProductStageState {
    pub fn stable_hash(&self) -> Result<Hash128, VnError> {
        Ok(Hash128::from_blake3(&postcard::to_allocvec(self)?))
    }
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
    manifest: VnPresentationProviderManifest,
    state: ProductStageState,
    tweens: Vec<StageTween>,
    timelines: BTreeMap<String, ActiveTimeline>,
    completed_timelines: BTreeSet<String>,
    shake: Option<ActiveShake>,
}

impl ProductStageDirector {
    pub fn new(
        manifest: VnPresentationProviderManifest,
        profile: impl Into<String>,
        viewport: StageViewport,
    ) -> Result<Self, VnError> {
        let profile = profile.into();
        manifest.profile(&profile).map_err(VnError::Diagnostic)?;
        validate_viewport(viewport)?;
        Ok(Self {
            manifest,
            state: ProductStageState {
                schema: PRODUCT_STAGE_STATE_SCHEMA.to_string(),
                profile,
                configured: false,
                viewport,
                safe_area: AspectRatio {
                    width: 16,
                    height: 9,
                },
                preloaded_assets: BTreeSet::new(),
                layers: BTreeMap::new(),
                entities: BTreeMap::new(),
                camera: ProductStageCamera::default(),
                movies: BTreeMap::new(),
                effects: BTreeMap::new(),
                backdrop_color: None,
                shade_color: [0, 0, 0, 255],
                shade_opacity: FixedScalar::ZERO,
                skip_allowed: true,
                transition: None,
                audio_bus_enabled: BTreeMap::from([
                    (VnAudioBus::Bgm, true),
                    (VnAudioBus::Se, true),
                ]),
                frame_index: 0,
                elapsed_ns: 0,
            },
            tweens: Vec::new(),
            timelines: BTreeMap::new(),
            completed_timelines: BTreeSet::new(),
            shake: None,
        })
    }

    pub fn state(&self) -> &ProductStageState {
        &self.state
    }

    pub fn resize_viewport(&mut self, viewport: StageViewport) -> Result<(), VnError> {
        validate_viewport(viewport)?;
        let mut next = self.clone();
        next.state.viewport = viewport;
        next.validate_state()?;
        *self = next;
        Ok(())
    }

    pub fn active_timeline_count(&self) -> usize {
        self.timelines.len()
    }

    pub fn requires_frame_tick(&self) -> bool {
        !self.tweens.is_empty() || !self.timelines.is_empty() || self.shake.is_some()
    }

    pub fn apply(&mut self, command: &StageCommand) -> Result<Vec<StageDirectorOutput>, VnError> {
        let (next, mut outputs) = self.prepare_batch(std::iter::once(command))?;
        *self = next;
        Ok(outputs
            .pop()
            .expect("single command produces one output group"))
    }

    pub fn prepare_batch<'a>(
        &self,
        commands: impl IntoIterator<Item = &'a StageCommand>,
    ) -> Result<(Self, Vec<Vec<StageDirectorOutput>>), VnError> {
        let mut next = self.clone();
        let mut outputs = Vec::new();
        for command in commands {
            outputs.push(next.apply_inner(command)?);
        }
        next.validate_state()?;
        Ok((next, outputs))
    }

    pub fn tick(&mut self, delta_ns: u64) -> Result<Vec<StageDirectorOutput>, VnError> {
        if delta_ns == 0 || delta_ns > MAX_FRAME_DELTA_NS {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TICK_DELTA",
                "presentation frame delta is outside the fixed-step budget",
            ));
        }
        let mut next = self.clone();
        let output = next.tick_inner(delta_ns)?;
        *self = next;
        Ok(output)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, VnError> {
        postcard::to_allocvec(self).map_err(Into::into)
    }

    pub fn restore(
        manifest: VnPresentationProviderManifest,
        profile: &str,
        bytes: &[u8],
    ) -> Result<Self, VnError> {
        let restored: Self = postcard::from_bytes(bytes)?;
        manifest.profile(profile).map_err(VnError::Diagnostic)?;
        if restored.state.schema != PRODUCT_STAGE_STATE_SCHEMA
            || restored.state.profile != profile
            || restored.manifest != manifest
        {
            return Err(stage_error(
                "ASTRA_VN_STAGE_SNAPSHOT_IDENTITY",
                "presentation snapshot identity does not match the package binding",
            ));
        }
        restored.validate_state()?;
        Ok(restored)
    }

    fn apply_inner(&mut self, command: &StageCommand) -> Result<Vec<StageDirectorOutput>, VnError> {
        tracing::trace!(
            event = "vn.presentation.director.apply",
            command = command.kind(),
            frame_index = self.state.frame_index,
            "typed presentation command entered the product stage director"
        );
        match command {
            StageCommand::Preload { asset } => {
                if !self.state.preloaded_assets.insert(asset.clone()) {
                    return Ok(Vec::new());
                }
                return Ok(vec![StageDirectorOutput::Preload {
                    asset: asset.clone(),
                }]);
            }
            StageCommand::Configure {
                viewport,
                safe_area,
            } => {
                validate_viewport(*viewport)?;
                if safe_area.width == 0 || safe_area.height == 0 {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_SAFE_AREA",
                        "stage safe area ratio must be non-zero",
                    ));
                }
                if self.state.configured
                    && (self.state.viewport != *viewport || self.state.safe_area != *safe_area)
                {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_RECONFIGURE",
                        "a configured stage cannot change authority without a resize transaction",
                    ));
                }
                self.state.viewport = *viewport;
                self.state.safe_area = *safe_area;
                self.state.configured = true;
            }
            StageCommand::DeclareLayer {
                id,
                kind,
                z,
                blend,
                clip,
                input,
            } => {
                self.require_configured()?;
                if self.state.layers.contains_key(id) {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_LAYER_DUPLICATE",
                        "stage layer id is already declared",
                    ));
                }
                let profile = self.profile()?;
                if self.state.layers.len() >= profile.max_layers as usize {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_LAYER_BUDGET",
                        "stage layer count exceeds the selected profile budget",
                    ));
                }
                self.state.layers.insert(
                    id.clone(),
                    ProductStageLayer {
                        id: id.clone(),
                        kind: *kind,
                        z: *z,
                        blend: *blend,
                        clip: *clip,
                        input: input.clone(),
                        visible: true,
                    },
                );
            }
            StageCommand::Background {
                asset,
                layer,
                preset,
                duration_ms,
            } => {
                self.require_layer(layer, &[StageLayerKind::Background])?;
                let duration =
                    self.resolve_duration(command.kind(), preset.as_deref(), *duration_ms)?;
                let id = format!("background.{layer}");
                self.state.entities.insert(
                    id.clone(),
                    ProductStageEntity {
                        id: id.clone(),
                        layer: layer.clone(),
                        asset: asset.clone(),
                        pose: None,
                        fit: StageFitMode::ContainHeight,
                        x: FixedScalar::ZERO,
                        y: FixedScalar::ZERO,
                        opacity: if duration == 0 {
                            FixedScalar::ONE
                        } else {
                            FixedScalar::ZERO
                        },
                        visible: true,
                    },
                );
                if duration > 0 {
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id),
                        TweenProperty::Opacity,
                        FixedScalar::ZERO,
                        FixedScalar::ONE,
                        duration,
                        self.resolve_easing(command.kind(), preset.as_deref())?,
                        false,
                    )?);
                }
            }
            StageCommand::Show {
                id,
                asset,
                pose,
                layer,
                placement,
                fit,
                opacity,
                preset,
            } => {
                self.require_layer(
                    layer,
                    &[
                        StageLayerKind::Sprite,
                        StageLayerKind::Cg,
                        StageLayerKind::Ui,
                    ],
                )?;
                let duration = self.resolve_duration(command.kind(), preset.as_deref(), 0)?;
                let x = placement_x(*placement, self.state.viewport.width)?;
                let y = FixedScalar {
                    millionths: i64::from(self.state.viewport.height) * 1_000_000,
                };
                self.state.entities.insert(
                    id.clone(),
                    ProductStageEntity {
                        id: id.clone(),
                        layer: layer.clone(),
                        asset: asset.clone(),
                        pose: pose.clone(),
                        fit: *fit,
                        x,
                        y,
                        opacity: if duration == 0 {
                            *opacity
                        } else {
                            FixedScalar::ZERO
                        },
                        visible: true,
                    },
                );
                if duration > 0 {
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id.clone()),
                        TweenProperty::Opacity,
                        FixedScalar::ZERO,
                        *opacity,
                        duration,
                        self.resolve_easing(command.kind(), preset.as_deref())?,
                        false,
                    )?);
                }
            }
            StageCommand::Hide {
                id,
                preset,
                duration_ms,
            } => {
                let entity = self.entity(id)?.clone();
                let duration =
                    self.resolve_duration(command.kind(), preset.as_deref(), *duration_ms)?;
                if duration == 0 {
                    self.state.entities.remove(id);
                    self.cancel_target(&TweenTarget::Entity(id.clone()));
                } else {
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id.clone()),
                        TweenProperty::Opacity,
                        entity.opacity,
                        FixedScalar::ZERO,
                        duration,
                        self.resolve_easing(command.kind(), preset.as_deref())?,
                        true,
                    )?);
                }
            }
            StageCommand::ClearLayer { layer, duration_ms } => {
                self.require_layer(
                    layer,
                    &[
                        StageLayerKind::Background,
                        StageLayerKind::Sprite,
                        StageLayerKind::Cg,
                        StageLayerKind::Video,
                        StageLayerKind::Ui,
                    ],
                )?;
                let entities = self
                    .state
                    .entities
                    .values()
                    .filter(|entity| entity.layer == *layer)
                    .map(|entity| (entity.id.clone(), entity.opacity))
                    .collect::<Vec<_>>();
                if *duration_ms == 0 {
                    for (id, _) in entities {
                        self.state.entities.remove(&id);
                        self.cancel_target(&TweenTarget::Entity(id));
                    }
                } else {
                    for (id, opacity) in entities {
                        self.schedule_tween(StageTween::new(
                            TweenTarget::Entity(id),
                            TweenProperty::Opacity,
                            opacity,
                            FixedScalar::ZERO,
                            *duration_ms,
                            VnPresentationEasing::Linear,
                            true,
                        )?);
                    }
                }
            }
            StageCommand::SetLayerVisibility { layer, visible } => {
                let layer = self.state.layers.get_mut(layer).ok_or_else(|| {
                    stage_error(
                        "ASTRA_VN_STAGE_LAYER_UNKNOWN",
                        "layer visibility references an undeclared layer",
                    )
                })?;
                layer.visible = *visible;
            }
            StageCommand::Backdrop { color } => {
                self.state.backdrop_color = (color[3] != 0).then_some(*color);
            }
            StageCommand::Shade { color, opacity } => {
                if color[3] != 255 {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_SHADE_COLOR_ALPHA",
                        "stage shade color must be opaque; opacity owns coverage",
                    ));
                }
                if !(0..=1_000_000).contains(&opacity.millionths) {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_SHADE_RANGE",
                        "stage shade opacity must be between zero and one",
                    ));
                }
                self.state.shade_color = *color;
                self.state.shade_opacity = *opacity;
            }
            StageCommand::SetSkipAllowed { allowed } => {
                self.state.skip_allowed = *allowed;
            }
            StageCommand::Move {
                id,
                x,
                y,
                duration_ms,
                preset,
            } => {
                let entity = self.entity(id)?.clone();
                let duration =
                    self.resolve_duration(command.kind(), preset.as_deref(), *duration_ms)?;
                if duration == 0 {
                    let entity = self.entity_mut(id)?;
                    entity.x = *x;
                    entity.y = *y;
                } else {
                    let easing = self.resolve_easing(command.kind(), preset.as_deref())?;
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id.clone()),
                        TweenProperty::X,
                        entity.x,
                        *x,
                        duration,
                        easing,
                        false,
                    )?);
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id.clone()),
                        TweenProperty::Y,
                        entity.y,
                        *y,
                        duration,
                        easing,
                        false,
                    )?);
                }
            }
            StageCommand::Camera {
                target,
                x,
                y,
                zoom,
                rotation,
                duration_ms,
                preset,
            } => {
                if zoom.millionths <= 0 {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_CAMERA_ZOOM",
                        "camera zoom must remain positive",
                    ));
                }
                let duration =
                    self.resolve_duration(command.kind(), preset.as_deref(), *duration_ms)?;
                let previous = self.state.camera.clone();
                self.state.camera.target = target.clone();
                if duration == 0 {
                    self.state.camera.x = *x;
                    self.state.camera.y = *y;
                    self.state.camera.zoom = *zoom;
                    self.state.camera.rotation = *rotation;
                } else {
                    let easing = self.resolve_easing(command.kind(), preset.as_deref())?;
                    for (property, start, end) in [
                        (TweenProperty::X, previous.x, *x),
                        (TweenProperty::Y, previous.y, *y),
                        (TweenProperty::Zoom, previous.zoom, *zoom),
                        (TweenProperty::Rotation, previous.rotation, *rotation),
                    ] {
                        self.schedule_tween(StageTween::new(
                            TweenTarget::Camera,
                            property,
                            start,
                            end,
                            duration,
                            easing,
                            false,
                        )?);
                    }
                }
            }
            StageCommand::Movie {
                layer,
                asset,
                alpha,
                loop_mode,
                end,
                fence,
                fallback,
            } => {
                self.require_layer(layer, &[StageLayerKind::Video])?;
                let movie = ProductStageMovie {
                    layer: layer.clone(),
                    asset: asset.clone(),
                    alpha: *alpha,
                    loop_mode: *loop_mode,
                    end: *end,
                    fence: fence.clone(),
                    fallback: fallback.clone(),
                };
                self.state.movies.insert(layer.clone(), movie.clone());
                return Ok(vec![StageDirectorOutput::Movie(movie)]);
            }
            StageCommand::Audio(cue) => {
                return Ok(vec![StageDirectorOutput::Audio(cue.clone())]);
            }
            StageCommand::AudioControl(control) => {
                return Ok(vec![StageDirectorOutput::AudioControl(control.clone())]);
            }
            StageCommand::Transition {
                preset,
                duration_ms,
            } => {
                let resolved = self
                    .manifest
                    .resolve_preset(&self.state.profile, command.kind(), preset)
                    .map_err(VnError::Diagnostic)?;
                let duration = if *duration_ms == 0 {
                    resolved.duration_ms
                } else {
                    *duration_ms
                };
                self.state.transition = Some(ProductStageTransition {
                    preset: preset.clone(),
                    filter: resolved.filter.clone(),
                    progress: FixedScalar::ZERO,
                    duration_ms: duration,
                });
                self.schedule_tween(StageTween::new(
                    TweenTarget::Transition,
                    TweenProperty::Progress,
                    FixedScalar::ZERO,
                    FixedScalar::ONE,
                    duration,
                    resolved.easing,
                    false,
                )?);
            }
            StageCommand::Shake {
                target,
                strength,
                duration_ms,
            } => {
                if *duration_ms == 0 || strength.millionths < 0 {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_SHAKE_RANGE",
                        "shake duration and strength are invalid",
                    ));
                }
                if !matches!(target.as_str(), "main" | "camera" | "camera.main") {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_SHAKE_TARGET",
                        "only the authoritative stage camera can be shaken",
                    ));
                }
                self.shake = Some(ActiveShake {
                    target: target.clone(),
                    strength: *strength,
                    elapsed_ns: 0,
                    duration_ns: u64::from(*duration_ms) * 1_000_000,
                });
            }
            StageCommand::Timeline(timeline) => self.apply_timeline(timeline)?,
            StageCommand::SetAudioBusEnabled { bus, enabled } => {
                self.state.audio_bus_enabled.insert(*bus, *enabled);
                return Ok(vec![StageDirectorOutput::AudioBusEnabled {
                    bus: *bus,
                    enabled: *enabled,
                }]);
            }
            StageCommand::Effect {
                target,
                lip_sync,
                filter,
                fallback,
                budget_us,
            } => {
                let profile = self.profile()?;
                if !profile.allowed_filters.iter().any(|item| item == filter)
                    || !profile
                        .fallback_policy_ids
                        .iter()
                        .any(|item| item == fallback)
                    || *budget_us == 0
                    || *budget_us > profile.max_effect_budget_us
                {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_EFFECT_POLICY",
                        "effect filter, fallback, or budget is not allowed by the selected profile",
                    ));
                }
                let effect = ProductStageEffect {
                    target: target.clone(),
                    lip_sync: *lip_sync,
                    filter: filter.clone(),
                    fallback: fallback.clone(),
                    budget_us: *budget_us,
                };
                self.state.effects.insert(target.clone(), effect.clone());
                return Ok(vec![StageDirectorOutput::Effect(effect)]);
            }
        }
        self.validate_state()?;
        Ok(Vec::new())
    }

    fn apply_timeline(&mut self, command: &TimelineCommand) -> Result<(), VnError> {
        match command {
            TimelineCommand::Cancel { id, .. } => {
                if self.timelines.remove(id).is_none() && !self.completed_timelines.remove(id) {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_TIMELINE_UNKNOWN",
                        "timeline cancel references an unknown timeline",
                    ));
                }
            }
            TimelineCommand::Start(spec) => {
                self.validate_timeline(spec)?;
                if self.timelines.contains_key(&spec.id) {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_TIMELINE_DUPLICATE",
                        "timeline id is already active",
                    ));
                }
                self.completed_timelines.remove(&spec.id);
                if spec.join == VnTimelineJoinPolicy::ReplaceTarget {
                    let targets = timeline_targets(spec);
                    self.timelines
                        .retain(|_, active| timeline_targets(&active.spec).is_disjoint(&targets));
                }
                let profile = self.profile()?;
                if self.timelines.len() >= profile.max_timelines as usize {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_TIMELINE_BUDGET",
                        "active timeline count exceeds the selected profile budget",
                    ));
                }
                self.timelines.insert(
                    spec.id.clone(),
                    ActiveTimeline {
                        spec: spec.clone(),
                        elapsed_ns: 0,
                    },
                );
            }
        }
        Ok(())
    }

    fn tick_inner(&mut self, delta_ns: u64) -> Result<Vec<StageDirectorOutput>, VnError> {
        self.state.frame_index = self.state.frame_index.checked_add(1).ok_or_else(|| {
            stage_error("ASTRA_VN_STAGE_FRAME_OVERFLOW", "frame index overflowed")
        })?;
        self.state.elapsed_ns =
            self.state.elapsed_ns.checked_add(delta_ns).ok_or_else(|| {
                stage_error("ASTRA_VN_STAGE_TIME_OVERFLOW", "stage time overflowed")
            })?;
        self.advance_tweens(delta_ns)?;
        let mut output = self.advance_timelines(delta_ns)?;
        self.advance_shake(delta_ns, &mut output)?;
        self.validate_state()?;
        Ok(output)
    }

    fn advance_tweens(&mut self, delta_ns: u64) -> Result<(), VnError> {
        let mut active = Vec::with_capacity(self.tweens.len());
        let tweens = std::mem::take(&mut self.tweens);
        for mut tween in tweens {
            tween.elapsed_ns = tween
                .elapsed_ns
                .checked_add(delta_ns)
                .ok_or_else(|| {
                    stage_error("ASTRA_VN_STAGE_TIME_OVERFLOW", "tween time overflowed")
                })?
                .min(tween.duration_ns);
            let progress = eased_progress(tween.elapsed_ns, tween.duration_ns, tween.easing)?;
            let value = fixed_lerp(tween.start, tween.end, progress)?;
            self.apply_property(&tween.target, tween.property, value)?;
            if tween.elapsed_ns == tween.duration_ns {
                if tween.remove_entity {
                    let TweenTarget::Entity(id) = &tween.target else {
                        return Err(stage_error(
                            "ASTRA_VN_STAGE_TWEEN_STATE",
                            "only entity opacity tween can remove an entity",
                        ));
                    };
                    self.state.entities.remove(id);
                }
                if tween.target == TweenTarget::Transition {
                    self.state.transition = None;
                }
            } else {
                active.push(tween);
            }
        }
        self.tweens = active;
        Ok(())
    }

    fn advance_timelines(&mut self, delta_ns: u64) -> Result<Vec<StageDirectorOutput>, VnError> {
        let mut output = Vec::new();
        let ids = self.timelines.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let (spec, elapsed_ns, complete) = {
                let active = self.timelines.get_mut(&id).ok_or_else(|| {
                    stage_error(
                        "ASTRA_VN_STAGE_TIMELINE_STATE",
                        "active timeline disappeared",
                    )
                })?;
                active.elapsed_ns = active.elapsed_ns.checked_add(delta_ns).ok_or_else(|| {
                    stage_error("ASTRA_VN_STAGE_TIME_OVERFLOW", "timeline time overflowed")
                })?;
                let duration_ns = u64::from(timeline_duration_ms(&active.spec)?) * 1_000_000;
                active.elapsed_ns = active.elapsed_ns.min(duration_ns);
                (
                    active.spec.clone(),
                    active.elapsed_ns,
                    active.elapsed_ns == duration_ns,
                )
            };
            self.apply_timeline_sample(&spec, elapsed_ns)?;
            if complete {
                self.timelines.remove(&id);
                self.completed_timelines.insert(id.clone());
                output.push(StageDirectorOutput::FenceCompleted {
                    kind: "timeline".to_string(),
                    id: spec.fence.unwrap_or(spec.id),
                });
            }
        }
        Ok(output)
    }

    fn apply_timeline_sample(
        &mut self,
        spec: &TimelineSpec,
        elapsed_ns: u64,
    ) -> Result<(), VnError> {
        let elapsed_ms = (elapsed_ns / 1_000_000) as u32;
        for track in &spec.tracks {
            let value = sample_track(track, elapsed_ms)?;
            let (target, property) = timeline_target_property(&track.target, &track.property)?;
            self.apply_property(&target, property, value)?;
        }
        Ok(())
    }

    fn advance_shake(
        &mut self,
        delta_ns: u64,
        output: &mut Vec<StageDirectorOutput>,
    ) -> Result<(), VnError> {
        let Some(mut shake) = self.shake.take() else {
            self.state.camera.shake_x = FixedScalar::ZERO;
            self.state.camera.shake_y = FixedScalar::ZERO;
            return Ok(());
        };
        shake.elapsed_ns = shake
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or_else(|| stage_error("ASTRA_VN_STAGE_TIME_OVERFLOW", "shake time overflowed"))?
            .min(shake.duration_ns);
        if shake.elapsed_ns == shake.duration_ns {
            self.state.camera.shake_x = FixedScalar::ZERO;
            self.state.camera.shake_y = FixedScalar::ZERO;
            output.push(StageDirectorOutput::FenceCompleted {
                kind: "shake".to_string(),
                id: shake.target,
            });
        } else {
            let phase = self.state.frame_index % 4;
            let (x_sign, y_sign) = match phase {
                0 => (1, 0),
                1 => (0, -1),
                2 => (-1, 0),
                _ => (0, 1),
            };
            let remaining = FixedScalar {
                millionths: i64::try_from(
                    (u128::from(shake.duration_ns - shake.elapsed_ns) * 1_000_000)
                        / u128::from(shake.duration_ns),
                )
                .map_err(|_| stage_error("ASTRA_VN_STAGE_SHAKE_RANGE", "shake range overflowed"))?,
            };
            let amplitude = fixed_mul(shake.strength, remaining)?;
            self.state.camera.shake_x = FixedScalar {
                millionths: amplitude.millionths * x_sign,
            };
            self.state.camera.shake_y = FixedScalar {
                millionths: amplitude.millionths * y_sign,
            };
            self.shake = Some(shake);
        }
        Ok(())
    }

    fn validate_timeline(&self, spec: &TimelineSpec) -> Result<(), VnError> {
        let profile = self.profile()?;
        if spec.budget_us == 0 || spec.budget_us > profile.max_effect_budget_us {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TIMELINE_EFFECT_BUDGET",
                "timeline budget exceeds the selected profile",
            ));
        }
        if spec.tracks.is_empty() {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TIMELINE_TRACKS",
                "timeline must declare at least one track",
            ));
        }
        let mut keys = BTreeSet::new();
        for track in &spec.tracks {
            if !keys.insert((&track.target, &track.property)) || track.keyframes.len() < 2 {
                return Err(stage_error(
                    "ASTRA_VN_STAGE_TIMELINE_TRACK",
                    "timeline tracks must be unique and contain at least two keyframes",
                ));
            }
            if track
                .keyframes
                .windows(2)
                .any(|pair| pair[0].time_ms >= pair[1].time_ms)
            {
                return Err(stage_error(
                    "ASTRA_VN_STAGE_TIMELINE_ORDER",
                    "timeline keyframes must be strictly ordered",
                ));
            }
            let _ = timeline_target_property(&track.target, &track.property)?;
        }
        if spec.join == VnTimelineJoinPolicy::Block && spec.fence.is_none() {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TIMELINE_FENCE",
                "blocking timeline must declare a completion fence",
            ));
        }
        Ok(())
    }

    fn validate_state(&self) -> Result<(), VnError> {
        if self.state.schema != PRODUCT_STAGE_STATE_SCHEMA
            || self.state.camera.zoom.millionths <= 0
            || self.state.layers.len() > self.profile()?.max_layers as usize
            || self.timelines.len() > self.profile()?.max_timelines as usize
        {
            return Err(stage_error(
                "ASTRA_VN_STAGE_STATE",
                "product stage state violates its schema or profile budget",
            ));
        }
        for entity in self.state.entities.values() {
            if !self.state.layers.contains_key(&entity.layer)
                || !(0..=1_000_000).contains(&entity.opacity.millionths)
            {
                return Err(stage_error(
                    "ASTRA_VN_STAGE_ENTITY_STATE",
                    "stage entity references an unknown layer or invalid opacity",
                ));
            }
        }
        Ok(())
    }

    fn resolve_duration(
        &self,
        command_kind: &str,
        preset: Option<&str>,
        explicit_ms: u32,
    ) -> Result<u32, VnError> {
        match preset {
            Some(preset) => {
                let preset = self
                    .manifest
                    .resolve_preset(&self.state.profile, command_kind, preset)
                    .map_err(VnError::Diagnostic)?;
                Ok(if explicit_ms == 0 {
                    preset.duration_ms
                } else {
                    explicit_ms
                })
            }
            None => Ok(explicit_ms),
        }
    }

    fn resolve_easing(
        &self,
        command_kind: &str,
        preset: Option<&str>,
    ) -> Result<VnPresentationEasing, VnError> {
        preset.map_or(Ok(VnPresentationEasing::Linear), |preset| {
            self.manifest
                .resolve_preset(&self.state.profile, command_kind, preset)
                .map(|preset| preset.easing)
                .map_err(VnError::Diagnostic)
        })
    }

    fn schedule_tween(&mut self, tween: StageTween) {
        self.tweens
            .retain(|active| active.target != tween.target || active.property != tween.property);
        self.tweens.push(tween);
    }

    fn apply_property(
        &mut self,
        target: &TweenTarget,
        property: TweenProperty,
        value: FixedScalar,
    ) -> Result<(), VnError> {
        match target {
            TweenTarget::Camera => match property {
                TweenProperty::X => self.state.camera.x = value,
                TweenProperty::Y => self.state.camera.y = value,
                TweenProperty::Zoom => {
                    if value.millionths <= 0 {
                        return Err(stage_error(
                            "ASTRA_VN_STAGE_CAMERA_ZOOM",
                            "camera timeline produced a non-positive zoom",
                        ));
                    }
                    self.state.camera.zoom = value;
                }
                TweenProperty::Rotation => self.state.camera.rotation = value,
                _ => {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_TIMELINE_PROPERTY",
                        "camera timeline property is unsupported",
                    ))
                }
            },
            TweenTarget::Entity(id) => {
                let entity = self.entity_mut(id)?;
                match property {
                    TweenProperty::X => entity.x = value,
                    TweenProperty::Y => entity.y = value,
                    TweenProperty::Opacity => entity.opacity = value,
                    _ => {
                        return Err(stage_error(
                            "ASTRA_VN_STAGE_TIMELINE_PROPERTY",
                            "entity timeline property is unsupported",
                        ))
                    }
                }
            }
            TweenTarget::Transition => {
                if property != TweenProperty::Progress {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_TWEEN_STATE",
                        "transition tween property is invalid",
                    ));
                }
                self.state
                    .transition
                    .as_mut()
                    .ok_or_else(|| {
                        stage_error(
                            "ASTRA_VN_STAGE_TWEEN_STATE",
                            "transition tween has no active transition",
                        )
                    })?
                    .progress = value;
            }
        }
        Ok(())
    }

    fn cancel_target(&mut self, target: &TweenTarget) {
        self.tweens.retain(|active| &active.target != target);
    }

    fn require_configured(&self) -> Result<(), VnError> {
        if self.state.configured {
            Ok(())
        } else {
            Err(stage_error(
                "ASTRA_VN_STAGE_NOT_CONFIGURED",
                "stage must be configured before declaring product layers",
            ))
        }
    }

    fn require_layer(&self, id: &str, kinds: &[StageLayerKind]) -> Result<(), VnError> {
        self.require_configured()?;
        let layer = self.state.layers.get(id).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_LAYER_UNKNOWN",
                "presentation command references an undeclared layer",
            )
        })?;
        if !kinds.contains(&layer.kind) {
            return Err(stage_error(
                "ASTRA_VN_STAGE_LAYER_KIND",
                "presentation command is incompatible with the declared layer kind",
            ));
        }
        Ok(())
    }

    fn profile(&self) -> Result<&crate::VnPresentationProfile, VnError> {
        self.manifest
            .profile(&self.state.profile)
            .map_err(VnError::Diagnostic)
    }

    fn entity(&self, id: &str) -> Result<&ProductStageEntity, VnError> {
        self.state.entities.get(id).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_ENTITY_UNKNOWN",
                "presentation command references an unknown stage entity",
            )
        })
    }

    fn entity_mut(&mut self, id: &str) -> Result<&mut ProductStageEntity, VnError> {
        self.state.entities.get_mut(id).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_ENTITY_UNKNOWN",
                "presentation command references an unknown stage entity",
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StageTween {
    target: TweenTarget,
    property: TweenProperty,
    start: FixedScalar,
    end: FixedScalar,
    duration_ns: u64,
    elapsed_ns: u64,
    easing: VnPresentationEasing,
    remove_entity: bool,
}

impl StageTween {
    fn new(
        target: TweenTarget,
        property: TweenProperty,
        start: FixedScalar,
        end: FixedScalar,
        duration_ms: u32,
        easing: VnPresentationEasing,
        remove_entity: bool,
    ) -> Result<Self, VnError> {
        if duration_ms == 0 {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TWEEN_DURATION",
                "scheduled tween duration must be non-zero",
            ));
        }
        Ok(Self {
            target,
            property,
            start,
            end,
            duration_ns: u64::from(duration_ms) * 1_000_000,
            elapsed_ns: 0,
            easing,
            remove_entity,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TweenTarget {
    Camera,
    Entity(String),
    Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TweenProperty {
    X,
    Y,
    Zoom,
    Rotation,
    Opacity,
    Progress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ActiveTimeline {
    spec: TimelineSpec,
    elapsed_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ActiveShake {
    target: String,
    strength: FixedScalar,
    elapsed_ns: u64,
    duration_ns: u64,
}

fn validate_viewport(viewport: StageViewport) -> Result<(), VnError> {
    if viewport.width == 0
        || viewport.height == 0
        || viewport.width > 16_384
        || viewport.height > 16_384
    {
        return Err(stage_error(
            "ASTRA_VN_STAGE_VIEWPORT",
            "stage viewport is empty or exceeds the product limit",
        ));
    }
    Ok(())
}

fn placement_x(placement: StagePlacement, width: u32) -> Result<FixedScalar, VnError> {
    let numerator = match placement {
        StagePlacement::Left => 1_i64,
        StagePlacement::Center => 3,
        StagePlacement::Right => 5,
    };
    let millionths = i64::from(width)
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_mul(numerator))
        .map(|value| value / 6)
        .ok_or_else(|| stage_error("ASTRA_VN_STAGE_COORDINATE", "stage coordinate overflowed"))?;
    Ok(FixedScalar { millionths })
}

fn timeline_targets(spec: &TimelineSpec) -> BTreeSet<String> {
    spec.tracks
        .iter()
        .map(|track| track.target.clone())
        .collect()
}

fn timeline_duration_ms(spec: &TimelineSpec) -> Result<u32, VnError> {
    spec.tracks
        .iter()
        .filter_map(|track| track.keyframes.last().map(|keyframe| keyframe.time_ms))
        .max()
        .filter(|duration| *duration > 0)
        .ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_TIMELINE_DURATION",
                "timeline has no non-zero terminal keyframe",
            )
        })
}

fn timeline_target_property(
    target: &str,
    property: &str,
) -> Result<(TweenTarget, TweenProperty), VnError> {
    let target = match target {
        "main" | "camera" | "camera.main" => TweenTarget::Camera,
        id => TweenTarget::Entity(id.to_string()),
    };
    let property = match property {
        "x" => TweenProperty::X,
        "y" => TweenProperty::Y,
        "opacity" => TweenProperty::Opacity,
        "zoom" => TweenProperty::Zoom,
        "rotation" => TweenProperty::Rotation,
        _ => {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TIMELINE_PROPERTY",
                "timeline property is not supported by the product stage model",
            ))
        }
    };
    match (&target, property) {
        (TweenTarget::Camera, TweenProperty::Opacity) => Err(stage_error(
            "ASTRA_VN_STAGE_TIMELINE_PROPERTY",
            "camera does not expose opacity",
        )),
        (TweenTarget::Entity(_), TweenProperty::Zoom | TweenProperty::Rotation) => {
            Err(stage_error(
                "ASTRA_VN_STAGE_TIMELINE_PROPERTY",
                "entity does not expose camera zoom or rotation",
            ))
        }
        _ => Ok((target, property)),
    }
}

fn sample_track(track: &crate::VnTimelineTrack, elapsed_ms: u32) -> Result<FixedScalar, VnError> {
    let first = track.keyframes.first().ok_or_else(|| {
        stage_error(
            "ASTRA_VN_STAGE_TIMELINE_TRACK",
            "timeline track has no keyframes",
        )
    })?;
    if elapsed_ms <= first.time_ms {
        return Ok(first.value);
    }
    for pair in track.keyframes.windows(2) {
        let [left, right] = pair else {
            unreachable!("windows(2) always yields pairs")
        };
        if elapsed_ms <= right.time_ms {
            let numerator = u64::from(elapsed_ms - left.time_ms) * 1_000_000;
            let denominator = u64::from(right.time_ms - left.time_ms);
            return fixed_lerp(
                left.value,
                right.value,
                FixedScalar {
                    millionths: i64::try_from(numerator / denominator).map_err(|_| {
                        stage_error(
                            "ASTRA_VN_STAGE_TIMELINE_RANGE",
                            "timeline interpolation range overflowed",
                        )
                    })?,
                },
            );
        }
    }
    Ok(track
        .keyframes
        .last()
        .expect("validated timeline has keyframes")
        .value)
}

fn eased_progress(
    elapsed_ns: u64,
    duration_ns: u64,
    easing: VnPresentationEasing,
) -> Result<FixedScalar, VnError> {
    if duration_ns == 0 || elapsed_ns > duration_ns {
        return Err(stage_error(
            "ASTRA_VN_STAGE_TWEEN_RANGE",
            "tween progress is outside its duration",
        ));
    }
    let linear = FixedScalar {
        millionths: i64::try_from((u128::from(elapsed_ns) * 1_000_000) / u128::from(duration_ns))
            .map_err(|_| {
            stage_error("ASTRA_VN_STAGE_TWEEN_RANGE", "tween progress overflowed")
        })?,
    };
    match easing {
        VnPresentationEasing::Linear => Ok(linear),
        VnPresentationEasing::EaseIn => fixed_mul(linear, linear),
        VnPresentationEasing::EaseOut => {
            let inverse = FixedScalar {
                millionths: 1_000_000 - linear.millionths,
            };
            Ok(FixedScalar {
                millionths: 1_000_000 - fixed_mul(inverse, inverse)?.millionths,
            })
        }
        VnPresentationEasing::EaseInOut if linear.millionths <= 500_000 => {
            let squared = fixed_mul(linear, linear)?;
            Ok(FixedScalar {
                millionths: squared.millionths.checked_mul(2).ok_or_else(|| {
                    stage_error("ASTRA_VN_STAGE_TWEEN_RANGE", "easing overflowed")
                })?,
            })
        }
        VnPresentationEasing::EaseInOut => {
            let inverse = FixedScalar {
                millionths: 1_000_000 - linear.millionths,
            };
            let squared = fixed_mul(inverse, inverse)?;
            Ok(FixedScalar {
                millionths: 1_000_000
                    - squared.millionths.checked_mul(2).ok_or_else(|| {
                        stage_error("ASTRA_VN_STAGE_TWEEN_RANGE", "easing overflowed")
                    })?,
            })
        }
    }
}

fn fixed_mul(left: FixedScalar, right: FixedScalar) -> Result<FixedScalar, VnError> {
    let product = i128::from(left.millionths)
        .checked_mul(i128::from(right.millionths))
        .ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_FIXED_RANGE",
                "fixed-point product overflowed",
            )
        })?
        / 1_000_000;
    Ok(FixedScalar {
        millionths: i64::try_from(product).map_err(|_| {
            stage_error(
                "ASTRA_VN_STAGE_FIXED_RANGE",
                "fixed-point product exceeds the product range",
            )
        })?,
    })
}

fn fixed_lerp(
    start: FixedScalar,
    end: FixedScalar,
    progress: FixedScalar,
) -> Result<FixedScalar, VnError> {
    if !(0..=1_000_000).contains(&progress.millionths) {
        return Err(stage_error(
            "ASTRA_VN_STAGE_TWEEN_RANGE",
            "interpolation progress is outside zero and one",
        ));
    }
    let delta = i128::from(end.millionths) - i128::from(start.millionths);
    let value = i128::from(start.millionths)
        + delta
            .checked_mul(i128::from(progress.millionths))
            .ok_or_else(|| stage_error("ASTRA_VN_STAGE_FIXED_RANGE", "interpolation overflowed"))?
            / 1_000_000;
    Ok(FixedScalar {
        millionths: i64::try_from(value).map_err(|_| {
            stage_error(
                "ASTRA_VN_STAGE_FIXED_RANGE",
                "interpolation exceeds the product range",
            )
        })?,
    })
}

fn stage_error(code: &str, message: &str) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message))
}
