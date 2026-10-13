use super::*;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("{0}")]
    Message(String),
    #[error("media diagnostics blocked")]
    Diagnostics(Vec<astra_core::Diagnostic>),
}

impl MediaError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RenderTargetFormat {
    Rgba8Srgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RendererDescriptor {
    pub provider_id: String,
    pub backend: String,
    pub headless: bool,
    pub packaged_eligible: bool,
    pub formats: Vec<RenderTargetFormat>,
    pub shader_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RendererCreateRequest {
    pub width: u32,
    pub height: u32,
    pub format: RenderTargetFormat,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CpuFrame {
    pub width: u32,
    pub height: u32,
    pub format: RenderTargetFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Alpha,
    Add,
    Opaque,
    Multiply,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RectI {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl RectI {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transform2D {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Transform2D {
    pub const IDENTITY: Self = Self {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub const fn translation(x: f32, y: f32) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }

    pub(super) fn then(self, next: Self) -> Self {
        Self {
            m11: next.m11 * self.m11 + next.m21 * self.m12,
            m12: next.m12 * self.m11 + next.m22 * self.m12,
            m21: next.m11 * self.m21 + next.m21 * self.m22,
            m22: next.m12 * self.m21 + next.m22 * self.m22,
            tx: next.m11 * self.tx + next.m21 * self.ty + next.tx,
            ty: next.m12 * self.tx + next.m22 * self.ty + next.ty,
        }
    }

    pub(super) fn inverse(self) -> Option<Self> {
        let determinant = self.m11 * self.m22 - self.m12 * self.m21;
        if !determinant.is_finite() || determinant.abs() < f32::EPSILON {
            return None;
        }
        let inverse = 1.0 / determinant;
        let m11 = self.m22 * inverse;
        let m12 = -self.m12 * inverse;
        let m21 = -self.m21 * inverse;
        let m22 = self.m11 * inverse;
        Some(Self {
            m11,
            m12,
            m21,
            m22,
            tx: -(m11 * self.tx + m21 * self.ty),
            ty: -(m12 * self.tx + m22 * self.ty),
        })
    }

    pub(super) fn point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.m11 * x + self.m21 * y + self.tx,
            self.m12 * x + self.m22 * y + self.ty,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureFrame {
    pub width: u32,
    pub height: u32,
    /// sRGBA8 texture bytes; material and blend mode define compositing semantics.
    pub rgba8: OwnedPixelBuffer,
}

/// Process-local texture ownership for live scene submission.
///
/// The `Vec` allocation is moved into the shared owner without moving its
/// bytes into an `Arc<[u8]>` allocation.  Cloning this value only clones the
/// owner reference, so the capture allocation remains the same allocation
/// until the final GPU upload consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPixelBuffer(pub(super) Arc<astra_byte_source::OwnedByteBuffer>);

impl OwnedPixelBuffer {
    pub fn from_vec(rgba8: Vec<u8>) -> Self {
        Self::from_owned(rgba8.into())
    }

    pub fn from_owned(rgba8: astra_byte_source::OwnedByteBuffer) -> Self {
        Self(Arc::new(rgba8))
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn allocation_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }

    pub fn make_mut_for_update(&mut self) -> &mut Vec<u8> {
        if Arc::get_mut(&mut self.0).is_none() {
            self.0 = Arc::new(astra_byte_source::OwnedByteBuffer::from_vec(
                self.as_slice().to_vec(),
            ));
        }
        Arc::get_mut(&mut self.0)
            .expect("unique owner was established")
            .make_mut_vec()
    }
}

impl From<Vec<u8>> for OwnedPixelBuffer {
    fn from(value: Vec<u8>) -> Self {
        Self::from_vec(value)
    }
}

/// Retain a GPU capture or other shared allocation without copying pixels.
/// A subsequent mutable update uses the existing copy-on-write path.
impl From<Arc<[u8]>> for OwnedPixelBuffer {
    fn from(value: Arc<[u8]>) -> Self {
        Self::from_owned(astra_byte_source::OwnedByteBuffer::from_owner(
            value,
            |bytes| bytes.as_ref(),
        ))
    }
}

impl Deref for OwnedPixelBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl TextureFrame {
    pub fn from_vec(width: u32, height: u32, rgba8: Vec<u8>) -> Result<Self, MediaError> {
        Self::from_buffer(width, height, OwnedPixelBuffer::from_vec(rgba8))
    }

    pub fn from_buffer(
        width: u32,
        height: u32,
        rgba8: OwnedPixelBuffer,
    ) -> Result<Self, MediaError> {
        validate_texture_dimensions(width, height, rgba8.len())?;
        Ok(Self {
            width,
            height,
            rgba8,
        })
    }

    pub fn validate_integrity(&self) -> Result<(), MediaError> {
        validate_texture(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GlyphBitmapFormat {
    Alpha8,
    Rgba8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    pub format: GlyphBitmapFormat,
    pub pixels: OwnedPixelBuffer,
}

impl GlyphBitmap {
    pub fn from_pixels(
        width: u32,
        height: u32,
        format: GlyphBitmapFormat,
        pixels: OwnedPixelBuffer,
    ) -> Result<Self, MediaError> {
        validate_glyph_dimensions(width, height, format, pixels.len())?;
        Ok(Self {
            width,
            height,
            format,
            pixels,
        })
    }

    pub fn from_vec(
        width: u32,
        height: u32,
        format: GlyphBitmapFormat,
        pixels: Vec<u8>,
    ) -> Result<Self, MediaError> {
        Self::from_pixels(width, height, format, OwnedPixelBuffer::from_vec(pixels))
    }

    pub fn validate_integrity(&self) -> Result<(), MediaError> {
        validate_glyph(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshVertex2D {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    /// Premultiplied sRGBA color.
    pub premultiplied_rgba: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeshMaterial2D {
    Solid,
    ColorTexture,
    GlyphMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextureFilter2D {
    Nearest,
    Linear,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneCompositing2D {
    #[default]
    LinearSrgb,
    EncodedSrgb,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshDraw2D {
    pub vertex_start: u32,
    pub vertex_count: u32,
    pub index_start: u32,
    pub index_count: u32,
    pub material: MeshMaterial2D,
    pub texture_id: Option<String>,
    pub texture_filter: TextureFilter2D,
    pub opacity: f32,
    pub blend: BlendMode,
    pub scissor: Option<RectI>,
}
