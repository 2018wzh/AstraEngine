use super::*;

pub(super) fn float_param(
    params: &BTreeMap<String, FilterParam>,
    name: &str,
) -> Result<String, PlatformError> {
    match params.get(name) {
        Some(FilterParam::Float(value)) if value.is_finite() => Ok(format!("{value:.9}")),
        _ => Err(invalid(
            "offscreen.filter",
            "validated filter parameter is unavailable",
        )),
    }
}

/// Creates an instance with the corresponding native platform backend policy.
/// Retain it in the graphics owner and lend it to each session renderer so native
/// backend loaders remain loaded across session creation and destruction.
pub fn native_wgpu_instance() -> Result<wgpu::Instance, PlatformError> {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    #[cfg(target_os = "windows")]
    {
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::StaticDxc;
    }
    #[cfg(target_os = "linux")]
    {
        descriptor.backends = wgpu::Backends::VULKAN;
    }
    #[cfg(target_os = "macos")]
    {
        descriptor.backends = wgpu::Backends::METAL;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(unavailable(
            "offscreen.platform",
            "offscreen GPU is implemented only for Windows, Linux, and macOS",
        ))
    }
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    Ok(wgpu::Instance::new(descriptor))
}

pub(super) fn native_backend_mask() -> wgpu::Backends {
    #[cfg(target_os = "windows")]
    return wgpu::Backends::DX12;
    #[cfg(target_os = "linux")]
    return wgpu::Backends::VULKAN;
    #[cfg(target_os = "macos")]
    return wgpu::Backends::METAL;
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    return wgpu::Backends::empty();
}

pub(super) fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    ring: &mut ReadbackRing,
) -> Result<CapturedFrame, PlatformError> {
    let row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("offscreen.readback", "frame row overflows"))?;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    if ring.width != width
        || ring.height != height
        || ring.padded_row_bytes != padded
        || ring.slots.len() != READBACK_RING_SIZE
    {
        ring.width = width;
        ring.height = height;
        ring.padded_row_bytes = padded;
        ring.next_slot = 0;
        ring.slots = (0..READBACK_RING_SIZE)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("astra-offscreen-readback-ring"),
                    size: u64::from(padded) * u64::from(height),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();
    }
    let slot = ring.next_slot;
    ring.next_slot = (ring.next_slot + 1) % READBACK_RING_SIZE;
    let buffer = ring
        .slots
        .get(slot)
        .ok_or_else(|| invalid("offscreen.readback", "readback ring slot is unavailable"))?;
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(READBACK_TIMEOUT),
        })
        .map_err(|_| {
            unavailable(
                "offscreen.readback",
                "GPU readback exceeded its bounded timeout",
            )
        })?;
    rx.recv()
        .map_err(|_| unavailable("offscreen.readback", "GPU readback callback was lost"))?
        .map_err(|_| unavailable("offscreen.readback", "GPU readback mapping failed"))?;
    let mapped = slice.get_mapped_range();
    let mut rgba8 = Vec::with_capacity(row as usize * height as usize);
    for source in mapped.chunks_exact(padded as usize).take(height as usize) {
        rgba8.extend_from_slice(&source[..row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(CapturedFrame {
        width,
        height,
        rgba8: rgba8.into(),
    })
}

pub(super) fn validate_native_backend(backend: wgpu::Backend) -> Result<(), PlatformError> {
    let valid = cfg!(target_os = "windows") && backend == wgpu::Backend::Dx12
        || cfg!(target_os = "linux") && backend == wgpu::Backend::Vulkan
        || cfg!(target_os = "macos") && backend == wgpu::Backend::Metal;
    if valid {
        Ok(())
    } else {
        Err(unavailable(
            "offscreen.backend",
            "adapter backend does not match the native platform policy",
        ))
    }
}

pub(super) fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        _ => "unsupported",
    }
}

pub(super) fn device_type_name(device: wgpu::DeviceType) -> &'static str {
    match device {
        wgpu::DeviceType::DiscreteGpu => "discrete_gpu",
        wgpu::DeviceType::IntegratedGpu => "integrated_gpu",
        wgpu::DeviceType::VirtualGpu => "virtual_gpu",
        wgpu::DeviceType::Cpu => "cpu",
        _ => "other",
    }
}

pub(super) fn hash(bytes: &[u8]) -> String {
    astra_core::Hash256::from_sha256(bytes).to_string()
}

pub(super) fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

pub(super) fn unavailable(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}
