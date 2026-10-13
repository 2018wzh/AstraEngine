use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum SceneCommand {
    UploadTexture {
        resource_id: String,
        frame: TextureFrame,
    },
    UpdateTextureRegion {
        resource_id: String,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba8: OwnedPixelBuffer,
    },
    UploadGlyph {
        resource_id: String,
        glyph: GlyphBitmap,
    },
    ReleaseResource {
        resource_id: String,
    },
    Sprite {
        id: String,
        texture_id: String,
        source: Option<RectI>,
        destination: RectI,
        opacity: f32,
        blend: BlendMode,
    },
    GlyphRun {
        id: String,
        glyphs: Arc<[GlyphInstance]>,
        rgba: [u8; 4],
        opacity: f32,
        blend: BlendMode,
    },
    Mesh2D {
        id: String,
        vertices: Arc<[MeshVertex2D]>,
        indices: Arc<[u32]>,
        material: MeshMaterial2D,
        texture_id: Option<String>,
        texture_filter: TextureFilter2D,
        opacity: f32,
        blend: BlendMode,
    },
    MeshBatch2D {
        vertices: Arc<[MeshVertex2D]>,
        indices: Arc<[u32]>,
        draws: Arc<[MeshDraw2D]>,
        compositing: SceneCompositing2D,
    },
    Clear {
        rgba: [u8; 4],
    },
    Rect {
        id: String,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: [u8; 4],
    },
    Texture {
        id: String,
        frame: TextureFrame,
        destination: RectI,
        opacity: f32,
        blend: BlendMode,
    },
    VideoFrame {
        id: String,
        frame: TextureFrame,
        destination: RectI,
        opacity: f32,
        blend: BlendMode,
        presentation_time_ns: u64,
    },
    Glyph {
        id: String,
        glyph: GlyphBitmap,
        x: i32,
        y: i32,
        rgba: [u8; 4],
        opacity: f32,
        blend: BlendMode,
    },
    PushClip {
        rect: RectI,
    },
    PopClip,
    PushTransform {
        transform: Transform2D,
    },
    PopTransform,
    SetCamera {
        transform: Transform2D,
    },
    PushOpacity {
        opacity: f32,
    },
    PopOpacity,
    FilterGraph {
        graph: crate::FilterGraph,
    },
    /// Intersect a screen-space, repeating 8x8 binary coverage mask.
    /// Bit 63 covers pixel (0, 0), bit 0 covers (7, 7).
    PushPixelMask {
        bits: u64,
    },
    PopPixelMask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlyphInstance {
    pub resource_id: String,
    pub x: i32,
    pub y: i32,
    pub rotation_quadrants: u8,
}

pub type DrawCommand = SceneCommand;

impl SceneCommand {
    pub fn clear(rgba: [u8; 4]) -> Self {
        Self::Clear { rgba }
    }

    pub fn rect(
        id: impl Into<String>,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: [u8; 4],
    ) -> Self {
        Self::Rect {
            id: id.into(),
            x,
            y,
            width,
            height,
            rgba,
        }
    }
}

pub trait Renderer2DProvider {
    type Renderer: Renderer2D;

    fn descriptor(&self) -> RendererDescriptor;
    fn create(&self, request: RendererCreateRequest) -> Result<Self::Renderer, MediaError>;
}

pub trait Renderer2D {
    fn capture_frame(&mut self, commands: &[DrawCommand]) -> Result<CpuFrame, MediaError>;

    fn capture_hash(&mut self, commands: &[DrawCommand]) -> Result<Hash256, MediaError> {
        let frame = self.capture_frame(commands)?;
        Ok(frame_hash(
            frame.width,
            frame.height,
            frame.format,
            &frame.bytes,
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CpuRendererProvider;

impl Renderer2DProvider for CpuRendererProvider {
    type Renderer = HeadlessRenderer;

    fn descriptor(&self) -> RendererDescriptor {
        RendererDescriptor {
            provider_id: "astra.renderer.headless".to_string(),
            backend: "headless-cpu".to_string(),
            headless: true,
            packaged_eligible: true,
            formats: vec![RenderTargetFormat::Rgba8Srgb],
            shader_model: "deterministic-cpu".to_string(),
        }
    }

    fn create(&self, request: RendererCreateRequest) -> Result<Self::Renderer, MediaError> {
        if request.width == 0 || request.height == 0 {
            return Err(MediaError::message("render target must be non-empty"));
        }
        Ok(HeadlessRenderer {
            request,
            textures: BTreeMap::new(),
            glyphs: BTreeMap::new(),
            texture_ids: HashSet::new(),
            glyph_ids: HashSet::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HeadlessRenderer {
    pub(super) request: RendererCreateRequest,
    pub(super) textures: BTreeMap<String, TextureFrame>,
    pub(super) glyphs: BTreeMap<String, GlyphBitmap>,
    pub(super) texture_ids: HashSet<String>,
    pub(super) glyph_ids: HashSet<String>,
}
