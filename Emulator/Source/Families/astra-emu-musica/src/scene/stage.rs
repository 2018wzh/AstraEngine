use super::*;
use crate::MusicaStageCommand;

impl Scene {
    pub(super) fn stage(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        stage: &MusicaStageCommand,
    ) -> FamilyResult<()> {
        let (foreground, prefix) = match stage.resource_sequence.as_slice() {
            [foreground] => (foreground, None),
            [prefix, foreground] => (foreground, prefix.as_deref()),
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_STAGE_SEQUENCE",
                    "native stage requires one or two resource slots",
                ))
            }
        };
        if let Some(background) = &stage.background {
            self.layer(
                commands,
                &background.resource_uri,
                background.x,
                background.y,
                1.0,
            )?;
        }
        for stand in &stage.stands {
            self.stand(commands, stand)?;
        }
        // Native main foreground precedes the separately registered prefix layer.
        if let Some(uri) = foreground {
            self.stage_foreground(commands, uri, stage.reference_position.unwrap_or([0, 0]))?;
        }
        if let Some(uri) = prefix {
            self.stage_foreground(commands, uri, [0, 0])?;
        }
        Ok(())
    }

    fn stage_foreground(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        uri: &str,
        position: [i32; 2],
    ) -> FamilyResult<()> {
        let blend = foreground_blend(uri)?;
        self.layer_with_blend(commands, uri, position, 1.0, blend)
    }
}

fn foreground_blend(uri: &str) -> FamilyResult<BlendMode> {
    let suffix = uri.get(uri.len().saturating_sub(7)..).unwrap_or("");
    if suffix.eq_ignore_ascii_case("_sc.png") {
        Ok(BlendMode::Screen)
    } else if suffix.eq_ignore_ascii_case("_ov.png") {
        Err(error(
            "ASTRA_EMU_MUSICA_STAGE_BLEND",
            "native overlay compositing is not implemented",
        ))
    } else {
        Ok(BlendMode::Alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_suffix_selects_screen_and_rejects_overlay() {
        assert_eq!(
            foreground_blend("musica:/bg/light_SC.PNG").unwrap(),
            BlendMode::Screen
        );
        assert_eq!(
            foreground_blend("musica:/bg/light.png").unwrap(),
            BlendMode::Alpha
        );
        assert_eq!(
            foreground_blend("musica:/bg/light_ov.png")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MUSICA_STAGE_BLEND"
        );
    }
}
