use super::*;

impl ProductStageDirector {
    pub(super) fn apply_inner(
        &mut self,
        command: &StageCommand,
    ) -> Result<Vec<StageDirectorOutput>, VnError> {
        tracing::trace!(
            event = "vn.presentation.director.apply",
            command = command.kind(),
            frame_index = self.state.frame_index,
            "typed presentation command entered the product stage director"
        );
        if !self.prepare_interrupt(command)? {
            return Ok(Vec::new());
        }
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
                interrupt,
            } => {
                self.require_layer(layer, &[StageLayerKind::Background])?;
                let duration =
                    self.resolve_duration(command.kind(), preset.as_deref(), *duration_ms)?;
                let id = format!("background.{layer}");
                let start_opacity = if *interrupt == PresentationInterruptPolicy::ReplaceFromCurrent
                {
                    self.state
                        .entities
                        .get(&id)
                        .map_or(FixedScalar::ZERO, |entity| entity.opacity)
                } else {
                    FixedScalar::ZERO
                };
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
                            start_opacity
                        },
                        visible: true,
                    },
                );
                if duration > 0 {
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id),
                        TweenProperty::Opacity,
                        start_opacity,
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
                interrupt,
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
                let start_opacity = if *interrupt == PresentationInterruptPolicy::ReplaceFromCurrent
                {
                    self.state
                        .entities
                        .get(id)
                        .map_or(FixedScalar::ZERO, |entity| entity.opacity)
                } else {
                    FixedScalar::ZERO
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
                            start_opacity
                        },
                        visible: true,
                    },
                );
                if duration > 0 {
                    self.schedule_tween(StageTween::new(
                        TweenTarget::Entity(id.clone()),
                        TweenProperty::Opacity,
                        start_opacity,
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
                ..
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
            StageCommand::ClearLayer {
                layer, duration_ms, ..
            } => {
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
                ..
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
                ..
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
                descriptor_id,
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
                    descriptor_id: descriptor_id.clone(),
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
}
