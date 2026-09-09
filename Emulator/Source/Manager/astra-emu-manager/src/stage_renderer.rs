use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use astra_emu_family_api::{FamilyResult, FrameAlpha, FrameFormat, FrameView, FrameVisitor};
use astra_emu_manager::{
    effects::{FilterConfiguration, FilterEngine, FilterPreset},
    AstraUnderlayRenderer, WgpuFrameContext,
};
use wgpu::{Extent3d, TextureUsages};

const DEFAULT_STAGE_WIDTH: u32 = 800;
const DEFAULT_STAGE_HEIGHT: u32 = 600;
const BYTES_PER_PIXEL: u32 = 4;
const COPY_ROW_ALIGNMENT: u32 = 256;

/// A synchronous handoff from a Family session to the render notifier.
/// `publish` copies the borrowed ABI frame into a host-owned buffer before the
/// family returns from its frame callback.
#[derive(Clone, Default)]
pub(crate) struct FrameMailbox {
    state: Arc<Mutex<FrameMailboxState>>,
}

#[derive(Default)]
struct FrameMailboxState {
    generation: u64,
    frame: Option<CapturedFrame>,
}

#[derive(Clone)]
struct CapturedFrame {
    generation: u64,
    width: u32,
    height: u32,
    stride: u32,
    pixels: Arc<Vec<u8>>,
}

impl FrameMailbox {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn publish(&self, frame: FrameView<'_>) -> FamilyResult<()> {
        frame.info.validate()?;
        let alpha = match frame.info.format {
            FrameFormat::Rgba8Srgb { alpha } => alpha,
        };
        let source_stride = frame.info.stride;
        let row_bytes = frame
            .info
            .width
            .checked_mul(BYTES_PER_PIXEL)
            .ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame row size overflows",
                )
            })?;
        let upload_stride = align_row(row_bytes)?;
        let upload_len = usize::try_from(upload_stride)
            .ok()
            .and_then(|stride| stride.checked_mul(usize::try_from(frame.info.height).ok()?))
            .ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame upload size overflows host memory",
                )
            })?;
        let source = frame.as_slice();
        let source_len = frame.info.required_bytes().ok_or_else(|| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_SIZE",
                "frame source size overflows",
            )
        })?;
        if source.len() < source_len {
            return Err(astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_BYTES",
                "family frame is shorter than its declared stride",
            ));
        }
        let row_bytes = usize::try_from(row_bytes).map_err(|_| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_SIZE",
                "frame row does not fit host memory",
            )
        })?;
        let source_stride = usize::try_from(source_stride).map_err(|_| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_SIZE",
                "frame stride does not fit host memory",
            )
        })?;
        let upload_stride = usize::try_from(upload_stride).map_err(|_| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_SIZE",
                "frame upload stride does not fit host memory",
            )
        })?;
        let height = usize::try_from(frame.info.height).map_err(|_| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_SIZE",
                "frame height does not fit host memory",
            )
        })?;
        let mut pixels = vec![0_u8; upload_len];
        for row in 0..height {
            let source_start = row.checked_mul(source_stride).ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame source offset overflows",
                )
            })?;
            let target_start = row.checked_mul(upload_stride).ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame upload offset overflows",
                )
            })?;
            let source_end = source_start.checked_add(row_bytes).ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame source row overflows",
                )
            })?;
            let source_row = source.get(source_start..source_end).ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_BYTES",
                    "frame row exceeds the borrowed buffer",
                )
            })?;
            let target_end = target_start.checked_add(row_bytes).ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame upload row overflows",
                )
            })?;
            let target_row = pixels.get_mut(target_start..target_end).ok_or_else(|| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_SIZE",
                    "frame upload row exceeds host buffer",
                )
            })?;
            target_row.copy_from_slice(source_row);
            normalize_alpha(target_row, alpha);
        }
        let mut state = self.state.lock().map_err(|_| {
            astra_emu_family_api::FamilyError::new(
                "ASTRA_EMU_HOST_FRAME_LOCK",
                "frame mailbox lock is poisoned",
            )
        })?;
        state.generation = state.generation.checked_add(1).ok_or_else(|| {
            astra_emu_family_api::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_GENERATION",
                "frame generation exhausted",
            )
        })?;
        state.frame = Some(CapturedFrame {
            generation: state.generation,
            width: frame.info.width,
            height: frame.info.height,
            stride: u32::try_from(upload_stride).map_err(|_| {
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_STRIDE",
                    "frame upload stride does not fit ABI metadata",
                )
            })?,
            pixels: Arc::new(pixels),
        });
        Ok(())
    }

    /// Drop the last frame when a Family session ends. The generation is
    /// advanced so a frame from a previous session can never be mistaken for
    /// a newly published frame after a renderer reset.
    pub(crate) fn clear(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned())?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_HOST_FRAME_GENERATION".to_owned())?;
        state.frame = None;
        Ok(())
    }

    fn snapshot(&self) -> Result<Option<CapturedFrame>, String> {
        self.state
            .lock()
            .map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.frame.clone())
    }
}

fn normalize_alpha(row: &mut [u8], alpha: FrameAlpha) {
    for pixel in row.as_chunks_mut::<4>().0 {
        match alpha {
            FrameAlpha::Opaque => pixel[3] = u8::MAX,
        }
    }
}

pub(crate) struct FrameCollector {
    mailbox: FrameMailbox,
}

impl FrameCollector {
    pub(crate) fn new(mailbox: FrameMailbox) -> Self {
        Self { mailbox }
    }
}

impl FrameVisitor for FrameCollector {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()> {
        self.mailbox.publish(frame)
    }
}

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

    pub(crate) fn set_filter_preset(&mut self, preset_id: &str) -> Result<(), String> {
        let mut config = self.filter_configuration.clone();
        config.preset = match preset_id {
            "none" => FilterPreset::None,
            "scale" => FilterPreset::Scale,
            "sharpen" => FilterPreset::Sharpen,
            "anime4k" | "anime4k_restore_upscale" => {
                config.scale = 2.0;
                FilterPreset::Anime4kRestoreUpscale
            }
            _ => return Err("ASTRA_EMU_FILTER_PRESET_UNSUPPORTED".into()),
        };
        self.configure_filter_inner(&config, None)
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

    pub(crate) fn filter_configuration(&self) -> &FilterConfiguration {
        &self.filter_configuration
    }

    pub(crate) fn filter_source(&self) -> Option<&str> {
        self.filter_source.as_deref()
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
            .map(|texture| (texture, self.output_width, self.output_height))
    }

    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        let Some(frame) = self.mailbox.snapshot()? else {
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
