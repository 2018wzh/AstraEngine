use super::*;
use crate::MusicaSystemPage;
impl Scene {
    fn gallery_asset(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        name: &str,
        position: [i32; 2],
        dimensions: Option<(u32, u32)>,
        opacity: f32,
    ) -> FamilyResult<()> {
        let uri = format!("musica:/sys/{name}");
        let frame = self.texture(&uri)?;
        if dimensions.is_some_and(|size| size != (frame.width, frame.height)) {
            return Err(error(
                "ASTRA_EMU_MUSICA_GALLERY_DIMENSIONS",
                "gallery resource dimensions are invalid",
            ));
        }
        self.layer(commands, &uri, position[0], position[1], opacity)
    }
    pub fn render_gallery(&mut self, page: &MusicaSystemPage, focus: u32) -> FamilyResult<()> {
        if crate::runtime::gallery::count(page).is_none_or(|count| focus >= count)
            || (self.width, self.height) != (1280, 720)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_GALLERY_STATE",
                "gallery page, focus or stage is invalid",
            ));
        }
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        match page {
            MusicaSystemPage::Memories | MusicaSystemPage::GalleryMovie => {
                self.gallery_asset(
                    &mut commands,
                    "memories.png",
                    [0, 0],
                    Some((1280, 720)),
                    1.0,
                )?;
                if *page == MusicaSystemPage::GalleryMovie {
                    commands.extend(
                        self.text
                            .gallery_movies(focus)
                            .map_err(|code| error(&code, "movie labels could not be rendered"))?,
                    );
                }
            }
            MusicaSystemPage::GalleryBgm => {
                self.gallery_asset(
                    &mut commands,
                    &format!("musicPage{}.png", focus / 16 + 1),
                    [0, 0],
                    Some((1280, 720)),
                    1.0,
                )?;
                self.gallery_asset(
                    &mut commands,
                    "musicNote.png",
                    [143, 96 + (focus % 16) as i32 * 32],
                    Some((32, 32)),
                    1.0,
                )?;
            }
            MusicaSystemPage::GalleryReplay => {
                self.gallery_asset(
                    &mut commands,
                    &format!("flash{focus}.png"),
                    [0, 0],
                    Some((1280, 720)),
                    1.0,
                )?;
                self.gallery_asset(
                    &mut commands,
                    &format!("flash{focus}menu.png"),
                    [0, 656],
                    Some((384, 64)),
                    1.0,
                )?;
            }
            MusicaSystemPage::GalleryCg => {
                for name in ["cgmode0.png", "cgmode0box.png"] {
                    self.gallery_asset(&mut commands, name, [0, 0], Some((1280, 720)), 1.0)?;
                }
                self.gallery_asset(
                    &mut commands,
                    &format!("cgpage{:03}.png", focus + 1),
                    [64, 48],
                    None,
                    1.0,
                )?;
                let mut thumbnails = self
                    .archive
                    .manifest()
                    .entries
                    .iter()
                    .filter(|e| {
                        e.uri.starts_with("musica:/sys/cgthumb/")
                            && e.uri.to_ascii_lowercase().ends_with(".png")
                    })
                    .map(|e| e.uri.clone())
                    .collect::<Vec<_>>();
                if thumbnails.len() > 256 {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_GALLERY_BOUND",
                        "CG thumbnail count exceeds the native limit",
                    ));
                }
                thumbnails.sort_unstable();
                for (index, uri) in thumbnails
                    .iter()
                    .skip(focus as usize * 16)
                    .take(16)
                    .enumerate()
                {
                    let name = uri.strip_prefix("musica:/sys/").ok_or_else(|| {
                        error("ASTRA_EMU_MUSICA_GALLERY_RESOURCE", "invalid thumbnail URI")
                    })?;
                    self.gallery_asset(
                        &mut commands,
                        name,
                        [65 + (index % 4) as i32 * 176, 97 + (index / 4) as i32 * 112],
                        Some((128, 72)),
                        0.4,
                    )?;
                }
                self.gallery_asset(&mut commands, "cgmode0menu.png", [560, 640], None, 1.0)?;
            }
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_GALLERY_STATE",
                    "invalid gallery page",
                ))
            }
        }
        self.submit(commands)
    }
}
