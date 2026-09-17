use super::*;
use crate::MusicaCharacterState;

impl Scene {
    pub(super) fn characters(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        state: &MusicaRuntimeState,
    ) -> FamilyResult<()> {
        if state.characters.is_empty() {
            return Ok(());
        }
        commands.push(SceneCommand::PushClip {
            rect: RectI {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
        });
        for character in state
            .characters
            .values()
            .filter(|character| character.visible)
        {
            let [resource] = character.resource_uris.as_slice() else {
                return Err(error(
                    "ASTRA_EMU_MUSICA_CHARACTER_RESOURCE_COUNT",
                    "character requires one resource",
                ));
            };
            self.character_sprite(commands, character, resource, character.opacity_256)?;
            if let Some(replacement) = &character.replacement {
                self.character_sprite(
                    commands,
                    character,
                    &replacement.resource_uri,
                    replacement.next_opacity_256,
                )?;
            }
        }
        commands.push(SceneCommand::PopClip);
        Ok(())
    }
    fn character_sprite(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        character: &MusicaCharacterState,
        uri: &str,
        opacity: u16,
    ) -> FamilyResult<()> {
        if !uri.to_ascii_lowercase().ends_with(".png") {
            return Err(error(
                "ASTRA_EMU_MUSICA_CHARACTER_CODEC",
                "character requires a static PNG resource",
            ));
        }
        let frame = self.texture(uri)?;
        let left = i64::from(character.anchor_position[0]) - i64::from(frame.width) / 2;
        let top = i64::from(self.height)
            - i64::from(frame.height)
            - i64::from(character.anchor_position[1]);
        let left = i32::try_from(left).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_CHARACTER_POSITION",
                "character horizontal anchor overflowed",
            )
        })?;
        let top = i32::try_from(top).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_CHARACTER_POSITION",
                "character vertical anchor overflowed",
            )
        })?;
        let mirrored = !character.positive_orientation;
        commands.push(SceneCommand::PushTransform {
            transform: astra_media_core::Transform2D {
                m11: if mirrored { -1.0 } else { 1.0 },
                tx: left as f32 + if mirrored { frame.width as f32 } else { 0.0 },
                ty: top as f32,
                ..astra_media_core::Transform2D::IDENTITY
            },
        });
        commands.push(SceneCommand::Texture {
            id: format!("character:{}:{}", character.slot_id, commands.len()),
            destination: RectI {
                x: 0,
                y: 0,
                width: frame.width,
                height: frame.height,
            },
            frame,
            opacity: f32::from(opacity) / 256.0,
            blend: BlendMode::Alpha,
        });
        commands.push(SceneCommand::PopTransform);
        Ok(())
    }
}
