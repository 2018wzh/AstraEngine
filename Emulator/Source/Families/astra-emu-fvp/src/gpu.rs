use std::sync::{Arc, RwLock};

use astra_emu_family_api::{FamilyError, FamilyResult};
use rfvp::rfvp_render::{
    hosted::HostedGpuRenderer, wgpu, BindGroupLayouts, GpuCommonResources, Pipelines, RenderTarget,
};

pub(crate) struct GpuRenderer(HostedGpuRenderer);

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;

fn error(code: &str) -> FamilyError {
    FamilyError::new(code, "RFVP native GPU rendering failed")
}

impl GpuRenderer {
    pub(crate) fn new(width: u32, height: u32) -> FamilyResult<Self> {
        pollster::block_on(Self::create(width, height))
    }

    async fn create(width: u32, height: u32) -> FamilyResult<Self> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(error("ASTRA_EMU_FVP_GPU_DIMENSIONS"));
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| error("ASTRA_EMU_FVP_GPU_ADAPTER"))?;
        let info = adapter.get_info();
        if !matches!(
            info.device_type,
            wgpu::DeviceType::DiscreteGpu
                | wgpu::DeviceType::IntegratedGpu
                | wgpu::DeviceType::VirtualGpu
        ) {
            return Err(error("ASTRA_EMU_FVP_GPU_HARDWARE_REQUIRED"));
        }
        let limits = wgpu::Limits {
            max_push_constant_size: 80,
            ..wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("RFVP native renderer"),
                    required_features: wgpu::Features::PUSH_CONSTANTS,
                    required_limits: limits,
                },
                None,
            )
            .await
            .map_err(|cause| {
                // Native wgpu device-request errors contain fixed diagnostics,
                // feature flags and numeric limits, not game data or paths.
                FamilyError::new(
                    "ASTRA_EMU_FVP_GPU_DEVICE",
                    format!("RFVP native GPU device initialization failed: {cause}"),
                )
            })?;
        tracing::info!(event = "fvp_gpu_created", backend = ?info.backend, device_type = ?info.device_type, vendor = info.vendor, "RFVP native GPU renderer created");
        let queue = Arc::new(queue);
        let bind_group_layouts = BindGroupLayouts::new(&device);
        let pipelines = Pipelines::new(&device, &queue, &bind_group_layouts, RenderTarget::FORMAT);
        let resources = Arc::new(GpuCommonResources {
            device,
            queue,
            render_buffer_size: RwLock::new((width, height)),
            bind_group_layouts,
            pipelines,
        });
        Ok(Self(HostedGpuRenderer::new(resources, (width, height))))
    }

    pub(crate) fn capture(&mut self, core: &rfvp::hosted::RfvpCore) -> FamilyResult<Vec<u8>> {
        core.render_hosted_gpu(&mut self.0).map_err(error)
    }
}
