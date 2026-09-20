use super::*;

impl Scene {
    pub(super) fn particles(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        state: &MusicaRuntimeState,
    ) -> FamilyResult<()> {
        if state.firefly.is_none() && state.secondary_effect.is_none() {
            return Ok(());
        }
        crate::runtime::particles::validate_state(state)
            .map_err(|cause| error(cause.diagnostic_code(), "invalid particle state"))?;
        if (self.width, self.height) != (1280, 720) {
            return Err(error(
                "ASTRA_EMU_MUSICA_PARTICLE_STAGE_IDENTITY",
                "particle effects require a 1280x720 stage",
            ));
        }
        commands.push(SceneCommand::PushClip {
            rect: RectI {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
        });
        if let Some(effect) = &state.firefly {
            self.particle_layer(
                commands,
                &effect.resources,
                effect.particles.iter().map(|particle| {
                    (
                        particle.kind,
                        particle.position,
                        f32::from(effect.fade_alpha_256) / 256.0 * f32::from(particle.opacity_255)
                            / 255.0,
                    )
                }),
            )?;
        }
        if let Some(effect) = &state.secondary_effect {
            self.particle_layer(
                commands,
                &effect.resources,
                effect.particles.iter().map(|particle| {
                    (
                        particle.kind,
                        particle.position,
                        f32::from(effect.alpha_256) / 256.0,
                    )
                }),
            )?;
        }
        commands.push(SceneCommand::PopClip);
        Ok(())
    }

    fn particle_layer(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        resources: &[String; 3],
        particles: impl Iterator<Item = (u8, [i32; 2], f32)>,
    ) -> FamilyResult<()> {
        let frames = resources
            .iter()
            .map(|uri| self.texture_asset(uri))
            .collect::<FamilyResult<Vec<_>>>()?;
        for (kind, [x, y], opacity) in particles {
            if opacity == 0.0 {
                continue;
            }
            let asset = frames
                .get(usize::from(kind))
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_PARTICLE_KIND",
                        "particle texture index is out of bounds",
                    )
                })?
                .clone();
            commands.push(SceneCommand::Texture {
                id: format!("particle:{}", commands.len()),
                destination: RectI {
                    x: x.checked_add(asset.logical_origin[0]).ok_or_else(|| {
                        error("ASTRA_EMU_MUSICA_PARTICLE_POSITION", "particle x overflows")
                    })?,
                    y: y.checked_add(asset.logical_origin[1]).ok_or_else(|| {
                        error("ASTRA_EMU_MUSICA_PARTICLE_POSITION", "particle y overflows")
                    })?,
                    width: asset.logical_extent.width,
                    height: asset.logical_extent.height,
                },
                frame: asset.frame,
                opacity,
                blend: BlendMode::Alpha,
            });
        }
        Ok(())
    }
}
