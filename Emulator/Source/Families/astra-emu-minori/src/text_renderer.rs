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

pub(crate) struct MinoriTextRenderer {
    provider: CosmicTextLayoutProvider,
    resources: TextRenderResourceOwner,
    renderer: Option<(u32, u32, HeadlessRenderer)>,
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
    pub(crate) fn new() -> Result<Self, String> {
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
                targets: vec!["astra-emu-minori".into()],
                profiles: vec!["minori.reference".into()],
                bytes,
            }],
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| "ASTRA_EMU_MINORI_TEXT_PROVIDER_CREATE".to_owned())?;
        Ok(Self {
            provider,
            resources: TextRenderResourceOwner::default(),
            renderer: None,
        })
    }

    pub(crate) fn render(
        &mut self,
        width: u32,
        height: u32,
        text: &str,
        speaker: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let renderer = match &mut self.renderer {
            Some((bound_width, bound_height, renderer))
                if *bound_width == width && *bound_height == height =>
            {
                renderer
            }
            Some(_) => return Err("ASTRA_EMU_MINORI_TEXT_STAGE_CHANGED".into()),
            slot @ None => {
                let renderer = CpuRendererProvider
                    .create(RendererCreateRequest {
                        width,
                        height,
                        format: RenderTargetFormat::Rgba8Srgb,
                        profile: "astra.emu.minori.text.v1".into(),
                    })
                    .map_err(|_| "ASTRA_EMU_MINORI_TEXT_RENDERER_CREATE".to_owned())?;
                let (_, _, renderer) = slot.insert((width, height, renderer));
                renderer
            }
        };
        let mut commands = vec![SceneCommand::Clear { rgba: [0, 0, 0, 0] }];
        append_layout(
            &self.provider,
            &mut self.resources,
            &mut commands,
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
        )?;
        if let Some(speaker) = speaker {
            append_layout(
                &self.provider,
                &mut self.resources,
                &mut commands,
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
            )?;
        }
        renderer
            .capture_frame(&commands)
            .map(|frame| frame.bytes)
            .map_err(|_| "ASTRA_EMU_MINORI_TEXT_RENDER".to_owned())
    }
}

fn append_layout(
    provider: &CosmicTextLayoutProvider,
    resources: &mut TextRenderResourceOwner,
    commands: &mut Vec<SceneCommand>,
    layout_id: &str,
    text: &str,
    region: Region,
) -> Result<(), String> {
    let layout = provider
        .layout(&TextLayoutRequest {
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
        })
        .map_err(|_| "ASTRA_EMU_MINORI_TEXT_LAYOUT".to_owned())?;
    if layout.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
        )
    }) {
        return Err("ASTRA_EMU_MINORI_TEXT_LAYOUT_DIAGNOSTIC".into());
    }
    commands.push(SceneCommand::PushTransform {
        transform: Transform2D::translation(region.x as f32, region.y as f32),
    });
    commands.extend(
        resources
            .update_layout(layout_id, &layout, [255, 255, 255, 255])
            .map_err(|_| "ASTRA_EMU_MINORI_TEXT_RESOURCE".to_owned())?,
    );
    commands.push(SceneCommand::PopTransform);
    Ok(())
}
