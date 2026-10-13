use super::*;

impl ProductStageDirector {
    pub(super) fn prepare_interrupt(&mut self, command: &StageCommand) -> Result<bool, VnError> {
        let Some((policy, targets, coordinator_owned)) = self.interrupt_targets(command) else {
            return Ok(true);
        };
        let active = self
            .tweens
            .iter()
            .any(|tween| targets.iter().any(|target| target == &tween.target));
        if !active {
            return Ok(true);
        }
        match policy {
            PresentationInterruptPolicy::Queue => {
                if coordinator_owned {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_REGION_COORDINATOR_DIVERGENCE",
                        "region coordinator activated a queued command while its stage tween remained active",
                    ));
                }
                if self.queued_stage_commands.len() >= 4_096 {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_COMMAND_QUEUE_LIMIT",
                        "stage command queue exceeds its serialized bound",
                    ));
                }
                self.queued_stage_commands.push_back(command.clone());
                Ok(false)
            }
            PresentationInterruptPolicy::Reject => Err(stage_error(
                "ASTRA_VN_STAGE_INTERRUPT_REJECTED",
                "stage command conflicts with an active authored transition",
            )),
            PresentationInterruptPolicy::ReplaceFromCurrent => {
                self.tweens
                    .retain(|tween| !targets.iter().any(|target| target == &tween.target));
                Ok(true)
            }
            PresentationInterruptPolicy::SnapThenStart => {
                let completing = self
                    .tweens
                    .iter()
                    .filter(|tween| targets.iter().any(|target| target == &tween.target))
                    .cloned()
                    .collect::<Vec<_>>();
                self.tweens
                    .retain(|tween| !targets.iter().any(|target| target == &tween.target));
                let mut remove = BTreeSet::new();
                for tween in completing {
                    self.apply_property(&tween.target, tween.property, tween.end)?;
                    if tween.remove_entity {
                        if let TweenTarget::Entity(id) = tween.target {
                            remove.insert(id);
                        }
                    }
                }
                for id in remove {
                    self.state.entities.remove(&id);
                    self.cancel_target(&TweenTarget::Entity(id));
                }
                Ok(true)
            }
        }
    }

    pub(super) fn interrupt_targets(
        &self,
        command: &StageCommand,
    ) -> Option<(PresentationInterruptPolicy, Vec<TweenTarget>, bool)> {
        match command {
            StageCommand::Background {
                layer, interrupt, ..
            } => Some((
                *interrupt,
                vec![TweenTarget::Entity(format!("background.{layer}"))],
                true,
            )),
            StageCommand::Show { id, interrupt, .. }
            | StageCommand::Hide { id, interrupt, .. }
            | StageCommand::Move { id, interrupt, .. } => {
                Some((*interrupt, vec![TweenTarget::Entity(id.clone())], true))
            }
            StageCommand::ClearLayer {
                layer, interrupt, ..
            } => Some((
                *interrupt,
                self.state
                    .entities
                    .values()
                    .filter(|entity| entity.layer == *layer)
                    .map(|entity| TweenTarget::Entity(entity.id.clone()))
                    .collect(),
                false,
            )),
            StageCommand::Movie { interrupt, .. } => Some((*interrupt, Vec::new(), true)),
            _ => None,
        }
    }

    pub(super) fn require_configured(&self) -> Result<(), VnError> {
        if self.state.configured {
            Ok(())
        } else {
            Err(stage_error(
                "ASTRA_VN_STAGE_NOT_CONFIGURED",
                "stage must be configured before declaring product layers",
            ))
        }
    }

    pub(super) fn require_layer(&self, id: &str, kinds: &[StageLayerKind]) -> Result<(), VnError> {
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

    pub(super) fn profile(&self) -> Result<&crate::VnPresentationProfile, VnError> {
        self.manifest
            .profile(&self.state.profile)
            .map_err(VnError::Diagnostic)
    }

    pub(super) fn entity(&self, id: &str) -> Result<&ProductStageEntity, VnError> {
        self.state.entities.get(id).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_ENTITY_UNKNOWN",
                "presentation command references an unknown stage entity",
            )
        })
    }

    pub(super) fn entity_mut(&mut self, id: &str) -> Result<&mut ProductStageEntity, VnError> {
        self.state.entities.get_mut(id).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_ENTITY_UNKNOWN",
                "presentation command references an unknown stage entity",
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StageTween {
    pub(super) target: TweenTarget,
    pub(super) property: TweenProperty,
    pub(super) start: FixedScalar,
    pub(super) end: FixedScalar,
    pub(super) duration_ns: u64,
    pub(super) elapsed_ns: u64,
    pub(super) easing: VnPresentationEasing,
    pub(super) remove_entity: bool,
}

impl StageTween {
    pub(super) fn new(
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
pub(super) enum TweenTarget {
    Camera,
    Entity(String),
    Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum TweenProperty {
    X,
    Y,
    Zoom,
    Rotation,
    Opacity,
    Progress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ActiveTimeline {
    pub(super) spec: TimelineSpec,
    pub(super) elapsed_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ActiveShake {
    pub(super) target: String,
    pub(super) strength: FixedScalar,
    pub(super) elapsed_ns: u64,
    pub(super) duration_ns: u64,
}

pub(super) fn validate_viewport(viewport: StageViewport) -> Result<(), VnError> {
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

pub(super) fn duration_ns(duration_ms: u32) -> Result<u64, VnError> {
    u64::from(duration_ms)
        .checked_mul(1_000_000)
        .ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_REGION_DURATION",
                "presentation region duration overflowed",
            )
        })
}

pub(super) fn placement_x(placement: StagePlacement, width: u32) -> Result<FixedScalar, VnError> {
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

pub(super) fn canonical_timeline_target(target: &str) -> &str {
    match target {
        "main" | "camera" | "camera.main" => "camera",
        other => other,
    }
}

pub(super) fn timeline_properties(spec: &TimelineSpec) -> BTreeSet<(&str, &str)> {
    spec.tracks
        .iter()
        .map(|track| {
            (
                canonical_timeline_target(&track.target),
                track.property.as_str(),
            )
        })
        .collect()
}

pub(super) fn timeline_duration_ms(spec: &TimelineSpec) -> Result<u32, VnError> {
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

pub(super) fn timeline_target_property(
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

pub(super) fn sample_track(
    track: &crate::VnTimelineTrack,
    elapsed_ms: u32,
) -> Result<FixedScalar, VnError> {
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
            return fixed_lerp(
                left.value,
                right.value,
                eased_progress(
                    u64::from(elapsed_ms - left.time_ms),
                    u64::from(right.time_ms - left.time_ms),
                    track.easing,
                )?,
            );
        }
    }
    Ok(track
        .keyframes
        .last()
        .expect("validated timeline has keyframes")
        .value)
}

pub(super) fn eased_progress(
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

pub(super) fn fixed_mul(left: FixedScalar, right: FixedScalar) -> Result<FixedScalar, VnError> {
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
