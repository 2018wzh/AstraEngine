use super::*;
use astra_emu_sdk::{TextScene, TextSceneLayout};
use astra_text::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, PackagedFont,
    TextDirection, TextLayoutConfig, TextLayoutRequest, TextRun, WrapPolicy,
};
mod fonts;
mod paint;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct NativeTextRenderer {
    database: Option<fontdb::Database>,
    fonts: BTreeMap<String, PackagedFont>,
    scene: Option<TextScene>,
    pending: Vec<SceneCommand>,
    paint: paint::NativePaint,
    pub(super) default_font: Option<String>,
}

impl NativeTextRenderer {
    pub(super) fn is_initialized(&self) -> bool {
        self.scene.is_some()
    }
    pub(super) fn append(
        &self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &str,
    ) -> Result<(), CoreError> {
        // 940 writes this process-global boolean. Its native image initializer
        // is -1, so absence is proportional, not fixed-width.
        let proportional = vm
            .process_global_words
            .get(&0x004f_0580)
            .copied()
            .unwrap_or(u32::MAX)
            != 0;
        let surface = vm
            .native_text
            .get_mut(&slot)
            .ok_or_else(|| error("ASTRA_EMU_CMVS_TEXT_OBJECT"))?;
        let font = self
            .fonts
            .get(&surface.font)
            .ok_or_else(|| error("ASTRA_EMU_CMVS_FONT_BINDING"))?;
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| error("ASTRA_EMU_CMVS_FONT_BINDING"))?;
        let size = surface.size;
        let mut measure = |text: &str| {
            let request = request(
                format!(
                    "cmvs.measure.{}.{}.{}",
                    font.asset_id,
                    size,
                    text.chars().next().unwrap_or('\0') as u32
                ),
                text,
                size,
                &font.family,
            );
            let layout = scene
                .layout_shared(&request)
                .map_err(|_| error("ASTRA_EMU_CMVS_TEXT_SHAPING"))?;
            let advance = layout
                .shaped_runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .map(|glyph| glyph.advance)
                .sum::<f32>();
            if !advance.is_finite() || !(0.0..=4096.0).contains(&advance) {
                return Err(error("ASTRA_EMU_CMVS_TEXT_ADVANCE"));
            }
            Ok(advance.round() as u32)
        };
        crate::vm::append_native_static_text(
            surface,
            source,
            if proportional {
                Some(&mut measure)
            } else {
                None
            },
        )
    }
    pub(super) fn bind_font(&mut self, name: &str) -> Result<(), CoreError> {
        if self.fonts.contains_key(name) {
            return Ok(());
        }
        if self.fonts.len() >= 64 {
            return Err(error("ASTRA_EMU_CMVS_FONT_BOUND"));
        }
        let database = self.database.get_or_insert_with(|| {
            let mut database = fontdb::Database::new();
            database.load_system_fonts();
            database
        });
        let resolved = if name.is_empty() {
            self.default_font
                .as_deref()
                .ok_or_else(|| error("ASTRA_EMU_CMVS_DEFAULT_FONT_UNBOUND"))?
        } else {
            name
        };
        let font = fonts::load(database, resolved)?;
        let mut fonts = self.fonts.clone();
        fonts.insert(name.to_owned(), font);
        let unique = fonts
            .values()
            .map(|font| (font.asset_id.clone(), font.clone()))
            .collect::<BTreeMap<_, _>>();
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: "astra-emu-cmvs".into(),
                profile: "cmvs.native".into(),
                default_locale: "ja-JP".into(),
            },
            unique.into_values().collect(),
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| error("ASTRA_EMU_CMVS_FONT_PROVIDER"))?;
        if let Some(scene) = &mut self.scene {
            self.pending.extend(scene.clear());
        }
        self.scene = Some(TextScene::new(provider));
        self.fonts = fonts;
        Ok(())
    }

    pub(super) fn frame(&mut self, vm: &CmvsPs2aVmState) -> Result<Vec<SceneCommand>, CoreError> {
        let mut regions = Vec::new();
        let mut styles = BTreeMap::new();
        for (slot, surface) in &vm.native_text {
            for (index, glyph) in surface.glyphs.iter().enumerate() {
                let id = format!("cmvs.text.{slot}.{index}");
                let font = self
                    .fonts
                    .get(&glyph.font)
                    .ok_or_else(|| error("ASTRA_EMU_CMVS_FONT_BINDING"))?;
                let x = glyph.position[0]
                    .checked_add(surface.position[0])
                    .ok_or_else(|| error("ASTRA_EMU_CMVS_TEXT_GEOMETRY"))?;
                let y = glyph.position[1]
                    .checked_add(surface.position[1])
                    .ok_or_else(|| error("ASTRA_EMU_CMVS_TEXT_GEOMETRY"))?;
                regions.push(TextSceneLayout {
                    translation: (x, y),
                    rgba: [255; 4],
                    outline: None,
                    request: TextLayoutRequest {
                        key: id.clone(),
                        runs: vec![TextRun {
                            text: glyph.text.clone(),
                            language: "ja-JP".into(),
                            script: Some("Jpan".into()),
                            direction: TextDirection::LeftToRight,
                            ruby: Vec::new(),
                            voice: None,
                        }],
                        constraint: LayoutConstraint {
                            max_width: glyph.size as f32 * 4.0,
                            max_height: None,
                            max_lines: None,
                            font_size: glyph.size as f32,
                            line_height: glyph.size as f32,
                            wrap: WrapPolicy::None,
                            overflow: OverflowPolicy::Visible,
                        },
                        font_families: vec![font.family.clone()],
                        features: Vec::new(),
                    },
                });
                styles.insert(id, (glyph, [x, y]));
            }
        }
        let mut commands = std::mem::take(&mut self.pending);
        if let Some(scene) = &mut self.scene {
            commands.extend(
                scene
                    .frame(&regions)
                    .map_err(|_| error("ASTRA_EMU_CMVS_TEXT_SHAPING"))?,
            );
        } else if !regions.is_empty() {
            return Err(error("ASTRA_EMU_CMVS_FONT_BINDING"));
        }
        self.paint.project(commands, &styles)
    }
}

fn request(key: String, text: &str, size: u32, family: &str) -> TextLayoutRequest {
    TextLayoutRequest {
        key,
        runs: vec![TextRun {
            text: text.into(),
            language: "ja-JP".into(),
            script: Some("Jpan".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: size as f32 * 4.0,
            max_height: None,
            max_lines: None,
            font_size: size as f32,
            line_height: size as f32,
            wrap: WrapPolicy::None,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec![family.into()],
        features: Vec::new(),
    }
}
