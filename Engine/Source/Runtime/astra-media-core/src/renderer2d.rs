use std::collections::BTreeMap;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    pub hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Alpha,
    Add,
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

    fn then(self, next: Self) -> Self {
        Self {
            m11: next.m11 * self.m11 + next.m21 * self.m12,
            m12: next.m12 * self.m11 + next.m22 * self.m12,
            m21: next.m11 * self.m21 + next.m21 * self.m22,
            m22: next.m12 * self.m21 + next.m22 * self.m22,
            tx: next.m11 * self.tx + next.m21 * self.ty + next.tx,
            ty: next.m12 * self.tx + next.m22 * self.ty + next.ty,
        }
    }

    fn inverse(self) -> Option<Self> {
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

    fn point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.m11 * x + self.m21 * y + self.tx,
            self.m12 * x + self.m22 * y + self.ty,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextureFrame {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GlyphBitmapFormat {
    Alpha8,
    Rgba8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    pub format: GlyphBitmapFormat,
    pub pixels: Vec<u8>,
    pub hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneCommand {
    UploadTexture {
        resource_id: String,
        frame: TextureFrame,
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
        glyphs: Vec<GlyphInstance>,
        rgba: [u8; 4],
        opacity: f32,
        blend: BlendMode,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlyphInstance {
    pub resource_id: String,
    pub x: i32,
    pub y: i32,
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
        Ok(self.capture_frame(commands)?.hash)
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
        })
    }
}

#[derive(Debug, Clone)]
pub struct HeadlessRenderer {
    request: RendererCreateRequest,
    textures: BTreeMap<String, TextureFrame>,
    glyphs: BTreeMap<String, GlyphBitmap>,
}

impl HeadlessRenderer {
    pub fn capture_frame(&mut self, commands: &[DrawCommand]) -> Result<CpuFrame, MediaError> {
        <Self as Renderer2D>::capture_frame(self, commands)
    }

    pub fn capture_hash(&mut self, commands: &[DrawCommand]) -> Result<Hash256, MediaError> {
        <Self as Renderer2D>::capture_hash(self, commands)
    }
}

impl Renderer2D for HeadlessRenderer {
    fn capture_frame(&mut self, commands: &[DrawCommand]) -> Result<CpuFrame, MediaError> {
        let width = self.request.width as usize;
        let height = self.request.height as usize;
        let byte_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| MediaError::message("render target is too large"))?;
        let mut bytes = vec![0; byte_len];
        // Resource mutations are committed only after the whole command stream succeeds.
        let mut textures = self.textures.clone();
        let mut glyph_resources = self.glyphs.clone();
        let full_clip = RectI::new(0, 0, self.request.width, self.request.height);
        let mut clips = vec![full_clip];
        let mut transforms = vec![Transform2D::IDENTITY];
        let mut camera = Transform2D::IDENTITY;
        let mut opacities = vec![1.0_f32];
        for command in commands {
            match command {
                DrawCommand::UploadTexture { resource_id, frame } => {
                    validate_texture(frame)?;
                    if textures.contains_key(resource_id) {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_DUPLICATE: texture resource is already uploaded",
                        ));
                    }
                    textures.insert(resource_id.clone(), frame.clone());
                }
                DrawCommand::UploadGlyph { resource_id, glyph } => {
                    validate_glyph(glyph)?;
                    if glyph_resources.contains_key(resource_id) {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_DUPLICATE: glyph resource is already uploaded",
                        ));
                    }
                    glyph_resources.insert(resource_id.clone(), glyph.clone());
                }
                DrawCommand::ReleaseResource { resource_id } => {
                    let removed = textures.remove(resource_id).is_some()
                        | glyph_resources.remove(resource_id).is_some();
                    if !removed {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_UNKNOWN: released resource is not uploaded",
                        ));
                    }
                }
                DrawCommand::Sprite {
                    texture_id,
                    source,
                    destination,
                    opacity,
                    blend,
                    ..
                } => {
                    let frame = textures.get(texture_id).ok_or_else(|| {
                        MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_UNKNOWN: sprite texture is not uploaded",
                        )
                    })?;
                    let cropped;
                    let frame = if let Some(source) = source {
                        cropped = crop_texture(frame, *source)?;
                        &cropped
                    } else {
                        frame
                    };
                    draw_texture(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        *destination,
                        frame,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::GlyphRun {
                    glyphs,
                    rgba,
                    opacity,
                    blend,
                    ..
                } => {
                    for instance in glyphs {
                        let glyph =
                            glyph_resources.get(&instance.resource_id).ok_or_else(|| {
                                MediaError::message(
                                    "ASTRA_MEDIA_RESOURCE_UNKNOWN: glyph resource is not uploaded",
                                )
                            })?;
                        draw_glyph(
                            &mut bytes,
                            width,
                            height,
                            *clips.last().expect("clip stack is initialized"),
                            current_transform(camera, &transforms),
                            instance.x,
                            instance.y,
                            glyph,
                            *rgba,
                            *opacity * opacities.last().copied().unwrap_or(1.0),
                            *blend,
                        )?;
                    }
                }
                DrawCommand::Clear { rgba } => {
                    for pixel in bytes.chunks_exact_mut(4) {
                        pixel.copy_from_slice(rgba);
                    }
                }
                DrawCommand::Rect {
                    x,
                    y,
                    width: rect_width,
                    height: rect_height,
                    rgba,
                    ..
                } => {
                    let transform = current_transform(camera, &transforms);
                    draw_solid(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        transform,
                        RectI::new(*x as i32, *y as i32, *rect_width, *rect_height),
                        *rgba,
                        *opacities.last().expect("opacity stack is initialized"),
                        BlendMode::Alpha,
                    )?;
                }
                DrawCommand::Texture {
                    frame,
                    destination,
                    opacity,
                    blend,
                    ..
                }
                | DrawCommand::VideoFrame {
                    frame,
                    destination,
                    opacity,
                    blend,
                    ..
                } => {
                    validate_texture(frame)?;
                    draw_texture(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        *destination,
                        frame,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::Glyph {
                    glyph,
                    x,
                    y,
                    rgba,
                    opacity,
                    blend,
                    ..
                } => {
                    validate_glyph(glyph)?;
                    draw_glyph(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        *x,
                        *y,
                        glyph,
                        *rgba,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::PushClip { rect } => {
                    let transformed =
                        transformed_bounds(current_transform(camera, &transforms), *rect)?;
                    clips.push(intersection(
                        *clips.last().expect("clip stack is initialized"),
                        transformed,
                    ));
                }
                DrawCommand::PopClip => {
                    if clips.len() == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_CLIP_STACK: clip stack underflow",
                        ));
                    }
                    clips.pop();
                }
                DrawCommand::PushTransform { transform } => {
                    validate_transform(*transform)?;
                    let current = *transforms.last().expect("transform stack is initialized");
                    transforms.push(current.then(*transform));
                }
                DrawCommand::PopTransform => {
                    if transforms.len() == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_TRANSFORM_STACK: transform stack underflow",
                        ));
                    }
                    transforms.pop();
                }
                DrawCommand::SetCamera { transform } => {
                    validate_transform(*transform)?;
                    camera = *transform;
                }
                DrawCommand::PushOpacity { opacity } => {
                    validate_opacity(*opacity)?;
                    opacities.push(opacities.last().copied().unwrap_or(1.0) * *opacity);
                }
                DrawCommand::PopOpacity => {
                    if opacities.len() == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_OPACITY_STACK: opacity stack underflow",
                        ));
                    }
                    opacities.pop();
                }
                DrawCommand::FilterGraph { graph } => {
                    let frame = CpuFrame {
                        width: self.request.width,
                        height: self.request.height,
                        format: self.request.format,
                        hash: frame_hash(
                            self.request.width,
                            self.request.height,
                            self.request.format,
                            &bytes,
                        ),
                        bytes,
                    };
                    bytes = crate::CpuFilterExecutor.execute(graph, frame)?.0.bytes;
                }
            }
        }
        if clips.len() != 1 || transforms.len() != 1 || opacities.len() != 1 {
            return Err(MediaError::message(
                "ASTRA_MEDIA_SCENE_STACK: scene command stacks are unbalanced",
            ));
        }
        let hash = frame_hash(
            self.request.width,
            self.request.height,
            self.request.format,
            &bytes,
        );
        let frame = CpuFrame {
            width: self.request.width,
            height: self.request.height,
            format: self.request.format,
            bytes,
            hash,
        };
        self.textures = textures;
        self.glyphs = glyph_resources;
        tracing::trace!(
            target: "astra_media_core::renderer2d",
            event = "renderer2d.frame.committed",
            command_count = commands.len(),
            texture_count = self.textures.len(),
            glyph_count = self.glyphs.len(),
            frame_hash = %frame.hash,
        );
        Ok(frame)
    }
}

fn current_transform(camera: Transform2D, transforms: &[Transform2D]) -> Transform2D {
    transforms
        .last()
        .copied()
        .unwrap_or(Transform2D::IDENTITY)
        .then(camera)
}

fn validate_transform(transform: Transform2D) -> Result<(), MediaError> {
    if [
        transform.m11,
        transform.m12,
        transform.m21,
        transform.m22,
        transform.tx,
        transform.ty,
    ]
    .into_iter()
    .all(f32::is_finite)
        && transform.inverse().is_some()
    {
        Ok(())
    } else {
        Err(MediaError::message(
            "ASTRA_MEDIA_TRANSFORM: transform is non-finite or singular",
        ))
    }
}

fn validate_opacity(opacity: f32) -> Result<(), MediaError> {
    if opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(MediaError::message(
            "ASTRA_MEDIA_OPACITY: opacity must be within 0..=1",
        ))
    }
}

fn validate_texture(frame: &TextureFrame) -> Result<(), MediaError> {
    let expected = frame.width as usize * frame.height as usize * 4;
    if frame.width == 0 || frame.height == 0 || frame.rgba8.len() != expected {
        return Err(MediaError::message(
            "ASTRA_MEDIA_TEXTURE_SIZE: texture dimensions do not match payload",
        ));
    }
    if Hash256::from_sha256(&frame.rgba8) != frame.hash {
        return Err(MediaError::message(
            "ASTRA_MEDIA_TEXTURE_HASH: texture payload hash mismatch",
        ));
    }
    Ok(())
}

fn validate_glyph(glyph: &GlyphBitmap) -> Result<(), MediaError> {
    let channels = match glyph.format {
        GlyphBitmapFormat::Alpha8 => 1,
        GlyphBitmapFormat::Rgba8 => 4,
    };
    let expected = (glyph.width as usize)
        .checked_mul(glyph.height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| MediaError::message("ASTRA_MEDIA_GLYPH_SIZE: glyph size overflows"))?;
    if glyph.width == 0 || glyph.height == 0 || glyph.pixels.len() != expected {
        return Err(MediaError::message(
            "ASTRA_MEDIA_GLYPH_SIZE: glyph dimensions do not match payload",
        ));
    }
    if Hash256::from_sha256(&glyph.pixels) != glyph.hash {
        return Err(MediaError::message(
            "ASTRA_MEDIA_GLYPH_HASH: glyph payload hash mismatch",
        ));
    }
    Ok(())
}

fn crop_texture(frame: &TextureFrame, source: RectI) -> Result<TextureFrame, MediaError> {
    if source.x < 0
        || source.y < 0
        || source.width == 0
        || source.height == 0
        || source.x as u32 + source.width > frame.width
        || source.y as u32 + source.height > frame.height
    {
        return Err(MediaError::message(
            "ASTRA_MEDIA_SOURCE_RECT: sprite source rectangle is outside the texture",
        ));
    }
    let mut rgba8 = Vec::with_capacity(source.width as usize * source.height as usize * 4);
    for y in source.y as u32..source.y as u32 + source.height {
        let start = ((y * frame.width + source.x as u32) * 4) as usize;
        let end = start + source.width as usize * 4;
        rgba8.extend_from_slice(&frame.rgba8[start..end]);
    }
    Ok(TextureFrame {
        width: source.width,
        height: source.height,
        hash: Hash256::from_sha256(&rgba8),
        rgba8,
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_solid(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    destination: RectI,
    rgba: [u8; 4],
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    let frame = TextureFrame {
        width: 1,
        height: 1,
        rgba8: rgba.to_vec(),
        hash: Hash256::from_sha256(&rgba),
    };
    draw_texture(
        target,
        target_width,
        target_height,
        clip,
        transform,
        destination,
        &frame,
        opacity,
        blend,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_texture(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    destination: RectI,
    frame: &TextureFrame,
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    validate_opacity(opacity)?;
    if destination.width == 0 || destination.height == 0 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_DESTINATION_SIZE: destination must be non-empty",
        ));
    }
    let inverse = transform
        .inverse()
        .ok_or_else(|| MediaError::message("ASTRA_MEDIA_TRANSFORM: transform is singular"))?;
    let bounds = intersection(clip, transformed_bounds(transform, destination)?);
    for y in bounds.y.max(0) as usize
        ..(bounds.y + bounds.height as i32)
            .max(0)
            .min(target_height as i32) as usize
    {
        for x in bounds.x.max(0) as usize
            ..(bounds.x + bounds.width as i32)
                .max(0)
                .min(target_width as i32) as usize
        {
            let (local_x, local_y) = inverse.point(x as f32 + 0.5, y as f32 + 0.5);
            let dx = local_x - destination.x as f32;
            let dy = local_y - destination.y as f32;
            if dx < 0.0
                || dy < 0.0
                || dx >= destination.width as f32
                || dy >= destination.height as f32
            {
                continue;
            }
            let sx = ((dx / destination.width as f32) * frame.width as f32).floor() as usize;
            let sy = ((dy / destination.height as f32) * frame.height as f32).floor() as usize;
            let source_offset = (sy.min(frame.height as usize - 1) * frame.width as usize
                + sx.min(frame.width as usize - 1))
                * 4;
            let target_offset = (y * target_width + x) * 4;
            blend_pixel(
                &mut target[target_offset..target_offset + 4],
                &frame.rgba8[source_offset..source_offset + 4],
                opacity,
                blend,
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    x: i32,
    y: i32,
    glyph: &GlyphBitmap,
    rgba: [u8; 4],
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    let mut rgba8 = Vec::with_capacity(glyph.width as usize * glyph.height as usize * 4);
    match glyph.format {
        GlyphBitmapFormat::Alpha8 => {
            for alpha in &glyph.pixels {
                rgba8.extend_from_slice(&[
                    rgba[0],
                    rgba[1],
                    rgba[2],
                    ((*alpha as u16 * rgba[3] as u16) / 255) as u8,
                ]);
            }
        }
        GlyphBitmapFormat::Rgba8 => {
            for source in glyph.pixels.chunks_exact(4) {
                rgba8.extend_from_slice(&[
                    ((source[0] as u16 * rgba[0] as u16) / 255) as u8,
                    ((source[1] as u16 * rgba[1] as u16) / 255) as u8,
                    ((source[2] as u16 * rgba[2] as u16) / 255) as u8,
                    ((source[3] as u16 * rgba[3] as u16) / 255) as u8,
                ]);
            }
        }
    }
    let frame = TextureFrame {
        width: glyph.width,
        height: glyph.height,
        hash: Hash256::from_sha256(&rgba8),
        rgba8,
    };
    draw_texture(
        target,
        target_width,
        target_height,
        clip,
        transform,
        RectI::new(x, y, glyph.width, glyph.height),
        &frame,
        opacity,
        blend,
    )
}

fn transformed_bounds(transform: Transform2D, rect: RectI) -> Result<RectI, MediaError> {
    validate_transform(transform)?;
    let x1 = rect.x as f32 + rect.width as f32;
    let y1 = rect.y as f32 + rect.height as f32;
    let points = [
        transform.point(rect.x as f32, rect.y as f32),
        transform.point(x1, rect.y as f32),
        transform.point(rect.x as f32, y1),
        transform.point(x1, y1),
    ];
    let min_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let min_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let max_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    let max_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    Ok(RectI::new(
        min_x,
        min_y,
        max_x.saturating_sub(min_x) as u32,
        max_y.saturating_sub(min_y) as u32,
    ))
}

fn intersection(left: RectI, right: RectI) -> RectI {
    let x0 = left.x.max(right.x);
    let y0 = left.y.max(right.y);
    let x1 = (left.x + left.width as i32).min(right.x + right.width as i32);
    let y1 = (left.y + left.height as i32).min(right.y + right.height as i32);
    RectI::new(
        x0,
        y0,
        x1.saturating_sub(x0) as u32,
        y1.saturating_sub(y0) as u32,
    )
}

fn blend_pixel(target: &mut [u8], source: &[u8], opacity: f32, blend: BlendMode) {
    let alpha = source[3] as f32 / 255.0 * opacity;
    let target_alpha = target[3] as f32 / 255.0;
    let output_alpha = alpha + target_alpha * (1.0 - alpha);
    match blend {
        BlendMode::Alpha => {
            for channel in 0..3 {
                let premultiplied = source[channel] as f32 * alpha
                    + target[channel] as f32 * target_alpha * (1.0 - alpha);
                target[channel] = if output_alpha > 0.0 {
                    (premultiplied / output_alpha).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                };
            }
        }
        BlendMode::Add => {
            for channel in 0..3 {
                target[channel] =
                    target[channel].saturating_add((source[channel] as f32 * alpha).round() as u8);
            }
        }
        BlendMode::Multiply => {
            for channel in 0..3 {
                let multiplied = target[channel] as f32 * source[channel] as f32 / 255.0;
                target[channel] = (target[channel] as f32 * (1.0 - alpha) + multiplied * alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        BlendMode::Screen => {
            for channel in 0..3 {
                let screened = 255.0
                    - (255.0 - target[channel] as f32) * (255.0 - source[channel] as f32) / 255.0;
                target[channel] = (target[channel] as f32 * (1.0 - alpha) + screened * alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    target[3] = (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}

pub fn frame_hash(width: u32, height: u32, format: RenderTargetFormat, bytes: &[u8]) -> Hash256 {
    let mut payload = Vec::with_capacity(12 + bytes.len());
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend_from_slice(&(format as u32).to_le_bytes());
    payload.extend_from_slice(bytes);
    Hash256::from_sha256(&payload)
}
