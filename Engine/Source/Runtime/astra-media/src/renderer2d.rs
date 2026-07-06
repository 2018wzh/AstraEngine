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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawCommand {
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
}

impl DrawCommand {
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
pub struct HeadlessRendererProvider;

impl Renderer2DProvider for HeadlessRendererProvider {
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
        Ok(HeadlessRenderer { request })
    }
}

#[derive(Debug, Clone)]
pub struct HeadlessRenderer {
    request: RendererCreateRequest,
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
        for command in commands {
            match command {
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
                    let x0 = (*x as usize).min(width);
                    let y0 = (*y as usize).min(height);
                    let x1 = x0.saturating_add(*rect_width as usize).min(width);
                    let y1 = y0.saturating_add(*rect_height as usize).min(height);
                    for py in y0..y1 {
                        for px in x0..x1 {
                            let offset = (py * width + px) * 4;
                            bytes[offset..offset + 4].copy_from_slice(rgba);
                        }
                    }
                }
            }
        }
        let hash = frame_hash(
            self.request.width,
            self.request.height,
            self.request.format,
            &bytes,
        );
        Ok(CpuFrame {
            width: self.request.width,
            height: self.request.height,
            format: self.request.format,
            bytes,
            hash,
        })
    }
}

fn frame_hash(width: u32, height: u32, format: RenderTargetFormat, bytes: &[u8]) -> Hash256 {
    let mut payload = Vec::with_capacity(12 + bytes.len());
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend_from_slice(&(format as u32).to_le_bytes());
    payload.extend_from_slice(bytes);
    Hash256::from_sha256(&payload)
}

#[cfg(feature = "desktop-wgpu")]
#[derive(Debug, Clone, Default)]
pub struct WgpuRendererProvider;

#[cfg(feature = "desktop-wgpu")]
impl WgpuRendererProvider {
    pub fn descriptor(&self) -> RendererDescriptor {
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        RendererDescriptor {
            provider_id: "astra.renderer.wgpu".to_string(),
            backend: format!("wgpu:{format:?}"),
            headless: true,
            packaged_eligible: true,
            formats: vec![RenderTargetFormat::Rgba8Srgb],
            shader_model: "wgpu".to_string(),
        }
    }
}
