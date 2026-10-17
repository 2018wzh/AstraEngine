//! Hardware headless endpoint for the production stage renderer. Readback occurs
//! only when a checkpoint is requested, after import, PixelPresentation and filters.
use crate::stage_renderer::{FrameMailbox, ManagerStageRenderer};
use astra_emu_manager::{AstraUnderlayRenderer, StageTextureUpdate, WgpuFrameContext};
use astra_emu_manager_core::model::FrameLease;
use std::time::Duration;

pub(super) struct Presenter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    mailbox: FrameMailbox,
    renderer: ManagerStageRenderer,
    output: Option<StageTextureUpdate>,
    close_result: Option<Result<(), String>>,
}
impl Presenter {
    pub fn new(epoch: u64) -> Result<Option<Self>, String> {
        #[cfg(not(windows))]
        {
            let _ = epoch;
            Ok(None)
        }
        #[cfg(windows)]
        {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = wgpu::Backends::DX12;
            let instance = wgpu::Instance::new(descriptor);
            let adapter =
                match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })) {
                    Ok(adapter) => adapter,
                    Err(_) => return Ok(None),
                };
            if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
                return Ok(None);
            }
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("astra headless production presenter"),
                    required_features: astra_emu_manager::effects::FilterEngine::required_features(
                    ),
                    ..Default::default()
                }))
                .map_err(|error| format!("ASTRA_EMU_HEADLESS_DEVICE: {error}"))?;
            let mailbox = FrameMailbox::new();
            mailbox.activate(epoch).map_err(|error| error.to_string())?;
            let mut renderer = ManagerStageRenderer::new(mailbox.clone());
            renderer.setup(WgpuFrameContext {
                device: &device,
                queue: &queue,
            })?;
            Ok(Some(Self {
                device,
                queue,
                mailbox,
                renderer,
                output: None,
                close_result: None,
            }))
        }
    }
    pub fn host(&self) -> Result<Option<astra_emu_abi::D3D12Host>, String> {
        self.mailbox.d3d12()
    }
    pub fn close(&mut self) -> Result<(), String> {
        if let Some(result) = &self.close_result {
            return result.clone();
        }
        let result = self.renderer.teardown();
        self.close_result = Some(result.clone());
        result
    }
    pub fn present(&mut self, frame: &FrameLease) -> Result<(), String> {
        self.mailbox
            .publish(frame.clone())
            .map_err(|error| error.to_string())?;
        self.renderer.render(WgpuFrameContext {
            device: &self.device,
            queue: &self.queue,
        })?;
        if let Some(output) = self.renderer.take_stage_texture_update()? {
            self.output = Some(output);
        }
        Ok(())
    }
    pub fn capture(&self) -> Result<image::RgbaImage, String> {
        let texture = &self
            .output
            .as_ref()
            .ok_or("ASTRA_EMU_HEADLESS_PRESENT_MISSING")?
            .texture;
        self.capture_texture(texture, false)
    }
    #[cfg(all(test, windows))]
    fn capture_native_source(&self, frame: &FrameLease) -> Result<image::RgbaImage, String> {
        // The immutable frame owner remains alive through this additional Host
        // checkpoint readback. Return to COMMON before releasing that owner.
        let texture = self.renderer.native_export_for_capture(frame)?;
        self.capture_texture(&texture, true)
    }
    fn capture_texture(
        &self,
        texture: &wgpu::Texture,
        external: bool,
    ) -> Result<image::RgbaImage, String> {
        let row = texture.width() * 4;
        let stride =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("astra Host checkpoint readback"),
            size: u64::from(stride) * u64::from(texture.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(texture.height()),
                },
            },
            texture.size(),
        );
        if external {
            encoder.transition_resources(
                std::iter::empty(),
                std::iter::once(wgpu::TextureTransition {
                    texture,
                    selector: None,
                    state: wgpu::TextureUses::PRESENT,
                }),
            );
        }
        let submission = self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(5)),
            })
            .map_err(|error| format!("ASTRA_EMU_HEADLESS_CAPTURE_POLL: {error}"))?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let mapped = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity(row as usize * texture.height() as usize);
        for row_pixels in mapped.chunks_exact(stride as usize) {
            pixels.extend_from_slice(&row_pixels[..row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        tracing::info!(
            event = "astra.emu.headless.host_capture",
            width = texture.width(),
            height = texture.height(),
            host_readback_bytes = pixels.len(),
            delivery_cpu_bytes = 0u64
        );
        image::RgbaImage::from_raw(texture.width(), texture.height(), pixels)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_CAPTURE_SIZE".into())
    }
}
impl Drop for Presenter {
    fn drop(&mut self) {
        if self.close_result.is_none() {
            if let Err(error) = self.close() {
                tracing::error!(event = "astra.emu.headless.graphics_cleanup_failed", error);
            }
        }
    }
}

#[cfg(all(test, windows))]
#[path = "headless_presenter_tests.rs"]
mod tests;
