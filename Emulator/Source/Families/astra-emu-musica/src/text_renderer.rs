use astra_core::Hash256;
use astra_emu_sdk::{TextOutline, TextScene, TextSceneLayout};
use astra_media_core::SceneCommand;
use astra_text::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, PackagedFont,
    TextDirection, TextLayoutConfig, TextLayoutRequest, TextRun, WrapPolicy,
};

const FONT_FAMILY: &str = "Noto Sans JP";
const FONT_ASSET_ID: &str = "asset:/font/emu/noto-sans-jp";

pub struct MusicaTextRenderer {
    scene: TextScene,
    encoding: crate::ScriptEncoding,
    pub(crate) shadow: bool,
}

#[derive(Clone, Copy)]
struct Region {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    font_size: f32,
    line_height: f32,
    max_lines: u32,
}

impl MusicaTextRenderer {
    pub fn new(encoding: crate::ScriptEncoding) -> Result<Self, String> {
        let fonts = [
            (
                FONT_FAMILY,
                FONT_ASSET_ID,
                include_bytes!(
                    "../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf"
                )
                .as_slice(),
            ),
            (
                "Noto Sans SC",
                "asset:/font/emu/noto-sans-sc",
                include_bytes!(
                    "../../../../../Examples/NativeVN/Assets/Fonts/NotoSansSC-Variable.ttf"
                )
                .as_slice(),
            ),
        ]
        .into_iter()
        .map(|(family, asset_id, bytes)| {
            Ok(PackagedFont {
                asset_id: asset_id.into(),
                family: family.into(),
                face_index: 0,
                hash: Hash256::from_sha256(bytes),
                license_id: "OFL-1.1".into(),
                subset: None,
                coverage: astra_text::font_unicode_coverage(bytes, 0).map_err(text_error)?,
                targets: vec!["astra-emu-musica".into()],
                profiles: vec!["musica.reference".into()],
                bytes: bytes.to_vec(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: "astra-emu-musica".into(),
                profile: "musica.reference".into(),
                default_locale: "ja-JP".into(),
            },
            fonts,
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| "ASTRA_EMU_MUSICA_TEXT_PROVIDER_CREATE".to_owned())?;
        Ok(Self {
            scene: TextScene::new(provider),
            encoding,
            shadow: true,
        })
    }

    pub(crate) fn set_encoding(&mut self, encoding: crate::ScriptEncoding) {
        self.encoding = encoding;
    }

    fn frame(&mut self, regions: &mut [TextSceneLayout]) -> Result<Vec<SceneCommand>, String> {
        let (families, language, script) = match self.encoding {
            crate::ScriptEncoding::ShiftJis => ([FONT_FAMILY, "Noto Sans SC"], "ja-JP", "Jpan"),
            crate::ScriptEncoding::Gbk => (["Noto Sans SC", FONT_FAMILY], "zh-Hans", "Hani"),
        };
        for region in regions.iter_mut() {
            region.request.font_families = families.into_iter().map(str::to_owned).collect();
            for run in &mut region.request.runs {
                run.language = language.into();
                run.script = Some(script.into());
            }
        }
        self.scene.frame(regions).map_err(text_error)
    }

    pub(crate) fn save_cards(
        &mut self,
        cards: &[(u32, crate::storage::SaveCard)],
    ) -> Result<Vec<SceneCommand>, String> {
        let mut regions = cards
            .iter()
            .map(|(slot, card)| {
                let (x, y) = crate::scene::save_pages::slot_position(slot % 10);
                let text = if card.comment.is_empty() {
                    card.timestamp.clone()
                } else {
                    format!("{}\n{}", card.timestamp, card.comment)
                };
                let mut layout = layout_request(
                    &format!("musica.save.card.{slot}"),
                    &text,
                    Region {
                        x: x + 124,
                        y: y + 11,
                        width: 218,
                        height: 58,
                        font_size: 18.0,
                        line_height: 23.0,
                        max_lines: 2,
                    },
                );
                layout.rgba = [255, 0, 0, 255];
                layout
            })
            .collect::<Vec<_>>();
        self.frame(&mut regions)
    }

    pub fn commands(
        &mut self,
        message: Option<(&str, Option<&str>)>,
        choices: Option<(&[String], u32)>,
    ) -> Result<Vec<SceneCommand>, String> {
        if let Some((labels, selected)) = choices {
            let mut regions = labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    let mut layout = layout_request(
                        &format!("musica.choice.{index}"),
                        label,
                        choice_region(index),
                    );
                    layout.rgba = if index == selected as usize {
                        [255, 220, 96, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                    layout
                })
                .collect::<Vec<_>>();
            return self.frame(&mut regions);
        }
        let Some((text, speaker)) = message else {
            return Ok(self.scene.clear());
        };
        let mut regions = vec![layout_request(
            "musica.reference.message.body",
            text,
            Region {
                x: 160,
                y: 568,
                width: 960,
                height: 112,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 3,
            },
        )];
        if let Some(speaker) = speaker {
            regions.push(layout_request(
                "musica.reference.message.speaker",
                speaker,
                Region {
                    x: 160,
                    y: 528,
                    width: 960,
                    height: 32,
                    font_size: 26.0,
                    line_height: 32.0,
                    max_lines: 1,
                },
            ));
        }
        if self.shadow {
            for region in &mut regions {
                region.outline = Some(TextOutline {
                    radius: 2,
                    rgba: [0, 0, 0, 192],
                });
            }
        }
        self.frame(&mut regions)
    }
}

fn text_error(cause: astra_media_core::MediaError) -> String {
    match cause {
        astra_media_core::MediaError::Message(message) => {
            let code = message.split(':').next().unwrap_or("");
            if code.starts_with("ASTRA_")
                && code.len() <= 128
                && code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                code.to_owned()
            } else {
                "ASTRA_EMU_MUSICA_TEXT_RENDER".into()
            }
        }
        astra_media_core::MediaError::Diagnostics(_) => "ASTRA_EMU_MUSICA_TEXT_DIAGNOSTICS".into(),
    }
}

fn choice_region(index: usize) -> Region {
    Region {
        x: 240,
        y: 240 + index as i32 * 64,
        width: 800,
        height: 56,
        font_size: 28.0,
        line_height: 34.0,
        max_lines: 1,
    }
}

pub(crate) fn choice_at(count: usize, x: f32, y: f32) -> Option<u32> {
    if !(1..=4).contains(&count) || !x.is_finite() || !y.is_finite() {
        return None;
    }
    (0..count)
        .find(|index| {
            let region = choice_region(*index);
            x >= region.x as f32
                && x < region.x as f32 + region.width as f32
                && y >= region.y as f32
                && y < region.y as f32 + region.height as f32
        })
        .map(|index| index as u32)
}

fn layout_request(layout_id: &str, text: &str, region: Region) -> TextSceneLayout {
    TextSceneLayout {
        translation: (region.x, region.y),
        outline: None,
        rgba: [255, 255, 255, 255],
        request: TextLayoutRequest {
            key: layout_id.into(),
            runs: vec![TextRun {
                text: text.into(),
                language: "ja-JP".into(),
                script: Some("Jpan".into()),
                direction: TextDirection::LeftToRight,
                ruby: Vec::new(),
                voice: None,
            }],
            constraint: LayoutConstraint {
                max_width: region.width as f32,
                max_height: Some(region.height as f32),
                max_lines: Some(region.max_lines),
                font_size: region.font_size,
                line_height: region.line_height,
                wrap: WrapPolicy::WordOrGlyph,
                overflow: OverflowPolicy::Clip,
            },
            font_families: vec![FONT_FAMILY.into()],
            features: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_platform::SceneFrame;
    use astra_platform_common::WgpuOffscreenRenderer;

    #[test]
    fn packaged_font_accepts_japanese_punctuation_and_symbols() {
        let mut text = MusicaTextRenderer::new(crate::ScriptEncoding::ShiftJis).unwrap();
        text.commands(Some(("……――「テスト」！？ ♪", None)), None)
            .unwrap();
    }

    #[test]
    fn text_errors_preserve_codes_without_forwarding_messages() {
        use astra_media_core::MediaError;
        assert_eq!(
            text_error(MediaError::message(
                "ASTRA_TEXT_GLYPH_MISSING: private content"
            )),
            "ASTRA_TEXT_GLYPH_MISSING"
        );
        assert_eq!(
            text_error(MediaError::message("private content")),
            "ASTRA_EMU_MUSICA_TEXT_RENDER"
        );
    }

    #[test]
    fn choice_hit_testing_excludes_gaps_edges_and_invalid_coordinates() {
        assert_eq!(choice_at(2, 240.0, 240.0), Some(0));
        assert_eq!(choice_at(2, 1039.5, 295.5), Some(0));
        assert_eq!(choice_at(2, 240.0, 304.0), Some(1));
        for (x, y) in [
            (239.0, 250.0),
            (1040.0, 250.0),
            (250.0, 296.0),
            (250.0, 360.0),
            (f32::NAN, 250.0),
            (250.0, f32::INFINITY),
        ] {
            assert_eq!(choice_at(2, x, y), None);
        }
        assert_eq!(choice_at(0, 250.0, 250.0), None);
        assert_eq!(choice_at(5, 250.0, 250.0), None);
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_text_updates_shared_glyphs_and_releases_removed_regions() {
        let mut text = MusicaTextRenderer::new(crate::ScriptEncoding::ShiftJis).unwrap();
        let mut gpu = pollster::block_on(WgpuOffscreenRenderer::new()).unwrap();
        let mut sequence = 0;
        let mut draw = |message| {
            sequence += 1;
            gpu.render(&SceneFrame {
                sequence,
                width: 1280,
                height: 720,
                clear_rgba: [0, 0, 0, 0],
                commands: text.commands(message, None).unwrap(),
                semantics: None,
            })
            .unwrap()
            .rgba8
        };
        let first = draw(Some(("日本語の文字", Some("日本語"))));
        assert!(first.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
        assert_eq!(first, draw(Some(("日本語の文字", Some("日本語")))));
        let without_speaker = draw(Some(("日本語の文字", None)));
        assert_ne!(first, without_speaker);
        assert!(draw(None).iter().all(|byte| *byte == 0));
        assert_eq!(first, draw(Some(("日本語の文字", Some("日本語")))));
    }

    #[test]
    fn duplicate_region_failure_preserves_the_previous_glyph_residency() {
        let mut text = MusicaTextRenderer::new(crate::ScriptEncoding::ShiftJis).unwrap();
        text.commands(Some(("日本語", None)), None).unwrap();
        let region = || {
            layout_request(
                "duplicate",
                "文字",
                Region {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                    font_size: 24.0,
                    line_height: 30.0,
                    max_lines: 2,
                },
            )
        };
        assert!(text.scene.frame(&[region(), region()]).is_err());
        let commands = text.commands(Some(("日本語", None)), None).unwrap();
        assert!(!commands
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
        assert!(text
            .commands(None, None)
            .unwrap()
            .iter()
            .any(|command| matches!(command, SceneCommand::ReleaseResource { .. })));
    }
    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_shadow_toggle_reuses_glyphs_and_removes_outline_layers() {
        let mut text = MusicaTextRenderer::new(crate::ScriptEncoding::ShiftJis).unwrap();
        let mut gpu = pollster::block_on(WgpuOffscreenRenderer::new()).unwrap();
        let mut sequence = 0;
        let mut draw = |text: &mut MusicaTextRenderer, shadow| {
            text.shadow = shadow;
            sequence += 1;
            let commands = text
                .commands(Some(("Outline", Some("Name"))), None)
                .unwrap();
            if sequence > 1 {
                assert!(!commands.iter().any(|c| matches!(
                    c,
                    SceneCommand::UploadGlyph { .. } | SceneCommand::ReleaseResource { .. }
                )));
            }
            gpu.render(&SceneFrame {
                sequence,
                width: 1280,
                height: 720,
                clear_rgba: [0, 0, 0, 0],
                commands,
                semantics: None,
            })
            .unwrap()
            .rgba8
        };
        let outlined = draw(&mut text, true);
        let plain = draw(&mut text, false);
        let occupied = |rgba: &[u8]| rgba.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count();
        assert!(occupied(&outlined) > occupied(&plain));
        assert!(occupied(&plain) > 0);
        assert_eq!(outlined, draw(&mut text, true));
    }

    #[test]
    fn invalid_outline_rejects_before_mutating_glyph_ownership() {
        let mut text = MusicaTextRenderer::new(crate::ScriptEncoding::ShiftJis).unwrap();
        text.commands(Some(("Outline", None)), None).unwrap();
        for radius in [0, 9] {
            let mut region = layout_request("invalid", "Text", choice_region(0));
            region.outline = Some(TextOutline {
                radius,
                rgba: [0, 0, 0, 192],
            });
            assert!(text.scene.frame(&[region]).is_err());
        }
        let mut region = layout_request("overflow", "Text", choice_region(0));
        region.translation = (i32::MAX, i32::MAX);
        region.outline = Some(TextOutline {
            radius: 2,
            rgba: [0, 0, 0, 192],
        });
        assert!(text.scene.frame(&[region]).is_err());
        assert!(!text
            .commands(Some(("Outline", None)), None)
            .unwrap()
            .iter()
            .any(|c| matches!(c, SceneCommand::UploadGlyph { .. })));
    }
}
