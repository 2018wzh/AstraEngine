pub use astra_media_core::renderer2d::*;

#[cfg(feature = "desktop-wgpu")]
#[derive(Debug, Clone, Default)]
pub struct WgpuRendererProvider;

#[cfg(feature = "desktop-wgpu")]
impl WgpuRendererProvider {
    pub fn descriptor(&self) -> astra_media_core::RendererDescriptor {
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        astra_media_core::RendererDescriptor {
            provider_id: "astra.renderer.wgpu".to_string(),
            backend: format!("wgpu:{format:?}"),
            headless: true,
            packaged_eligible: true,
            formats: vec![astra_media_core::RenderTargetFormat::Rgba8Srgb],
            shader_model: "wgpu".to_string(),
        }
    }
}
