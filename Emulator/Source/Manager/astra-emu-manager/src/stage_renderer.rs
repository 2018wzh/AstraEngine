use std::{
    env,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use astra_emu_manager::{
    AstraUnderlayRenderer, StageTextureUpdate, WgpuFrameContext,
    effects::{FilterConfiguration, FilterEngine, FilterPreset, PreparedFilter},
};
use astra_emu_manager_core::model::{FrameIdentity, FrameLease};
use astra_media_core::Viewport2D;
use wgpu::{Extent3d, TextureUsages};

const DEFAULT_STAGE_WIDTH: u32 = 800;
const DEFAULT_STAGE_HEIGHT: u32 = 600;

#[cfg(windows)]
#[path = "stage_d3d12.rs"]
mod d3d12;
#[path = "stage_filter.rs"]
mod filter;
#[path = "stage_frame.rs"]
mod frame;
#[path = "stage_pixels.rs"]
mod pixels;
pub(crate) use frame::FrameMailbox;

pub(crate) struct ManagerStageRenderer {
    mailbox: FrameMailbox,
    device: Option<wgpu::Device>,
    input_texture: Option<wgpu::Texture>,
    output_texture: Option<wgpu::Texture>,
    filtered_texture: Option<wgpu::Texture>,
    presented_input: Option<wgpu::Texture>,
    pixel_presentation: pixels::PixelPresentation,
    input_width: u32,
    input_height: u32,
    logical_width: u32,
    logical_height: u32,
    output_width: u32,
    output_height: u32,
    uploaded_identity: Option<FrameIdentity>,
    texture_dirty: bool,
    dxc_path: PathBuf,
    filter_engine: FilterEngine,
    filter_configuration: FilterConfiguration,
    filter_dirty: bool,
    filter_activation: filter::FilterActivation,
    #[cfg(windows)]
    d3d12: Option<d3d12::Importer>,
}

impl ManagerStageRenderer {
    pub(crate) fn new(mailbox: FrameMailbox) -> Self {
        let dxc_path = env::var_os("ASTRA_EMU_DXC_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(default_dxc_path);
        Self::with_dxc_path(mailbox, dxc_path)
    }

    fn with_dxc_path(mailbox: FrameMailbox, dxc_path: PathBuf) -> Self {
        Self {
            mailbox,
            device: None,
            input_texture: None,
            output_texture: None,
            filtered_texture: None,
            presented_input: None,
            pixel_presentation: pixels::PixelPresentation::default(),
            input_width: DEFAULT_STAGE_WIDTH,
            input_height: DEFAULT_STAGE_HEIGHT,
            logical_width: DEFAULT_STAGE_WIDTH,
            logical_height: DEFAULT_STAGE_HEIGHT,
            output_width: DEFAULT_STAGE_WIDTH,
            output_height: DEFAULT_STAGE_HEIGHT,
            uploaded_identity: None,
            texture_dirty: false,
            filter_engine: FilterEngine::new(dxc_path.clone()),
            dxc_path,
            filter_configuration: FilterConfiguration::default(),
            filter_dirty: false,
            filter_activation: filter::FilterActivation::default(),
            #[cfg(windows)]
            d3d12: None,
        }
    }

    fn configure_filter_inner(
        &mut self,
        config: &FilterConfiguration,
        source: Option<&str>,
    ) -> Result<(), String> {
        let prepared = PreparedFilter::new(config, source).map_err(|error| error.to_string())?;
        if self.uploaded_identity.is_none() {
            self.filter_activation.defer(prepared);
            return Ok(());
        }
        self.activate_filter(prepared, self.input_width, self.input_height)?;
        self.filter_activation.complete(Ok(()));
        Ok(())
    }

    fn activate_filter(
        &mut self,
        prepared: PreparedFilter,
        input_width: u32,
        input_height: u32,
    ) -> Result<(), String> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HOST_RENDERER_NOT_SETUP".to_owned())?;
        let (output_width, output_height) = prepared
            .output_dimensions(input_width, input_height)
            .map_err(|error| error.to_string())?;
        validate_dimensions(device, output_width, output_height)?;
        let config = prepared.configuration().clone();
        let mut candidate = FilterEngine::new(self.dxc_path.clone());
        candidate
            .reload_prepared(device, prepared)
            .map_err(|error| error.to_string())?;
        let replacement = if config.preset == FilterPreset::None {
            None
        } else if self.filtered_texture.as_ref().is_some_and(|texture| {
            texture.width() == output_width && texture.height() == output_height
        }) {
            self.filtered_texture.clone()
        } else {
            Some(create_stage_texture(
                device,
                output_width,
                output_height,
                "output",
                wgpu::TextureFormat::Rgba8Unorm,
            )?)
        };

        self.filter_engine = candidate;
        self.filter_configuration = config;
        self.filtered_texture = replacement;
        self.output_width = output_width;
        self.output_height = output_height;
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
        input_format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        validate_dimensions(device, input_width, input_height)?;
        validate_dimensions(device, output_width, output_height)?;
        let input = create_stage_texture(device, input_width, input_height, "input", input_format)?;
        let filtered = if self.filter_configuration.preset == FilterPreset::None {
            None
        } else if self.filtered_texture.as_ref().is_some_and(|texture| {
            texture.width() == output_width && texture.height() == output_height
        }) {
            self.filtered_texture.clone()
        } else {
            Some(create_stage_texture(
                device,
                output_width,
                output_height,
                "output",
                wgpu::TextureFormat::Rgba8Unorm,
            )?)
        };
        self.output_texture = filtered.as_ref().cloned().or_else(|| Some(input.clone()));
        self.filtered_texture = filtered;
        self.input_texture = Some(input);
        self.input_width = input_width;
        self.input_height = input_height;
        self.output_width = output_width;
        self.output_height = output_height;
        self.uploaded_identity = None;
        self.presented_input = None;
        self.texture_dirty = true;
        Ok(())
    }

    fn render_frame(
        &mut self,
        context: WgpuFrameContext<'_>,
        frame: &FrameLease,
    ) -> Result<(), String> {
        let info = frame.info;
        let pending = self.filter_activation.take_pending();
        if pending.is_none() && !self.filter_activation.ready_for_frame()? {
            return Ok(());
        }
        validate_dimensions(context.device, info.width, info.height)?;
        if let Some(prepared) = pending {
            self.activate_filter(prepared, info.width, info.height)?;
            self.filter_activation.complete(Ok(()));
        }
        if Some(frame.identity) == self.uploaded_identity && !self.filter_dirty {
            return Ok(());
        }
        let (output_width, output_height) = self
            .filter_engine
            .active_output_dimensions(info.width, info.height)
            .map_err(|error| error.to_string())?;
        let input_format = pixels::input_format(info.format);
        let input_needs_replacement = self.input_width != info.width
            || self.input_height != info.height
            || self
                .input_texture
                .as_ref()
                .is_none_or(|texture| texture.format() != input_format);
        if input_needs_replacement {
            self.recreate_textures(
                context.device,
                info.width,
                info.height,
                output_width,
                output_height,
                input_format,
            )?;
        } else if self.filter_configuration.preset != FilterPreset::None
            && self.filtered_texture.as_ref().is_none_or(|texture| {
                texture.width() != output_width || texture.height() != output_height
            })
        {
            self.filtered_texture = Some(create_stage_texture(
                context.device,
                output_width,
                output_height,
                "output",
                wgpu::TextureFormat::Rgba8Unorm,
            )?);
            self.output_width = output_width;
            self.output_height = output_height;
        }
        if self.logical_width != info.logical_width || self.logical_height != info.logical_height {
            self.logical_width = info.logical_width;
            self.logical_height = info.logical_height;
            self.texture_dirty = true;
        }
        let input = self
            .input_texture
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HOST_RENDERER_NOT_SETUP".to_owned())?;
        if Some(frame.identity) != self.uploaded_identity {
            if let Some(pixels) = frame.pixels() {
                let required = info
                    .required_bytes()
                    .ok_or_else(|| "ASTRA_EMU_HOST_FRAME_SIZE".to_owned())?;
                context.queue.write_texture(
                    input.as_image_copy(),
                    &pixels[..required],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(info.stride),
                        rows_per_image: Some(info.height),
                    },
                    Extent3d {
                        width: info.width,
                        height: info.height,
                        depth_or_array_layers: 1,
                    },
                );
                // write_texture consumes CPU bytes before returning.
            } else {
                #[cfg(windows)]
                self.d3d12
                    .as_mut()
                    .ok_or("ASTRA_EMU_D3D12_HOST_UNSUPPORTED")?
                    .copy(
                        context.device,
                        context.queue,
                        frame.d3d12().ok_or("ASTRA_EMU_GPU_BACKEND_UNSUPPORTED")?,
                        input,
                    )?;
                #[cfg(not(windows))]
                return Err("ASTRA_EMU_GPU_BACKEND_UNSUPPORTED".into());
            }
            self.presented_input =
                Some(
                    self.pixel_presentation
                        .apply(context.device, context.queue, input, info),
                );
        }
        let presented_input = self
            .presented_input
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HOST_FRAME_BASELINE".to_owned())?;
        let output = self
            .filter_engine
            .apply(
                context.device,
                context.queue,
                presented_input,
                self.filtered_texture.as_ref(),
                &self.filter_configuration,
            )
            .map_err(|error| error.to_string())?;
        if self.output_texture.as_ref() != Some(&output) {
            self.output_texture = Some(output);
            self.texture_dirty = true;
        }
        self.uploaded_identity = Some(frame.identity);
        self.mailbox.presented(frame.identity, frame.info)?;
        self.filter_dirty = false;
        Ok(())
    }
}

fn default_dxc_path() -> PathBuf {
    let executable = env::current_exe().unwrap_or_else(|_| PathBuf::from("astra-emu-manager"));
    dxc_path_for_executable(&executable)
}

fn dxc_path_for_executable(executable: &Path) -> PathBuf {
    let filename = format!(
        "{}dxcompiler.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    );
    match executable.parent() {
        Some(directory) => directory.join(filename),
        None => PathBuf::from(filename),
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
        if self.device.is_some() {
            return Err("ASTRA_EMU_HOST_RENDERER_DUPLICATE_SETUP".into());
        }
        let adapter = context.device.adapter_info();
        tracing::info!(
            event = "astra.emu.host.gpu.created",
            backend = ?adapter.backend,
            device_type = ?adapter.device_type,
            device = %adapter.name,
            vendor_id = adapter.vendor,
            device_id = adapter.device,
            driver = %adapter.driver,
        );
        #[cfg(windows)]
        {
            let importer = d3d12::Importer::new(context.device)?;
            self.mailbox
                .set_d3d12(importer.as_ref().map(|importer| importer.host))?;
            self.d3d12 = importer;
        }
        self.device = Some(context.device.clone());
        self.filter_dirty = false;
        Ok(())
    }

    fn take_stage_texture_update(&mut self) -> Result<Option<StageTextureUpdate>, String> {
        if !self.texture_dirty {
            return Ok(None);
        }
        let texture = self
            .output_texture
            .clone()
            .ok_or_else(|| "ASTRA_EMU_HOST_RENDERER_OUTPUT_MISSING".to_owned())?;
        let content = content_viewport_for_output(
            self.logical_width,
            self.logical_height,
            self.input_width,
            self.input_height,
            texture.width(),
            texture.height(),
        )?;
        self.texture_dirty = false;
        Ok(Some(StageTextureUpdate {
            raster_width: texture.width(),
            raster_height: texture.height(),
            texture,
            logical_width: self.logical_width,
            logical_height: self.logical_height,
            content,
        }))
    }

    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        #[cfg(windows)]
        if let Some(importer) = &mut self.d3d12 {
            importer.reap()?;
        }
        let Some(frame) = self.mailbox.snapshot()? else {
            #[cfg(windows)]
            if let Some(importer) = &mut self.d3d12 {
                importer.drain()?;
            }
            if self.uploaded_identity.is_some() {
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
                self.presented_input = None;
                self.pixel_presentation = pixels::PixelPresentation::default();
                self.uploaded_identity = None;
                self.filter_dirty = false;
            }
            return Ok(());
        };
        let first_frame = self.uploaded_identity.is_none();
        let result = self.render_frame(context, &frame);
        if first_frame && result.is_err() {
            self.filter_activation.complete(result);
            return self.filter_activation.ready_for_frame().map(|_| ());
        }
        result
    }

    fn teardown(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        let gpu_result = self
            .d3d12
            .as_mut()
            .map_or(Ok(()), |importer| importer.drain());
        #[cfg(not(windows))]
        let gpu_result = Ok(());
        #[cfg(windows)]
        if gpu_result.is_ok() {
            self.d3d12 = None;
        }
        let mailbox_result = self.mailbox.set_d3d12(None);
        self.input_texture = None;
        self.output_texture = None;
        self.filtered_texture = None;
        self.presented_input = None;
        self.pixel_presentation = pixels::PixelPresentation::default();
        self.device = None;
        self.input_width = DEFAULT_STAGE_WIDTH;
        self.input_height = DEFAULT_STAGE_HEIGHT;
        self.output_width = DEFAULT_STAGE_WIDTH;
        self.output_height = DEFAULT_STAGE_HEIGHT;
        self.logical_width = DEFAULT_STAGE_WIDTH;
        self.logical_height = DEFAULT_STAGE_HEIGHT;
        self.uploaded_identity = None;
        self.texture_dirty = false;
        self.filter_engine = FilterEngine::new(self.dxc_path.clone());
        self.filter_configuration = FilterConfiguration::default();
        self.filter_dirty = false;
        self.filter_activation = filter::FilterActivation::default();
        gpu_result.and(mailbox_result)
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

pub(crate) fn content_viewport_for_output(
    logical_width: u32,
    logical_height: u32,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<Viewport2D, String> {
    if [
        logical_width,
        logical_height,
        input_width,
        input_height,
        output_width,
        output_height,
    ]
    .contains(&0)
    {
        return Err("ASTRA_EMU_HOST_FRAME_DIMENSIONS".into());
    }
    // A native surface already includes the core's own scene/UI/letterboxing.
    // Host V is the complete filtered raster; only the core maps pixels to L.
    Ok(Viewport2D {
        x: 0,
        y: 0,
        width: output_width,
        height: output_height,
    })
}

fn create_stage_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    role: &str,
    format: wgpu::TextureFormat,
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
        format,
        usage: TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::COPY_SRC
            | TextureUsages::RENDER_ATTACHMENT
            | if role == "input" {
                TextureUsages::empty()
            } else {
                TextureUsages::STORAGE_BINDING
            },
        view_formats: &[],
    });
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(format!("ASTRA_EMU_HOST_RENDERER_TEXTURE: {error}"));
    }
    Ok(texture)
}

#[cfg(test)]
#[path = "stage_renderer_tests.rs"]
mod tests;
