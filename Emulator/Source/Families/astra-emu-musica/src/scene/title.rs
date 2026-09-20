use super::*;
impl Scene {
    pub fn render_title(&mut self, variant: u8, focus: Option<u32>) -> FamilyResult<()> {
        if variant > 2 {
            return Err(error(
                "ASTRA_EMU_MUSICA_TITLE_VARIANT",
                "unsupported title variant",
            ));
        }
        let exists = |uri| match self.archive.stat(uri) {
            Ok(_) => Ok(true),
            Err(e) if e.code() == "ASTRA_EMU_VFS_NOT_FOUND" => Ok(false),
            Err(e) => Err(core_error(e)),
        };
        let (base, over, native_size) = if exists("musica:/sys/topmenu0.png")? {
            (
                format!("topmenu{variant}.png"),
                format!("topmenu{variant}over.png"),
                false,
            )
        } else if exists("musica:/sys/topmenu.png")? {
            let stem = if variant == 2 { "topmenu2" } else { "topmenu" };
            (format!("{stem}.png"), format!("{stem}Over.png"), false)
        } else {
            (
                format!("topMenu{variant}.png"),
                format!("topMenu{variant}Over.png"),
                true,
            )
        };
        let base = format!("musica:/sys/{base}");
        let asset = self.texture_asset(&base)?;
        if native_size
            && (asset.logical_extent.width, asset.logical_extent.height)
                != (self.width, self.height)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_TITLE_DIMENSIONS",
                "title resource does not match the native stage",
            ));
        }
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        self.layer(&mut commands, &base, 0, 0, 1.0)?;
        if let Some(focus) = focus {
            let top = *crate::runtime::title::rows(variant)
                .get(focus as usize)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_TITLE_FOCUS",
                        "title focus is outside the menu",
                    )
                })?;
            let over = format!("musica:/sys/{over}");
            let asset = self.texture_asset(&over)?;
            if native_size
                && (asset.logical_extent.width, asset.logical_extent.height)
                    != (self.width, self.height)
            {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TITLE_DIMENSIONS",
                    "title hover resource does not match the native stage",
                ));
            }
            commands.push(SceneCommand::PushClip {
                rect: RectI {
                    x: 1024,
                    y: top,
                    width: 256,
                    height: 48,
                },
            });
            self.layer(&mut commands, &over, 0, 0, 1.0)?;
            commands.push(SceneCommand::PopClip);
        }
        self.submit(commands)
    }
}
