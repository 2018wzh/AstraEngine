use astra_core::Hash256;
use astra_emu_sdk::{TextScene, TextSceneLayout};
use astra_media_core::SceneCommand;
use astra_text::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, PackagedFont,
    TextDirection, TextLayoutConfig, TextLayoutRequest, TextRun, WrapPolicy,
};

const FONT_FAMILY: &str = "Noto Sans JP";
const FONT_ASSET_ID: &str = "asset:/font/emu/noto-sans-jp";

pub struct MinoriTextRenderer {
    scene: TextScene,
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

impl MinoriTextRenderer {
    pub fn new() -> Result<Self, String> {
        let bytes =
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
                .to_vec();
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: "astra-emu-minori".into(),
                profile: "minori.reference".into(),
                default_locale: "ja-JP".into(),
            },
            vec![PackagedFont {
                asset_id: FONT_ASSET_ID.into(),
                family: FONT_FAMILY.into(),
                face_index: 0,
                hash: Hash256::from_sha256(&bytes),
                license_id: "OFL-1.1".into(),
                subset: None,
                coverage: astra_text::font_unicode_coverage(&bytes, 0).map_err(text_error)?,
                targets: vec!["astra-emu-minori".into()],
                profiles: vec!["minori.reference".into()],
                bytes,
            }],
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| "ASTRA_EMU_MINORI_TEXT_PROVIDER_CREATE".to_owned())?;
        Ok(Self {
            scene: TextScene::new(provider),
        })
    }

    pub fn commands(
        &mut self,
        message: Option<(&str, Option<&str>)>,
        choices: Option<(&[String], u32)>,
    ) -> Result<Vec<SceneCommand>, String> {
        if let Some((labels, selected)) = choices {
            let regions = labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    let mut layout = layout_request(
                        &format!("minori.choice.{index}"),
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
            return self.scene.frame(&regions).map_err(text_error);
        }
        let Some((text, speaker)) = message else {
            return Ok(self.scene.clear());
        };
        let mut regions = vec![layout_request(
            "minori.reference.message.body",
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
                "minori.reference.message.speaker",
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
        self.scene.frame(&regions).map_err(text_error)
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
                "ASTRA_EMU_MINORI_TEXT_RENDER".into()
            }
        }
        astra_media_core::MediaError::Diagnostics(_) => "ASTRA_EMU_MINORI_TEXT_DIAGNOSTICS".into(),
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
        let mut text = MinoriTextRenderer::new().unwrap();
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
            "ASTRA_EMU_MINORI_TEXT_RENDER"
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
        let mut text = MinoriTextRenderer::new().unwrap();
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
        let mut text = MinoriTextRenderer::new().unwrap();
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
}
