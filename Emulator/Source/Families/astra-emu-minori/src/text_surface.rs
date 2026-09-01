use astra_core::{DiagnosticSeverity, Hash256};
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, PackagedFont,
    TextDirection, TextLayoutConfig, TextLayoutProvider, TextLayoutRequest,
    TextRenderResourceOwner, TextRun, UnicodeRange, WrapPolicy,
};
use astra_media_core::{
    CpuRendererProvider, HeadlessRenderer, RenderTargetFormat, Renderer2DProvider,
    RendererCreateRequest, SceneCommand, Transform2D,
};

const FONT_FAMILY: &str = "Noto Sans JP";
const FONT_ASSET_ID: &str = "asset:/font/emu/noto-sans-jp";
/// The original Minori message panel draws a small, independent downward
/// triangle after the completed message.  It is a presentation marker, not
/// part of the message source or backlog text.
const ADVANCE_INDICATOR: &str = "▼";

#[derive(Debug, Clone, Copy)]
pub(super) enum TextAlignment {
    Start,
    Center,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TextRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub line_height: f32,
    pub max_lines: u32,
    pub alignment: TextAlignment,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TextOutline {
    pub radius: u32,
    pub rgba: [u8; 4],
}

pub(super) struct TextSurfaceRequest {
    pub key: String,
    pub text: String,
    pub speaker: Option<String>,
    pub show_advance_indicator: bool,
    pub body: TextRegion,
    pub speaker_region: Option<TextRegion>,
    pub rgba: [u8; 4],
    pub outline: Option<TextOutline>,
}

pub(super) struct MinoriTextSurfaceRenderer {
    provider: CosmicTextLayoutProvider,
    renderer: HeadlessRenderer,
    resources: TextRenderResourceOwner,
    width: u32,
    height: u32,
}

impl MinoriTextSurfaceRenderer {
    pub(super) fn new(width: u32, height: u32) -> Result<Self, &'static str> {
        let bytes =
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
                .to_vec();
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: "astra-emu-minori".into(),
                profile: "minori-v1".into(),
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
                        start: 0x2010,
                        end: 0x2027,
                    },
                    UnicodeRange {
                        start: 0x25bc,
                        end: 0x25bc,
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
                targets: vec!["astra-emu-minori".into()],
                profiles: vec!["minori-v1".into()],
                bytes,
            }],
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| "ASTRA_EMU_MINORI_TEXT_PROVIDER_CREATE")?;
        let identity = provider
            .identity()
            .map_err(|_| "ASTRA_EMU_MINORI_TEXT_PROVIDER_IDENTITY")?;
        if identity.fonts.len() != 1
            || identity.fonts[0].asset_id != FONT_ASSET_ID
            || identity.fonts[0].family != FONT_FAMILY
        {
            return Err("ASTRA_EMU_MINORI_TEXT_PROVIDER_IDENTITY");
        }
        let renderer = CpuRendererProvider
            .create(RendererCreateRequest {
                width,
                height,
                format: RenderTargetFormat::Rgba8Srgb,
                profile: "astra.emu.minori.text_surface.v1".into(),
            })
            .map_err(|_| "ASTRA_EMU_MINORI_TEXT_RENDERER_CREATE")?;
        Ok(Self {
            provider,
            renderer,
            resources: TextRenderResourceOwner::default(),
            width,
            height,
        })
    }

    pub(super) fn render(
        &mut self,
        requests: &[TextSurfaceRequest],
    ) -> Result<Vec<u8>, &'static str> {
        if requests.is_empty() || requests.len() > 16 {
            return Err("ASTRA_EMU_MINORI_TEXT_BATCH_BOUNDS");
        }
        let mut commands = vec![SceneCommand::Clear { rgba: [0, 0, 0, 0] }];
        for request in requests {
            validate_region(request.body, self.width, self.height)?;
            if let Some(region) = request.speaker_region {
                validate_region(region, self.width, self.height)?;
            }
            append_text(
                &self.provider,
                &mut self.resources,
                &mut commands,
                &format!("{}.body", request.key),
                &request.text,
                request.body,
                request.rgba,
                request.outline,
                request.show_advance_indicator,
            )?;
            if let (Some(speaker), Some(region)) =
                (request.speaker.as_deref(), request.speaker_region)
            {
                append_text(
                    &self.provider,
                    &mut self.resources,
                    &mut commands,
                    &format!("{}.speaker", request.key),
                    speaker,
                    region,
                    request.rgba,
                    request.outline,
                    false,
                )?;
            }
        }
        let mut frame = self
            .renderer
            .capture_frame(&commands)
            .map_err(|error| {
                tracing::error!(
                    event = "astra_emu_minori_text_surface_render_failed",
                    diagnostic_code = "ASTRA_EMU_MINORI_TEXT_RENDER",
                    error = %error,
                    command_count = commands.len(),
                    "Minori text surface renderer rejected the typed command stream"
                );
                "ASTRA_EMU_MINORI_TEXT_RENDER"
            })?
            .bytes;
        for pixel in frame.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        Ok(frame)
    }
}

#[allow(clippy::too_many_arguments)]
fn append_text(
    provider: &CosmicTextLayoutProvider,
    owner: &mut TextRenderResourceOwner,
    commands: &mut Vec<SceneCommand>,
    layout_id: &str,
    text: &str,
    region: TextRegion,
    rgba: [u8; 4],
    outline: Option<TextOutline>,
    show_advance_indicator: bool,
) -> Result<(), &'static str> {
    let runs = vec![TextRun {
        text: text.into(),
        language: "ja-JP".into(),
        script: Some("Jpan".into()),
        direction: TextDirection::LeftToRight,
        ruby: Vec::new(),
        voice: None,
    }];
    let layout = provider
        .layout(&TextLayoutRequest {
            key: layout_id.into(),
            runs,
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
        })
        .map_err(|_| "ASTRA_EMU_MINORI_TEXT_LAYOUT")?;
    if layout.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
        )
    }) {
        return Err("ASTRA_EMU_MINORI_TEXT_LAYOUT_DIAGNOSTIC");
    }
    let origin_x = text_origin_x(&layout, region)?;
    append_layout_layers(
        owner, commands, layout_id, &layout, origin_x, region.y, rgba, outline,
    )?;
    if show_advance_indicator {
        let indicator_layout = provider
            .layout(&TextLayoutRequest {
                key: format!("{layout_id}.indicator"),
                runs: vec![TextRun {
                    text: ADVANCE_INDICATOR.into(),
                    language: "ja-JP".into(),
                    script: Some("Jpan".into()),
                    direction: TextDirection::LeftToRight,
                    ruby: Vec::new(),
                    voice: None,
                }],
                constraint: LayoutConstraint {
                    max_width: region.width as f32,
                    max_height: Some(region.height as f32),
                    max_lines: Some(1),
                    font_size: region.font_size,
                    line_height: region.line_height,
                    wrap: WrapPolicy::None,
                    overflow: OverflowPolicy::Clip,
                },
                font_families: vec![FONT_FAMILY.into()],
                features: Vec::new(),
            })
            .map_err(|_| "ASTRA_EMU_MINORI_TEXT_LAYOUT")?;
        if indicator_layout.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
            )
        }) {
            return Err("ASTRA_EMU_MINORI_TEXT_LAYOUT_DIAGNOSTIC");
        }
        let (indicator_x, indicator_y) =
            advance_indicator_origin(&layout, indicator_layout.width, region, origin_x)?;
        append_layout_layers(
            owner,
            commands,
            &format!("{layout_id}.indicator"),
            &indicator_layout,
            indicator_x,
            indicator_y,
            rgba,
            outline,
        )?;
    }
    Ok(())
}

fn text_origin_x(
    layout: &astra_media::TextLayoutResult,
    region: TextRegion,
) -> Result<i32, &'static str> {
    let occupied = layout.width.ceil().min(region.width as f32) as i64;
    let remaining = i64::from(region.width)
        .checked_sub(occupied)
        .ok_or("ASTRA_EMU_MINORI_TEXT_ALIGNMENT")?;
    let offset = match region.alignment {
        TextAlignment::Start => 0,
        TextAlignment::Center => remaining / 2,
    };
    i64::from(region.x)
        .checked_add(offset)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or("ASTRA_EMU_MINORI_TEXT_ALIGNMENT")
}

fn advance_indicator_origin(
    body_layout: &astra_media::TextLayoutResult,
    indicator_width: f32,
    region: TextRegion,
    origin_x: i32,
) -> Result<(i32, i32), &'static str> {
    if !indicator_width.is_finite() || indicator_width <= 0.0 {
        return Err("ASTRA_EMU_MINORI_TEXT_INDICATOR_LAYOUT");
    }
    let last_line = body_layout
        .lines
        .iter()
        .filter(|line| line.run_index == 0)
        .max_by_key(|line| line.line);
    let (line, line_top, line_width) = last_line
        .map(|line| (line.line, line.top, line.width))
        .unwrap_or((0, 0.0, 0.0));
    if !line_top.is_finite() || !line_width.is_finite() || line_top < 0.0 || line_width < 0.0 {
        return Err("ASTRA_EMU_MINORI_TEXT_INDICATOR_LAYOUT");
    }
    let mut local_x = line_width;
    let mut local_y = line_top;
    if line_width + indicator_width > region.width as f32 {
        let next_line = line
            .checked_add(1)
            .ok_or("ASTRA_EMU_MINORI_TEXT_INDICATOR_LAYOUT")?;
        if next_line >= region.max_lines {
            return Err("ASTRA_EMU_MINORI_TEXT_INDICATOR_LAYOUT");
        }
        local_x = 0.0;
        local_y = next_line as f32 * region.line_height;
    }
    let x = (i64::from(origin_x) as f32 + local_x).round();
    let y = (i64::from(region.y) as f32 + local_y).round();
    let x = i32::try_from(x as i64).map_err(|_| "ASTRA_EMU_MINORI_TEXT_INDICATOR_LAYOUT")?;
    let y = i32::try_from(y as i64).map_err(|_| "ASTRA_EMU_MINORI_TEXT_INDICATOR_LAYOUT")?;
    Ok((x, y))
}

#[allow(clippy::too_many_arguments)]
fn append_layout_layers(
    owner: &mut TextRenderResourceOwner,
    commands: &mut Vec<SceneCommand>,
    layout_id: &str,
    layout: &astra_media::TextLayoutResult,
    origin_x: i32,
    origin_y: i32,
    rgba: [u8; 4],
    outline: Option<TextOutline>,
) -> Result<(), &'static str> {
    let mut layers = Vec::new();
    if let Some(outline) = outline {
        let radius =
            i32::try_from(outline.radius).map_err(|_| "ASTRA_EMU_MINORI_TEXT_OUTLINE_BOUNDS")?;
        for y in -radius..=radius {
            for x in -radius..=radius {
                if (x == 0 && y == 0) || x * x + y * y > radius * radius {
                    continue;
                }
                layers.push((
                    format!("{layout_id}.outline.{x}.{y}"),
                    origin_x
                        .checked_add(x)
                        .ok_or("ASTRA_EMU_MINORI_TEXT_OUTLINE_BOUNDS")?,
                    origin_y
                        .checked_add(y)
                        .ok_or("ASTRA_EMU_MINORI_TEXT_OUTLINE_BOUNDS")?,
                    outline.rgba,
                ));
            }
        }
    }
    layers.push((layout_id.to_owned(), origin_x, origin_y, rgba));
    for (id, x, y, color) in layers {
        let mut layout_commands = owner
            .update_layout(&id, layout, color)
            .map_err(|_| "ASTRA_EMU_MINORI_TEXT_RESOURCE")?;
        commands.push(SceneCommand::PushTransform {
            transform: Transform2D::translation(x as f32, y as f32),
        });
        commands.append(&mut layout_commands);
        commands.push(SceneCommand::PopTransform);
    }
    Ok(())
}

fn validate_region(region: TextRegion, width: u32, height: u32) -> Result<(), &'static str> {
    let right = u64::try_from(region.x)
        .ok()
        .and_then(|x| x.checked_add(u64::from(region.width)));
    let bottom = u64::try_from(region.y)
        .ok()
        .and_then(|y| y.checked_add(u64::from(region.height)));
    if right.is_none_or(|right| right > u64::from(width))
        || bottom.is_none_or(|bottom| bottom > u64::from(height))
        || region.width == 0
        || region.height == 0
        || region.max_lines == 0
        || !region.font_size.is_finite()
        || !region.line_height.is_finite()
        || region.font_size <= 0.0
        || region.line_height < region.font_size
    {
        return Err("ASTRA_EMU_MINORI_TEXT_REGION_BOUNDS");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_text_frames_reuse_retained_glyph_resources() {
        let mut renderer = MinoriTextSurfaceRenderer::new(1280, 720).unwrap();
        let request = TextSurfaceRequest {
            key: "minori.test.message".into(),
            text: "日本語テキスト".into(),
            speaker: Some("話者".into()),
            show_advance_indicator: true,
            body: TextRegion {
                x: 160,
                y: 568,
                width: 960,
                height: 112,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 3,
                alignment: TextAlignment::Start,
            },
            speaker_region: Some(TextRegion {
                x: 160,
                y: 528,
                width: 960,
                height: 32,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 1,
                alignment: TextAlignment::Start,
            }),
            rgba: [255, 255, 255, 255],
            outline: Some(TextOutline {
                radius: 2,
                rgba: [0, 0, 0, 192],
            }),
        };

        let first = renderer.render(std::slice::from_ref(&request)).unwrap();
        let second = renderer.render(&[request]).unwrap();
        assert_eq!(first.len(), 1280 * 720 * 4);
        assert_eq!(second.len(), first.len());
    }

    #[test]
    fn advance_indicator_is_rendered_without_mutating_message_text() {
        let mut renderer = MinoriTextSurfaceRenderer::new(1280, 720).unwrap();
        let mut request = TextSurfaceRequest {
            key: "minori.test.indicator".into(),
            text: "本文".into(),
            speaker: None,
            show_advance_indicator: false,
            body: TextRegion {
                x: 160,
                y: 568,
                width: 960,
                height: 112,
                font_size: 26.0,
                line_height: 32.0,
                max_lines: 3,
                alignment: TextAlignment::Start,
            },
            speaker_region: None,
            rgba: [255, 255, 255, 255],
            outline: Some(TextOutline {
                radius: 2,
                rgba: [0, 0, 0, 192],
            }),
        };
        let without = renderer.render(std::slice::from_ref(&request)).unwrap();
        request.show_advance_indicator = true;
        let with = renderer.render(&[request]).unwrap();
        assert_ne!(with, without);
    }

    #[test]
    fn advance_indicator_stays_inline_when_the_last_line_has_room() {
        let renderer = MinoriTextSurfaceRenderer::new(1280, 720).unwrap();
        let region = TextRegion {
            x: 160,
            y: 568,
            width: 960,
            height: 112,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 3,
            alignment: TextAlignment::Start,
        };
        let body_layout = renderer
            .provider
            .layout(&TextLayoutRequest {
                key: "minori.test.indicator-placement".into(),
                runs: vec![TextRun {
                    text: "本文".into(),
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
            })
            .unwrap();
        let indicator_layout = renderer
            .provider
            .layout(&TextLayoutRequest {
                key: "minori.test.indicator-placement.marker".into(),
                runs: vec![TextRun {
                    text: ADVANCE_INDICATOR.into(),
                    language: "ja-JP".into(),
                    script: Some("Jpan".into()),
                    direction: TextDirection::LeftToRight,
                    ruby: Vec::new(),
                    voice: None,
                }],
                constraint: LayoutConstraint {
                    max_width: region.width as f32,
                    max_height: Some(region.height as f32),
                    max_lines: Some(1),
                    font_size: region.font_size,
                    line_height: region.line_height,
                    wrap: WrapPolicy::None,
                    overflow: OverflowPolicy::Clip,
                },
                font_families: vec![FONT_FAMILY.into()],
                features: Vec::new(),
            })
            .unwrap();
        let (x, y) = advance_indicator_origin(
            &body_layout,
            indicator_layout.width,
            region,
            text_origin_x(&body_layout, region).unwrap(),
        )
        .unwrap();
        assert!(x > region.x);
        assert_eq!(y, region.y);
    }
}
