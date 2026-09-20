use super::*;
use crate::{MusicaConfigState, MusicaPlayMode};
impl Scene {
    pub fn render_config(
        &mut self,
        config: &MusicaConfigState,
        pointer: Option<(f32, f32)>,
    ) -> FamilyResult<()> {
        config.validate().map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_CONFIG_STATE",
                "invalid native configuration",
            )
        })?;
        for (name, size) in [
            ("configBase.png", (self.width, self.height)),
            ("knob.png", (15, 25)),
            ("checkmark.png", (21, 32)),
            ("circle.png", (74, 74)),
        ] {
            let texture = self.texture_asset(&format!("musica:/sys/{name}"))?;
            if (texture.logical_extent.width, texture.logical_extent.height) != size {
                return Err(error(
                    "ASTRA_EMU_MUSICA_CONFIG_RESOURCE_DIMENSIONS",
                    "native configuration resource dimensions are invalid",
                ));
            }
        }
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        self.layer(&mut commands, "musica:/sys/configBase.png", 0, 0, 1.0)?;
        for (value, left, top) in [
            (config.message_speed_unread, 42, 159),
            (config.message_speed_read, 42, 235),
            (config.message_speed_auto_play, 42, 310),
            (config.bgm_volume, 578, 156),
            (config.voice_volume, 578, 231),
            (config.se_volume, 578, 305),
        ] {
            self.layer(
                &mut commands,
                "musica:/sys/knob.png",
                left + i32::from(value) * 2,
                top,
                1.0,
            )?;
        }
        let mut checks = vec![
            if config.preferred_play_mode == MusicaPlayMode::Auto {
                (40, 588)
            } else {
                (153, 588)
            },
            if config.fullscreen {
                (319, 120)
            } else {
                (319, 164)
            },
        ];
        for (enabled, position) in [
            (config.screen_effect, (319, 248)),
            (config.text_shadow, (319, 292)),
            (config.animation, (319, 336)),
            (config.backlog_voice_playback, (319, 424)),
            (config.stop_voice_at_next_message, (319, 476)),
            (config.progress_in_background, (319, 572)),
            (config.bgm_muted, (684, 117)),
            (config.voice_muted, (684, 193)),
            (config.se_muted, (684, 265)),
        ] {
            if enabled {
                checks.push(position);
            }
        }
        for (enabled, position) in config.character_voice_enabled.iter().zip([
            (575, 424),
            (575, 461),
            (575, 499),
            (575, 536),
            (696, 424),
        ]) {
            if *enabled {
                checks.push(position);
            }
        }
        for (left, top) in checks {
            self.layer(&mut commands, "musica:/sys/checkmark.png", left, top, 1.0)?;
        }
        if let Some((x, y)) = pointer {
            let left = if (592.0..648.0).contains(&x) && (600.0..640.0).contains(&y) {
                Some(584)
            } else if (701.0..775.0).contains(&x) && (600.0..640.0).contains(&y) {
                Some(701)
            } else {
                None
            };
            if let Some(left) = left {
                self.layer(&mut commands, "musica:/sys/circle.png", left, 584, 1.0)?;
            }
        }
        self.submit(commands)
    }
}
