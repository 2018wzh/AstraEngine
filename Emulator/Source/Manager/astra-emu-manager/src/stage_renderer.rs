use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use astra_emu_family_api::{FamilyResult, FrameAlpha, FrameFormat, FrameView, FrameVisitor};
use astra_emu_manager::{
    effects::{FilterConfiguration, FilterEngine},
    AstraUnderlayRenderer, WgpuFrameContext,
};
use wgpu::{Extent3d, TextureUsages};

const DEFAULT_STAGE_WIDTH: u32 = 800;
const DEFAULT_STAGE_HEIGHT: u32 = 600;
const BYTES_PER_PIXEL: u32 = 4;
const COPY_ROW_ALIGNMENT: u32 = 256;

#[path = "stage_frame.rs"]
mod frame;
pub(crate) use frame::{FrameCollector, FrameMailbox};

pub(crate) struct ManagerStageRenderer {
    mailbox: FrameMailbox,
    device: Option<wgpu::Device>,
    input_texture: Option<wgpu::Texture>,
    output_texture: Option<wgpu::Texture>,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    uploaded_generation: u64,
    texture_dirty: bool,
    dxc_path: PathBuf,
    filter_engine: FilterEngine,
    filter_configuration: FilterConfiguration,
    filter_dirty: bool,
    filter_source: Option<String>,
}

impl ManagerStageRenderer {
    pub(crate) fn new(mailbox: FrameMailbox) -> Self {
        let dxc_path = env::var_os("ASTRA_EMU_DXC_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dxcompiler.dll"));
        Self::with_dxc_path(mailbox, dxc_path)
    }

    fn with_dxc_path(mailbox: FrameMailbox, dxc_path: PathBuf) -> Self {
        Self {
            mailbox,
            device: None,
            input_texture: None,
            output_texture: None,
            input_width: DEFAULT_STAGE_WIDTH,
            input_height: DEFAULT_STAGE_HEIGHT,
            output_width: DEFAULT_STAGE_WIDTH,
            output_height: DEFAULT_STAGE_HEIGHT,
            uploaded_generation: 0,
            texture_dirty: false,
            filter_engine: FilterEngine::new(dxc_path.clone()),
            dxc_path,
            filter_configuration: FilterConfiguration::default(),
            filter_dirty: false,
            filter_source: None,
        }
    }

    fn configure_filter_inner(
        &mut self,
        config: &FilterConfiguration,
        source: Option<&str>,
    ) -> Result<(), String> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HOST_RENDERER_NOT_SETUP".to_owned())?;
        config.validate().map_err(|error| error.to_string())?;
        let mut candidate = FilterEngine::new(self.dxc_path.clone());
        if let Some(source) = source {
            candidate
                .reload_source(device, source, config)
                .map_err(|error| error.to_string())?;
        } else {
            candidate
                .reload(device, config)
                .map_err(|error| error.to_string())?;
        }
        let (output_width, output_height) = candidate
            .active_output_dimensions(self.input_width, self.input_height)
            .map_err(|error| error.to_string())?;
        let output_needs_replacement = self.output_texture.as_ref().is_none_or(|texture| {
            texture.width() != output_width || texture.height() != output_height
        });
        let replacement = if output_needs_replacement {
            Some(create_stage_texture(
                device,
                output_width,
                output_height,
                "output",
            )?)
        } else {
            None
        };

        self.filter_engine = candidate;
        self.filter_configuration = config.clone();
        self.filter_source = source.map(str::to_owned);
        if let Some(output) = replacement {
            self.output_texture = Some(output);
            self.output_width = output_width;
            self.output_height = output_height;
            self.texture_dirty = true;
        }
        self.filter_dirty = true;
        Ok(())
    }

    fn recreate_textures(
        &mut self,
        device: &wgpu::Device,
        input_width: u32,
        input_height: u32,
        output_width: u32,
        output_height: u32,
    ) -> Result<(), String> {
        validate_dimensions(device, input_width, input_height)?;
        validate_dimensions(device, output_width, output_height)?;
        let input = create_stage_texture(device, input_width, input_height, "input")?;
        let output = create_stage_texture(device, output_width, output_height, "output")?;
        self.input_texture = Some(input);
        self.output_texture = Some(output);
        self.input_width = input_width;
        self.input_height = input_height;
        self.output_width = output_width;
        self.output_height = output_height;
        self.uploaded_generation = 0;
        self.texture_dirty = true;
        Ok(())
    }
}

impl AstraUnderlayRenderer for ManagerStageRenderer {
    fn configure_filter(
        &mut self,
        config: &FilterConfiguration,
        source: Option<&str>,
    ) -> Result<(), String> {
        self.configure_filter_inner(config, source)
    }

    fn setup(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        if self.input_texture.is_some() || self.output_texture.is_some() {
            return Err("ASTRA_EMU_HOST_RENDERER_DUPLICATE_SETUP".into());
        }
        self.device = Some(context.device.clone());
        self.filter_engine = FilterEngine::new(self.dxc_path.clone());
        self.filter_dirty = false;
        self.recreate_textures(
            context.device,
            DEFAULT_STAGE_WIDTH,
            DEFAULT_STAGE_HEIGHT,
            DEFAULT_STAGE_WIDTH,
            DEFAULT_STAGE_HEIGHT,
        )
    }

    fn stage_texture(&self) -> Option<wgpu::Texture> {
        self.output_texture.clone()
    }

    fn take_stage_texture_update(&mut self) -> Option<(wgpu::Texture, u32, u32)> {
        if !self.texture_dirty {
            return None;
        }
        self.texture_dirty = false;
        self.output_texture
            .clone()
            .map(|texture| (texture, self.input_width, self.input_height))
    }

    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        let Some(frame) = self.mailbox.snapshot()? else {
            if self.uploaded_generation != 0 {
                if let Some(output) = &self.output_texture {
                    let view = output.create_view(&Default::default());
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                    }
                    context.queue.submit([encoder.finish()]);
                }
                self.input_texture = None;
                self.uploaded_generation = 0;
                self.filter_dirty = false;
            }
            return Ok(());
        };
        if frame.generation <= self.uploaded_generation && !self.filter_dirty {
            return Ok(());
        }
        let row_bytes = frame
            .width
            .checked_mul(BYTES_PER_PIXEL)
            .ok_or_else(|| "ASTRA_EMU_HOST_FRAME_SIZE".to_owned())?;
        if frame.width == 0 || frame.height == 0 || frame.stride < row_bytes {
            return Err("ASTRA_EMU_HOST_FRAME_DIMENSIONS".into());
        }
        validate_dimensions(context.device, frame.width, frame.height)?;
        let (output_width, output_height) = self
            .filter_engine
            .active_output_dimensions(frame.width, frame.height)
            .map_err(|error| error.to_string())?;
        let input_needs_replacement = self.input_width != frame.width
            || self.input_height != frame.height
            || self.input_texture.is_none();
        if input_needs_replacement {
            self.recreate_textures(
                context.device,
                frame.width,
                frame.height,
                output_width,
                output_height,
            )?;
        } else if self.output_width != output_width
            || self.output_height != output_height
            || self.output_texture.is_none()
        {
            let output =
                create_stage_texture(context.device, output_width, output_height, "output")?;
            self.output_texture = Some(output);
            self.output_width = output_width;
            self.output_height = output_height;
            self.texture_dirty = true;
        }
        let input = self
            .input_texture
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HOST_RENDERER_NOT_SETUP".to_owned())?;
        let output = self
            .output_texture
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HOST_RENDERER_NOT_SETUP".to_owned())?;
        let expected_len = usize::try_from(frame.stride)
            .ok()
            .and_then(|stride| stride.checked_mul(usize::try_from(frame.height).ok()?))
            .ok_or_else(|| "ASTRA_EMU_HOST_FRAME_SIZE".to_owned())?;
        if frame.pixels.len() != expected_len {
            return Err("ASTRA_EMU_HOST_FRAME_BYTES".into());
        }
        if frame.generation > self.uploaded_generation {
            context.queue.write_texture(
                input.as_image_copy(),
                &frame.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.stride),
                    rows_per_image: Some(frame.height),
                },
                Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.filter_engine
            .apply(
                context.device,
                context.queue,
                input,
                output,
                &self.filter_configuration,
            )
            .map_err(|error| error.to_string())?;
        if frame.generation > self.uploaded_generation {
            self.uploaded_generation = frame.generation;
        }
        self.filter_dirty = false;
        Ok(())
    }

    fn teardown(&mut self) {
        if let Err(error) = self.mailbox.clear() {
            tracing::error!(
                event = "astra.emu.stage_renderer.mailbox_clear_failed",
                diagnostic_code = "ASTRA_EMU_HOST_FRAME_LOCK",
                error_kind = %error,
            );
        }
        self.input_texture = None;
        self.output_texture = None;
        self.device = None;
        self.uploaded_generation = 0;
        self.texture_dirty = false;
        self.filter_engine = FilterEngine::new(self.dxc_path.clone());
        self.filter_configuration = FilterConfiguration::default();
        self.filter_dirty = false;
        self.filter_source = None;
    }
}

fn validate_dimensions(device: &wgpu::Device, width: u32, height: u32) -> Result<(), String> {
    let max = device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > max || height > max {
        return Err(format!(
            "ASTRA_EMU_HOST_FRAME_DIMENSIONS: requested {width}x{height}, device limit {max}"
        ));
    }
    Ok(())
}

fn create_stage_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    role: &str,
) -> Result<wgpu::Texture, String> {
    validate_dimensions(device, width, height)?;
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(if role == "input" {
            "astra-emu-manager-stage-input"
        } else {
            "astra-emu-manager-stage-output"
        }),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::COPY_SRC
            | TextureUsages::STORAGE_BINDING
            | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(format!("ASTRA_EMU_HOST_RENDERER_TEXTURE: {error}"));
    }
    Ok(texture)
}

fn align_row(row_bytes: u32) -> FamilyResult<u32> {
    row_bytes
        .checked_add(COPY_ROW_ALIGNMENT - 1)
        .map(|value| value / COPY_ROW_ALIGNMENT * COPY_ROW_ALIGNMENT)
        .ok_or_else(|| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_STRIDE",
                "frame upload stride overflows",
            )
        })
}

#[cfg(test)]
#[path = "stage_renderer_tests.rs"]
mod tests;
