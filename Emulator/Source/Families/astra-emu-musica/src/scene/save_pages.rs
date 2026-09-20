use super::*;
use crate::{storage::SaveCard, MusicaSystemPage};

impl Scene {
    pub fn render_save_page(
        &mut self,
        page: MusicaSystemPage,
        focus: u32,
        cards: &[(u32, SaveCard)],
    ) -> FamilyResult<()> {
        let title = match page {
            MusicaSystemPage::Save => "saveloadSave.png",
            MusicaSystemPage::Load => "saveloadLoad.png",
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_SAVE_PAGE",
                    "invalid save/load page",
                ))
            }
        };
        if focus >= crate::storage::SAVE_MAX_SLOTS || (self.width, self.height) != (1280, 720) {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_PAGE",
                "invalid save page stage or focus",
            ));
        }
        let page_asset = format!("saveload_Page{}.png", focus / 10);
        for (name, dimensions) in [
            ("saveloadBase.png", (1280, 720)),
            (title, (352, 48)),
            ("saveloadSelect.png", (344, 98)),
            (page_asset.as_str(), (208, 48)),
            ("saveloadButtons.png", (356, 48)),
            ("notsaved.png", (106, 60)),
        ] {
            let asset = self.texture_asset(&format!("musica:/sys/{name}"))?;
            if (asset.logical_extent.width, asset.logical_extent.height) != dimensions {
                return Err(error(
                    "ASTRA_EMU_MUSICA_SAVE_PAGE_DIMENSIONS",
                    "native save page resource dimensions are invalid",
                ));
            }
        }
        let mut commands = Vec::new();
        self.layer(&mut commands, "musica:/sys/saveloadBase.png", 0, 0, 1.0)?;
        self.layer(&mut commands, &format!("musica:/sys/{title}"), 64, 16, 1.0)?;
        if page == MusicaSystemPage::Save {
            commands.push(SceneCommand::PushClip {
                rect: RectI {
                    x: 578,
                    y: 656,
                    width: 240,
                    height: 48,
                },
            });
        }
        self.layer(
            &mut commands,
            "musica:/sys/saveloadButtons.png",
            462,
            656,
            1.0,
        )?;
        if page == MusicaSystemPage::Save {
            commands.push(SceneCommand::PopClip);
        }
        self.layer(
            &mut commands,
            &format!("musica:/sys/{page_asset}"),
            608,
            16,
            1.0,
        )?;
        for index in 0..10 {
            let (x, y) = slot_position(index);
            let slot = focus / 10 * 10 + index;
            if !cards.iter().any(|(id, _)| *id == slot) {
                self.layer(
                    &mut commands,
                    "musica:/sys/notsaved.png",
                    x + 4,
                    y + 18,
                    1.0,
                )?;
            }
        }
        let (x, y) = slot_position(focus % 10);
        self.layer(&mut commands, "musica:/sys/saveloadSelect.png", x, y, 1.0)?;
        for (slot, card) in cards {
            let (x, y) = slot_position(slot % 10);
            commands.push(SceneCommand::Texture {
                id: format!("musica.save.thumbnail.{slot}"),
                frame: card.texture()?,
                destination: RectI {
                    x: x + 10,
                    y: y + 15,
                    width: 96,
                    height: 54,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            });
        }
        let text = self
            .text
            .save_cards(cards)
            .map_err(|code| error(&code, "save card text could not be rendered"))?;
        self.append_text_commands(&mut commands, text);
        self.submit(commands)
    }
}
pub(crate) fn slot_position(index: u32) -> (i32, i32) {
    (
        [64, 456][(index % 2) as usize],
        [81, 189, 297, 405, 513][(index / 2) as usize],
    )
}
