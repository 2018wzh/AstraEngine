use super::*;

impl ProductStageDirector {
    pub(super) fn apply_timeline(&mut self, command: &TimelineCommand) -> Result<(), VnError> {
        match command {
            TimelineCommand::Cancel { id, .. } => {
                if self.timelines.remove(id).is_none() && !self.settled_timelines.remove(id) {
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
                self.settled_timelines.remove(&spec.id);
                if spec.join == VnTimelineJoinPolicy::ReplaceTarget {
                    let properties = timeline_properties(spec);
                    self.timelines.retain(|_, active| {
                        active.spec.tracks.retain(|track| {
                            !properties.contains(&(
                                canonical_timeline_target(&track.target),
                                track.property.as_str(),
                            ))
                        });
                        !active.spec.tracks.is_empty()
                    });
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

    pub(super) fn tick_inner(
        &mut self,
        delta_ns: u64,
    ) -> Result<Vec<StageDirectorOutput>, VnError> {
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

    pub(super) fn advance_tweens(&mut self, delta_ns: u64) -> Result<(), VnError> {
        let mut active = Vec::with_capacity(self.tweens.len());
        let tweens = std::mem::take(&mut self.tweens);
        let mut removed_entities = BTreeSet::new();
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
                    removed_entities.insert(id.clone());
                }
                if tween.target == TweenTarget::Transition {
                    self.state.transition = None;
                }
            } else {
                active.push(tween);
            }
        }
        self.tweens = active;
        for id in removed_entities {
            self.state.entities.remove(&id);
            self.cancel_target(&TweenTarget::Entity(id));
        }
        Ok(())
    }

    pub(super) fn advance_timelines(
        &mut self,
        delta_ns: u64,
    ) -> Result<Vec<StageDirectorOutput>, VnError> {
        let mut output = Vec::new();
        let timelines = std::mem::take(&mut self.timelines);
        for (id, mut active) in timelines {
            let duration_ns = u64::from(timeline_duration_ms(&active.spec)?) * 1_000_000;
            active.elapsed_ns = active.elapsed_ns.saturating_add(delta_ns).min(duration_ns);
            self.apply_timeline_sample(&active.spec, active.elapsed_ns)?;
            if active.elapsed_ns == duration_ns {
                self.settled_timelines.insert(id.clone());
                output.push(StageDirectorOutput::FenceCompleted {
                    kind: "timeline".to_string(),
                    id: active.spec.fence.unwrap_or(id),
                });
            } else {
                self.timelines.insert(id, active);
            }
        }
        Ok(output)
    }

    pub(super) fn apply_timeline_sample(
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

    pub(super) fn advance_shake(
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

    pub(super) fn validate_timeline(&self, spec: &TimelineSpec) -> Result<(), VnError> {
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
            let (target, property) = timeline_target_property(&track.target, &track.property)?;
            if let TweenTarget::Entity(id) = target {
                self.entity(&id)?;
            }
            for keyframe in &track.keyframes {
                match property {
                    TweenProperty::Zoom if keyframe.value.millionths <= 0 => {
                        return Err(stage_error(
                            "ASTRA_VN_STAGE_CAMERA_ZOOM",
                            "camera timeline keyframes must have positive zoom",
                        ))
                    }
                    TweenProperty::Opacity
                        if !(0..=1_000_000).contains(&keyframe.value.millionths) =>
                    {
                        return Err(stage_error(
                            "ASTRA_VN_STAGE_ENTITY_STATE",
                            "entity timeline opacity must be between zero and one",
                        ))
                    }
                    _ => {}
                }
            }
        }
        if spec.join == VnTimelineJoinPolicy::Block && spec.fence.is_none() {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TIMELINE_FENCE",
                "blocking timeline must declare a completion fence",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_state(&self) -> Result<(), VnError> {
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
        for tween in &self.tweens {
            if let TweenTarget::Entity(id) = &tween.target {
                self.entity(id)?;
            }
        }
        for timeline in self.timelines.values() {
            for track in &timeline.spec.tracks {
                let (target, _) = timeline_target_property(&track.target, &track.property)?;
                if let TweenTarget::Entity(id) = target {
                    self.entity(&id)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn resolve_duration(
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

    pub(super) fn resolve_easing(
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

    pub(super) fn schedule_tween(&mut self, tween: StageTween) {
        self.tweens
            .retain(|active| active.target != tween.target || active.property != tween.property);
        self.tweens.push(tween);
    }

    pub(super) fn apply_property(
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

    pub(super) fn cancel_target(&mut self, target: &TweenTarget) {
        self.tweens.retain(|active| &active.target != target);
        let TweenTarget::Entity(entity_id) = target else {
            return;
        };
        let mut settled = Vec::new();
        self.timelines.retain(|id, active| {
            active
                .spec
                .tracks
                .retain(|track| canonical_timeline_target(&track.target) != entity_id);
            if active.spec.tracks.is_empty() {
                settled.push(id.clone());
                false
            } else {
                true
            }
        });
        // A later authored cancel may retire this id, but removal is not a successful fence.
        self.settled_timelines.extend(settled);
    }
}
