use super::*;

impl ProductStageDirector {
    pub(super) fn region_envelope(
        &mut self,
        command: &StageCommand,
    ) -> Result<Option<PresentationCommandEnvelope>, VnError> {
        match command {
            StageCommand::Show { layer, opacity, .. } => {
                self.require_layer(
                    layer,
                    &[
                        StageLayerKind::Sprite,
                        StageLayerKind::Cg,
                        StageLayerKind::Ui,
                    ],
                )?;
                if !(0..=1_000_000).contains(&opacity.millionths) {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_ENTITY_STATE",
                        "stage entity opacity must be between zero and one",
                    ));
                }
            }
            StageCommand::Background { layer, .. } => {
                self.require_layer(layer, &[StageLayerKind::Background])?
            }
            StageCommand::Movie { layer, alpha, .. } => {
                self.require_layer(layer, &[StageLayerKind::Video])?;
                if !(0..=1_000_000).contains(&alpha.millionths) {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_MOVIE_ALPHA",
                        "movie alpha must be between zero and one",
                    ));
                }
            }
            StageCommand::ClearLayer { layer, .. } if !self.state.layers.contains_key(layer) => {
                return Err(stage_error(
                    "ASTRA_VN_STAGE_LAYER_UNKNOWN",
                    "presentation command references an undeclared layer",
                ));
            }
            _ => {}
        }
        let (interrupt, fence, payload) = match command {
            StageCommand::Background {
                asset,
                layer,
                preset,
                duration_ms,
                interrupt,
            } => (
                *interrupt,
                None,
                PresentationRegionCommand::Background(BackgroundRegionCommand {
                    layer: layer.clone(),
                    asset: Some(asset.clone()),
                    duration_ns: duration_ns(self.resolve_duration(
                        command.kind(),
                        preset.as_deref(),
                        *duration_ms,
                    )?)?,
                }),
            ),
            StageCommand::Show {
                id,
                asset,
                pose,
                layer,
                preset,
                interrupt,
                ..
            } => (
                *interrupt,
                None,
                PresentationRegionCommand::Character(CharacterRegionCommand {
                    character_id: id.clone(),
                    asset: asset.clone(),
                    pose: pose.clone(),
                    layer: layer.clone(),
                    visible: true,
                    duration_ns: duration_ns(self.resolve_duration(
                        command.kind(),
                        preset.as_deref(),
                        0,
                    )?)?,
                }),
            ),
            StageCommand::Hide {
                id,
                preset,
                duration_ms,
                interrupt,
            } => {
                let entity = self.entity(id)?;
                (
                    *interrupt,
                    None,
                    PresentationRegionCommand::Character(CharacterRegionCommand {
                        character_id: id.clone(),
                        asset: entity.asset.clone(),
                        pose: entity.pose.clone(),
                        layer: entity.layer.clone(),
                        visible: false,
                        duration_ns: duration_ns(self.resolve_duration(
                            command.kind(),
                            preset.as_deref(),
                            *duration_ms,
                        )?)?,
                    }),
                )
            }
            StageCommand::Move {
                id,
                duration_ms,
                preset,
                interrupt,
                ..
            } => {
                let entity = self.entity(id)?;
                (
                    *interrupt,
                    None,
                    PresentationRegionCommand::Character(CharacterRegionCommand {
                        character_id: id.clone(),
                        asset: entity.asset.clone(),
                        pose: entity.pose.clone(),
                        layer: entity.layer.clone(),
                        visible: entity.visible,
                        duration_ns: duration_ns(self.resolve_duration(
                            command.kind(),
                            preset.as_deref(),
                            *duration_ms,
                        )?)?,
                    }),
                )
            }
            StageCommand::Movie {
                layer,
                asset,
                loop_mode,
                end,
                fence,
                fallback,
                interrupt,
                ..
            } => (
                *interrupt,
                fence.clone(),
                PresentationRegionCommand::Video(VideoRegionCommand {
                    session_id: format!("movie.{layer}"),
                    layer: layer.clone(),
                    asset: asset.clone(),
                    logical_start_ns: 0,
                    loop_mode: *loop_mode,
                    end_behavior: *end,
                    fallback: fallback.clone(),
                }),
            ),
            _ => return Ok(None),
        };
        let sequence = self.next_region_sequence;
        self.next_region_sequence = self.next_region_sequence.checked_add(1).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_REGION_SEQUENCE",
                "presentation region sequence overflowed",
            )
        })?;
        Ok(Some(PresentationCommandEnvelope {
            fixed_step: self.state.frame_index,
            sequence,
            command_id: format!("stage.region.{sequence}"),
            interrupt,
            fence,
            payload,
        }))
    }
}
