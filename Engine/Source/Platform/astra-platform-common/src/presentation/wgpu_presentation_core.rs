use super::*;

/// Shared hardware-only WGPU presentation path. Backends own event loops and
/// surface creation; this core owns adapter policy, RGBA upload, presentation,
/// resize, loss classification, and padded readback layout.
pub struct WgpuPresentationCore {
    pub(super) _instance: wgpu::Instance,
    pub(super) surface: Option<wgpu::Surface<'static>>,
    pub(super) adapter: wgpu::Adapter,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) glyph_renderer: WgpuGlyphAtlasRenderer,
    pub(super) filters: crate::scene_filters::SceneFilters,
    pub(super) last_upload: Option<UploadFrame>,
    pub(super) last_frame: Option<RgbaFrame>,
    pub(super) last_scene_frame: Option<SceneFrame>,
    pub(super) last_sequence: Option<u64>,
    pub(super) device_lost: Arc<AtomicBool>,
    #[cfg(feature = "platform-test-driver")]
    pub(super) test_device_loss: AtomicBool,
}

pub struct WgpuReadback {
    pub(super) buffer: wgpu::Buffer,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) padded_row: u32,
}

pub(super) struct UploadFrame {
    pub(super) texture: wgpu::Texture,
    pub(super) width: u32,
    pub(super) height: u32,
}
