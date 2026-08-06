use astra_core::{DiagnosticSeverity, Hash256};
use astra_emu_family_api::{LegacyEphemeralText, LegacyTextPresentationV1, LegacyTextRegionV1};
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, PackagedFont,
    TextDirection, TextLayoutConfig, TextLayoutProvider, TextLayoutRequest,
    TextRenderResourceOwner, TextRun, UnicodeRange, WrapPolicy,
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
}

#[derive(Debug)]
pub(crate) struct PresentedTextFrame {
    pub rgba8: Vec<u8>,
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
                hash: Hash256::from_sha256(rgba8),
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
            )?;
        }
        let frame = renderer
            .capture_frame(&commands)
            .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RENDER".to_owned())?;
        Ok(PresentedTextFrame { rgba8: frame.bytes })
    }
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
) -> Result<(), String> {
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
        .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_LAYOUT".to_owned())?;
    if layout.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
        )
    }) {
        return Err("ASTRA_EMU_HEADLESS_TEXT_LAYOUT_DIAGNOSTIC".into());
    }
    let resource_commands = resources
        .update_layout(layout_id, &layout, rgba)
        .map_err(|_| "ASTRA_EMU_HEADLESS_TEXT_RESOURCE".to_owned())?;
    commands.push(SceneCommand::PushTransform {
        transform: Transform2D::translation(region.x as f32, region.y as f32),
    });
    commands.extend(resource_commands);
    commands.push(SceneCommand::PopTransform);
    Ok(())
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
            },
            speaker: Some(LegacyTextRegionV1 {
                x: 16,
                y: 8,
                width: 288,
                height: 32,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 1,
            }),
            rgba: [255, 255, 255, 255],
        }
    }

    #[test]
    fn explicit_cosmic_text_binding_renders_japanese_deterministically() {
        let underlay = (320, 128, vec![0; 320 * 128 * 4]);
        let text = LegacyEphemeralText {
            lease_id: "lease.test".into(),
            text: "夏空".into(),
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
