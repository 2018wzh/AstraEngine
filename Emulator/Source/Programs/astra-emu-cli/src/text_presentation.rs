use std::collections::BTreeSet;

use astra_core::{DiagnosticSeverity, Hash256};
use astra_emu_family_api::{
    LegacyEphemeralText, LegacyTextHorizontalAlignmentV1, LegacyTextOutlineV1,
    LegacyTextPresentationV1, LegacyTextRegionV1,
};
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, MediaError, OverflowPolicy,
    PackagedFont, TextDirection, TextLayoutConfig, TextLayoutProvider, TextLayoutRequest,
    TextLayoutResult, TextRenderLayoutUpdate, TextRenderResourceOwner, TextRun, UnicodeRange,
    WrapPolicy,
};
use astra_media_core::{
    BlendMode, CpuRendererProvider, HeadlessRenderer, RectI, RenderTargetFormat,
    Renderer2DProvider, RendererCreateRequest, SceneCommand, TextureFrame, Transform2D,
};

const TEXT_PROVIDER_ID: &str = "cosmic_text_cpu";
const FONT_FAMILY: &str = "Noto Sans JP";
const FONT_ASSET_ID: &str = "asset:/font/emu/noto-sans-jp";

pub(crate) struct BoundTextPresenter {
    provider: CosmicTextLayoutProvider,
    resources: TextRenderResourceOwner,
    renderer: Option<(u32, u32, HeadlessRenderer)>,
    active_layout_ids: BTreeSet<String>,
}

#[derive(Debug)]
pub(crate) struct PresentedTextFrame {
    pub rgba8: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct PresentedTextOverlay {
    pub lifecycle: Vec<SceneCommand>,
    pub draws: Vec<SceneCommand>,
}

impl BoundTextPresenter {
    pub(crate) fn new(provider_id: &str, target: &str, profile: &str) -> Result<Self, String> {
        if provider_id != TEXT_PROVIDER_ID {
            return Err("ASTRA_EMU_HEADLESS_TEXT_PROVIDER_BINDING".into());
        }
        let bytes =
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
                .to_vec();
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: target.into(),
                profile: profile.into(),
                default_locale: "ja-JP".into(),
            },
            vec![PackagedFont {
                asset_id: FONT_ASSET_ID.into(),
                family: FONT_FAMILY.into(),
                face_index: 0,
                hash: Hash256::from_sha256(&bytes),
                license_id: "OFL-1.1".into(),
                subset: None,
                coverage: vec![
                    UnicodeRange {
                        start: 0x20,
                        end: 0x7e,
                    },
                    UnicodeRange {
                        start: 0x2015,
                        end: 0x2015,
                    },
                    UnicodeRange {
                        start: 0x201c,
                        end: 0x201d,
                    },
                    UnicodeRange {
                        start: 0x2026,
                        end: 0x2026,
                    },
                    UnicodeRange {
                        start: 0x266a,
                        end: 0x266a,
                    },
                    UnicodeRange {
                        start: 0x3000,
                        end: 0x30ff,
                    },
                    UnicodeRange {
                        start: 0x3400,
                        end: 0x9fff,
                    },
                    UnicodeRange {
                        start: 0xff00,
                        end: 0xffef,
                    },
                ],
                targets: vec![target.into()],
                profiles: vec![profile.into()],
                bytes,
            }],
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_PROVIDER_CREATE".to_owned())?;
        let identity = provider
            .identity()
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_PROVIDER_IDENTITY".to_owned())?;
        if identity.fonts.len() != 1
            || identity.fonts[0].asset_id != FONT_ASSET_ID
            || identity.fonts[0].family != FONT_FAMILY
        {
            return Err("ASTRA_EMU_HEADLESS_TEXT_PROVIDER_IDENTITY".into());
        }
        Ok(Self {
            provider,
            resources: TextRenderResourceOwner::default(),
            renderer: None,
            active_layout_ids: BTreeSet::new(),
        })
    }

    pub(crate) fn render(
        &mut self,
        underlay: &(u32, u32, Vec<u8>),
        text: &LegacyEphemeralText,
        presentation: &LegacyTextPresentationV1,
    ) -> Result<PresentedTextFrame, String> {
        presentation
            .validate()
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_PRESENTATION_INVALID".to_owned())?;
        let (width, height, rgba8) = underlay;
        validate_underlay(*width, *height, rgba8)?;
        validate_region(presentation.body, *width, *height)?;
        if let Some(speaker) = presentation.speaker {
            validate_region(speaker, *width, *height)?;
        }
        if presentation.font_families.as_slice() != [FONT_FAMILY] {
            return Err("ASTRA_EMU_HEADLESS_TEXT_FONT_BINDING".into());
        }

        let renderer = match &mut self.renderer {
            Some((bound_width, bound_height, renderer))
                if *bound_width == *width && *bound_height == *height =>
            {
                renderer
            }
            Some(_) => return Err("ASTRA_EMU_HEADLESS_TEXT_STAGE_IDENTITY".into()),
            slot @ None => {
                let renderer = CpuRendererProvider
                    .create(RendererCreateRequest {
                        width: *width,
                        height: *height,
                        format: RenderTargetFormat::Rgba8Srgb,
                        profile: "astra.emu.text.v1".into(),
                    })
                    .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RENDERER_CREATE".to_owned())?;
                let (_, _, renderer) = slot.insert((*width, *height, renderer));
                renderer
            }
        };

        let mut commands = vec![SceneCommand::Texture {
            id: "astra.emu.text.underlay".into(),
            frame: TextureFrame {
                width: *width,
                height: *height,
                rgba8: rgba8.clone().into(),
            },
            destination: RectI::new(0, 0, *width, *height),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        }];
        append_layout(
            &self.provider,
            &mut self.resources,
            &mut commands,
            &format!("{}.body", presentation.layout_id),
            &text.text,
            &presentation.language,
            &presentation.font_families,
            presentation.body,
            presentation.rgba,
            presentation.outline,
        )?;
        if let Some(region) = presentation.speaker {
            append_layout(
                &self.provider,
                &mut self.resources,
                &mut commands,
                &format!("{}.speaker", presentation.layout_id),
                text.speaker.as_deref().unwrap_or(""),
                &presentation.language,
                &presentation.font_families,
                region,
                presentation.rgba,
                presentation.outline,
            )?;
        }
        let frame = renderer
            .capture_frame(&commands)
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RENDER".to_owned())?;
        Ok(PresentedTextFrame { rgba8: frame.bytes })
    }

    #[cfg(test)]
    pub(crate) fn render_overlay(
        &mut self,
        stage_width: u32,
        stage_height: u32,
        text: &LegacyEphemeralText,
        presentation: &LegacyTextPresentationV1,
    ) -> Result<PresentedTextOverlay, String> {
        self.render_overlays(stage_width, stage_height, &[(text, presentation)])
    }

    pub(crate) fn render_overlays(
        &mut self,
        stage_width: u32,
        stage_height: u32,
        entries: &[(&LegacyEphemeralText, &LegacyTextPresentationV1)],
    ) -> Result<PresentedTextOverlay, String> {
        if entries.is_empty() || entries.len() > 16 {
            return Err("ASTRA_EMU_HEADLESS_TEXT_BATCH_BOUNDS".into());
        }
        let mut prepared = Vec::with_capacity(entries.len() * 2);
        for (text, presentation) in entries {
            presentation
                .validate()
                .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_PRESENTATION_INVALID".to_owned())?;
            validate_region(presentation.body, stage_width, stage_height)?;
            if let Some(speaker) = presentation.speaker {
                validate_region(speaker, stage_width, stage_height)?;
            }
            if presentation.font_families.as_slice() != [FONT_FAMILY] {
                return Err("ASTRA_EMU_HEADLESS_TEXT_FONT_BINDING".into());
            }
            let body_id = format!("{}.body", presentation.layout_id);
            let body_layout = layout_text(
                &self.provider,
                &body_id,
                &text.text,
                &presentation.language,
                &presentation.font_families,
                presentation.body,
            )?;
            prepared.push(PreparedTextLayout::new(
                body_id,
                body_layout,
                presentation.body,
                presentation.rgba,
                presentation.outline,
            )?);
            if let Some(region) = presentation.speaker {
                let speaker_id = format!("{}.speaker", presentation.layout_id);
                let speaker_layout = layout_text(
                    &self.provider,
                    &speaker_id,
                    text.speaker.as_deref().unwrap_or(""),
                    &presentation.language,
                    &presentation.font_families,
                    region,
                )?;
                prepared.push(PreparedTextLayout::new(
                    speaker_id,
                    speaker_layout,
                    region,
                    presentation.rgba,
                    presentation.outline,
                )?);
            }
        }
        let next_layout_ids = prepared
            .iter()
            .flat_map(|item| &item.layers)
            .map(|layer| layer.id.clone())
            .collect::<BTreeSet<_>>();
        let removals = self
            .active_layout_ids
            .difference(&next_layout_ids)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let updates = prepared
            .iter()
            .flat_map(|item| {
                item.layers.iter().map(|layer| TextRenderLayoutUpdate {
                    layout_id: &layer.id,
                    layout: &item.layout,
                    shared_layout: None,
                    rgba: layer.rgba,
                    translation: layer.translation,
                })
            })
            .collect::<Vec<_>>();
        let frame = self
            .resources
            .update_frame(&updates, &removals)
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RESOURCE".to_owned())?;
        self.active_layout_ids = next_layout_ids;
        Ok(PresentedTextOverlay {
            lifecycle: frame.lifecycle,
            draws: frame
                .layouts
                .into_iter()
                .flat_map(|layout| layout.commands)
                .collect(),
        })
    }

    pub(crate) fn clear_overlays(&mut self) -> Result<PresentedTextOverlay, String> {
        let removals = self
            .active_layout_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let frame = self
            .resources
            .update_frame(&[], &removals)
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RESOURCE".to_owned())?;
        self.active_layout_ids.clear();
        Ok(PresentedTextOverlay {
            lifecycle: frame.lifecycle,
            draws: Vec::new(),
        })
    }
}

struct PreparedTextLayout {
    layout: TextLayoutResult,
    layers: Vec<TextLayer>,
}

impl PreparedTextLayout {
    fn new(
        layout_id: String,
        layout: TextLayoutResult,
        region: LegacyTextRegionV1,
        rgba: [u8; 4],
        outline: Option<LegacyTextOutlineV1>,
    ) -> Result<Self, String> {
        let origin = aligned_origin(region, layout.width)?;
        Ok(Self {
            layout,
            layers: text_layers(&layout_id, origin, rgba, outline)?,
        })
    }
}

fn layout_text(
    provider: &CosmicTextLayoutProvider,
    layout_id: &str,
    text: &str,
    language: &str,
    font_families: &[String],
    region: LegacyTextRegionV1,
) -> Result<TextLayoutResult, String> {
    let layout = provider
        .layout(&TextLayoutRequest {
            key: layout_id.into(),
            runs: vec![TextRun {
                text: text.into(),
                language: language.into(),
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
            font_families: font_families.to_vec(),
            features: Vec::new(),
        })
        .map_err(redacted_text_layout_error)?;
    if layout.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
        )
    }) {
        return Err("ASTRA_EMU_HEADLESS_TEXT_LAYOUT_DIAGNOSTIC".into());
    }
    Ok(layout)
}

#[allow(clippy::too_many_arguments)]
fn append_layout(
    provider: &CosmicTextLayoutProvider,
    resources: &mut TextRenderResourceOwner,
    commands: &mut Vec<SceneCommand>,
    layout_id: &str,
    text: &str,
    language: &str,
    font_families: &[String],
    region: LegacyTextRegionV1,
    rgba: [u8; 4],
    outline: Option<LegacyTextOutlineV1>,
) -> Result<(), String> {
    let layout = layout_text(provider, layout_id, text, language, font_families, region)?;
    for layer in text_layers(
        layout_id,
        aligned_origin(region, layout.width)?,
        rgba,
        outline,
    )? {
        let resource_commands = resources
            .update_layout(&layer.id, &layout, layer.rgba)
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RESOURCE".to_owned())?;
        commands.push(SceneCommand::PushTransform {
            transform: Transform2D::translation(
                layer.translation.0 as f32,
                layer.translation.1 as f32,
            ),
        });
        commands.extend(resource_commands);
        commands.push(SceneCommand::PopTransform);
    }
    Ok(())
}

fn aligned_origin(region: LegacyTextRegionV1, layout_width: f32) -> Result<(i32, i32), String> {
    if !layout_width.is_finite() || layout_width < 0.0 {
        return Err("ASTRA_EMU_HEADLESS_TEXT_LAYOUT_BOUNDS".into());
    }
    let occupied = layout_width.ceil().min(region.width as f32) as i64;
    let remaining = i64::from(region.width)
        .checked_sub(occupied)
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXT_ALIGNMENT_BOUNDS".to_owned())?;
    let offset = match region.horizontal_alignment {
        LegacyTextHorizontalAlignmentV1::Start => 0,
        LegacyTextHorizontalAlignmentV1::Center => remaining / 2,
        LegacyTextHorizontalAlignmentV1::End => remaining,
    };
    let x = i64::from(region.x)
        .checked_add(offset)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXT_ALIGNMENT_BOUNDS".to_owned())?;
    Ok((x, region.y))
}

struct TextLayer {
    id: String,
    translation: (i32, i32),
    rgba: [u8; 4],
}

fn text_layers(
    layout_id: &str,
    translation: (i32, i32),
    rgba: [u8; 4],
    outline: Option<LegacyTextOutlineV1>,
) -> Result<Vec<TextLayer>, String> {
    let mut layers = Vec::new();
    if let Some(outline) = outline {
        let radius = i32::from(outline.radius);
        for y in -radius..=radius {
            for x in -radius..=radius {
                if (x == 0 && y == 0) || x * x + y * y > radius * radius {
                    continue;
                }
                layers.push(TextLayer {
                    id: format!("{layout_id}.outline.{x}.{y}"),
                    translation: (
                        translation
                            .0
                            .checked_add(x)
                            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXT_OUTLINE_BOUNDS".to_owned())?,
                        translation
                            .1
                            .checked_add(y)
                            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXT_OUTLINE_BOUNDS".to_owned())?,
                    ),
                    rgba: outline.rgba,
                });
            }
        }
    }
    layers.push(TextLayer {
        id: layout_id.to_owned(),
        translation,
        rgba,
    });
    Ok(layers)
}

fn redacted_text_layout_error(error: MediaError) -> String {
    let mut codes = match error {
        MediaError::Diagnostics(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .filter(|code| valid_diagnostic_code(code))
            .collect::<Vec<_>>(),
        MediaError::Message(message) => message
            .split_once(':')
            .map(|(code, _)| code)
            .filter(|code| valid_diagnostic_code(code))
            .map(str::to_owned)
            .into_iter()
            .collect(),
    };
    codes.sort();
    codes.dedup();
    if codes.is_empty() {
        codes.push("ASTRA_TEXT_PROVIDER_MESSAGE".into());
    }
    format!("ASTRA_EMU_HEADLESS_TEXT_LAYOUT:{}", codes.join(","))
}

fn valid_diagnostic_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 96
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn validate_underlay(width: u32, height: u32, rgba8: &[u8]) -> Result<(), String> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXT_STAGE_BOUNDS".to_owned())?;
    if width == 0 || height == 0 || rgba8.len() != expected {
        return Err("ASTRA_EMU_HEADLESS_TEXT_UNDERLAY_IDENTITY".into());
    }
    Ok(())
}

fn validate_region(
    region: LegacyTextRegionV1,
    stage_width: u32,
    stage_height: u32,
) -> Result<(), String> {
    let right = u64::try_from(region.x)
        .ok()
        .and_then(|x| x.checked_add(u64::from(region.width)));
    let bottom = u64::try_from(region.y)
        .ok()
        .and_then(|y| y.checked_add(u64::from(region.height)));
    if right.is_none_or(|right| right > u64::from(stage_width))
        || bottom.is_none_or(|bottom| bottom > u64::from(stage_height))
    {
        tracing::error!(
            target: "astra_emu_cli::text",
            event = "astra.emu.headless.text_region_bounds",
            region_x = region.x,
            region_y = region.y,
            region_width = region.width,
            region_height = region.height,
            stage_width,
            stage_height,
            "text presentation region exceeds the active scene"
        );
        return Err("ASTRA_EMU_HEADLESS_TEXT_REGION_BOUNDS".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presentation() -> LegacyTextPresentationV1 {
        LegacyTextPresentationV1 {
            layout_id: "test.message".into(),
            language: "ja-JP".into(),
            font_families: vec![FONT_FAMILY.into()],
            body: LegacyTextRegionV1 {
                x: 16,
                y: 48,
                width: 288,
                height: 64,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 2,
                horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
            },
            speaker: Some(LegacyTextRegionV1 {
                x: 16,
                y: 8,
                width: 288,
                height: 32,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 1,
                horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
            }),
            rgba: [255, 255, 255, 255],
            outline: Some(LegacyTextOutlineV1 {
                radius: 2,
                rgba: [0, 0, 0, 192],
            }),
        }
    }

    #[test]
    fn explicit_cosmic_text_binding_renders_japanese_deterministically() {
        let underlay = (320, 128, vec![0; 320 * 128 * 4]);
        let text = LegacyEphemeralText {
            lease_id: "lease.test".into(),
            text: "夏空―“”…♪".into(),
            speaker: Some("話者".into()),
        };
        let mut first =
            BoundTextPresenter::new(TEXT_PROVIDER_ID, "headless-test", "minori-v1").unwrap();
        let first = first.render(&underlay, &text, &presentation()).unwrap();
        let mut second =
            BoundTextPresenter::new(TEXT_PROVIDER_ID, "headless-test", "minori-v1").unwrap();
        let second = second.render(&underlay, &text, &presentation()).unwrap();
        assert_eq!(first.rgba8, second.rgba8);
        assert!(first.rgba8.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn gpu_overlay_separates_one_shot_glyph_lifecycle_from_retained_draws() {
        let text = LegacyEphemeralText {
            lease_id: "lease.overlay".into(),
            text: "夏空".into(),
            speaker: Some("話者".into()),
        };
        let mut presenter =
            BoundTextPresenter::new(TEXT_PROVIDER_ID, "headless-test", "minori-v1").unwrap();
        let first = presenter
            .render_overlay(320, 128, &text, &presentation())
            .unwrap();
        assert!(!first.lifecycle.is_empty());
        assert!(first.lifecycle.iter().all(|command| matches!(
            command,
            SceneCommand::UploadGlyph { .. } | SceneCommand::ReleaseResource { .. }
        )));
        assert!(first
            .draws
            .iter()
            .any(|command| matches!(command, SceneCommand::GlyphRun { .. })));
        assert!(first.draws.iter().any(|command| matches!(
            command,
            SceneCommand::GlyphRun {
                rgba: [0, 0, 0, 192],
                ..
            }
        )));
        assert!(first.draws.iter().any(|command| matches!(
            command,
            SceneCommand::GlyphRun {
                rgba: [255, 255, 255, 255],
                ..
            }
        )));

        let second = presenter
            .render_overlay(320, 128, &text, &presentation())
            .unwrap();
        assert!(second.lifecycle.is_empty());
        assert_eq!(first.draws, second.draws);
    }

    #[test]
    fn gpu_overlay_batches_centered_choice_rows_without_dropping_earlier_layouts() {
        let first_text = LegacyEphemeralText {
            lease_id: "lease.choice.0".into(),
            text: "最初".into(),
            speaker: None,
        };
        let second_text = LegacyEphemeralText {
            lease_id: "lease.choice.1".into(),
            text: "次".into(),
            speaker: None,
        };
        let mut first = presentation();
        first.layout_id = "test.choice.0".into();
        first.speaker = None;
        first.body = LegacyTextRegionV1 {
            x: 16,
            y: 16,
            width: 288,
            height: 32,
            font_size: 26.0,
            line_height: 30.0,
            max_lines: 1,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Center,
        };
        let mut second = first.clone();
        second.layout_id = "test.choice.1".into();
        second.body.y = 64;
        let mut presenter =
            BoundTextPresenter::new(TEXT_PROVIDER_ID, "headless-test", "minori-v1").unwrap();
        let overlay = presenter
            .render_overlays(320, 128, &[(&first_text, &first), (&second_text, &second)])
            .unwrap();
        let glyph_origins = overlay
            .draws
            .iter()
            .filter_map(|command| match command {
                SceneCommand::GlyphRun { glyphs, .. } => Some(glyphs.as_ref()),
                _ => None,
            })
            .flatten()
            .map(|glyph| (glyph.x, glyph.y))
            .collect::<Vec<_>>();
        assert!(glyph_origins.iter().any(|origin| origin.1 < 48));
        assert!(glyph_origins.iter().any(|origin| origin.1 >= 64));
        assert!(glyph_origins.iter().all(|origin| origin.0 > 16));

        let cleared = presenter.clear_overlays().unwrap();
        assert!(cleared.draws.is_empty());
        assert!(cleared
            .lifecycle
            .iter()
            .all(|command| matches!(command, SceneCommand::ReleaseResource { .. })));
        assert!(!cleared.lifecycle.is_empty());
        let cleared_again = presenter.clear_overlays().unwrap();
        assert!(cleared_again.lifecycle.is_empty());
    }

    #[test]
    fn verified_outline_keeps_white_text_visible_on_a_white_stage() {
        let underlay = (320, 128, vec![255; 320 * 128 * 4]);
        let text = LegacyEphemeralText {
            lease_id: "lease.outline".into(),
            text: "夏空".into(),
            speaker: None,
        };
        let mut presentation = presentation();
        presentation.speaker = None;
        let mut presenter =
            BoundTextPresenter::new(TEXT_PROVIDER_ID, "headless-test", "minori-v1").unwrap();
        let frame = presenter.render(&underlay, &text, &presentation).unwrap();
        assert!(frame.rgba8.chunks_exact(4).any(|pixel| {
            pixel[0] < 128 && pixel[1] < 128 && pixel[2] < 128 && pixel[3] == 255
        }));
    }

    #[test]
    fn missing_or_unknown_provider_and_out_of_bounds_layout_block() {
        assert!(BoundTextPresenter::new("", "headless-test", "minori-v1").is_err());
        let mut presenter =
            BoundTextPresenter::new(TEXT_PROVIDER_ID, "headless-test", "minori-v1").unwrap();
        let mut invalid = presentation();
        invalid.body.x = 319;
        assert_eq!(
            presenter
                .render(
                    &(320, 128, vec![0; 320 * 128 * 4]),
                    &LegacyEphemeralText {
                        lease_id: "lease.test".into(),
                        text: "本文".into(),
                        speaker: None,
                    },
                    &invalid,
                )
                .unwrap_err(),
            "ASTRA_EMU_HEADLESS_TEXT_REGION_BOUNDS"
        );
    }
}
