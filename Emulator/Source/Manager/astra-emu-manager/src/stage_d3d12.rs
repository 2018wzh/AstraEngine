//! D3D12 delivery copies into the same private input used by CPU presentation.
//! Import initialization is external: wgpu create_texture_from_hal uses
//! TextureClearMode::None; native ready/COMMON is the initialized pixel boundary.
use astra_emu_abi as abi;
use astra_emu_manager_core::model::D3D12FrameOwner;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::HANDLE,
        Graphics::{Direct3D12::*, Dxgi::Common::*},
    },
    core::Interface,
};

type Result<T, E = String> = std::result::Result<T, E>;
static GRAPHICS_FAILED: AtomicBool = AtomicBool::new(false);
pub(super) fn graphics_ready() -> Result<()> {
    require(
        !GRAPHICS_FAILED.load(Ordering::Acquire),
        "graphics cleanup failed; reopening is disabled",
    )
}
fn error(error: impl std::fmt::Display) -> String {
    format!("ASTRA_EMU_D3D12: {error}")
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(error(message))
    }
}
fn luid(device: &ID3D12Device) -> u64 {
    let id = unsafe { device.GetAdapterLuid() };
    (id.HighPart as u32 as u64) << 32 | id.LowPart as u64
}

pub(super) struct Importer {
    pub host: abi::D3D12Host,
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    imports: Vec<((u64, u64, u64), Arc<Imported>)>,
    pending: Vec<Pending>,
}
struct Imported {
    texture: wgpu::Texture,
    ready: ID3D12Fence,
    done: ID3D12Fence,
    handles: [usize; 3],
}
struct Pending {
    imported: Arc<Imported>,
    owner: Arc<D3D12FrameOwner>,
}
impl Pending {
    fn completed(&self) -> bool {
        let done = unsafe { self.imported.done.GetCompletedValue() };
        done == u64::MAX || done >= self.owner.descriptor().lease.done_value
    }
}

impl Importer {
    pub fn new(device: &wgpu::Device) -> Result<Option<Self>> {
        graphics_ready()?;
        let Some(hal) = (unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }) else {
            return Ok(None);
        };
        let raw = hal.raw_device().clone();
        let queue = hal.raw_queue().clone();
        let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
            ..Default::default()
        };
        unsafe {
            raw.CheckFeatureSupport(
                D3D12_FEATURE_FORMAT_SUPPORT,
                (&mut support as *mut D3D12_FEATURE_DATA_FORMAT_SUPPORT).cast(),
                std::mem::size_of_val(&support) as u32,
            )
            .map_err(error)?;
        }
        require(
            support.Support1.contains(D3D12_FORMAT_SUPPORT1_TEXTURE2D),
            "RGBA8 sRGB textures unsupported",
        )?;
        let max = device.limits().max_texture_dimension_2d.min(16384);
        let host = abi::D3D12Host {
            header: abi::D3D12Host::HEADER,
            adapter_luid: luid(&raw),
            max_width: max,
            max_height: max,
            dxgi_format: abi::D3D12_FORMAT_RGBA8_UNORM_SRGB,
            copy_usage: abi::D3D12_COPY_SOURCE | abi::D3D12_COPY_DESTINATION,
        };
        Ok(Some(Self {
            host,
            device: raw,
            queue,
            imports: Vec::with_capacity(3),
            pending: Vec::with_capacity(3),
        }))
    }

    pub fn reap(&mut self) -> Result<()> {
        unsafe {
            self.device.GetDeviceRemovedReason().map_err(error)?;
        }
        self.pending.retain(|pending| !pending.completed());
        Ok(())
    }

    fn import(&mut self, device: &wgpu::Device, owner: &D3D12FrameOwner) -> Result<Arc<Imported>> {
        let frame = owner.descriptor();
        frame
            .validate(&self.host, frame.identity.session_epoch)
            .map_err(error)?;
        let key = (
            frame.identity.session_epoch,
            frame.identity.output_epoch,
            frame.resource_id,
        );
        let handles = [
            frame.lease.resource_handle,
            frame.lease.ready_fence_handle,
            frame.lease.done_fence_handle,
        ];
        if let Some((_, imported)) = self.imports.iter().find(|(existing, _)| *existing == key) {
            require(
                imported.handles == handles,
                "resource identity changed its shared handles",
            )?;
            require(
                imported.texture.width() == frame.info.raster.width
                    && imported.texture.height() == frame.info.raster.height,
                "resource identity changed geometry",
            )?;
            return Ok(imported.clone());
        }
        self.imports
            .retain(|(existing, _)| existing.0 == key.0 && existing.1 == key.1);
        require(
            self.imports.len() < 3,
            "native export pool exceeds three slots",
        )?;
        let mut resource: Option<ID3D12Resource> = None;
        let mut ready: Option<ID3D12Fence> = None;
        let mut done: Option<ID3D12Fence> = None;
        unsafe {
            self.device
                .OpenSharedHandle(HANDLE(handles[0] as *mut _), &mut resource)
                .map_err(error)?;
            self.device
                .OpenSharedHandle(HANDLE(handles[1] as *mut _), &mut ready)
                .map_err(error)?;
            self.device
                .OpenSharedHandle(HANDLE(handles[2] as *mut _), &mut done)
                .map_err(error)?;
        }
        let resource = resource.ok_or_else(|| error("resource handle returned null"))?;
        let ready = ready.ok_or_else(|| error("ready fence handle returned null"))?;
        let done = done.ok_or_else(|| error("done fence handle returned null"))?;
        let desc = unsafe { resource.GetDesc() };
        require(
            desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
                && desc.Width == frame.info.raster.width as u64
                && desc.Height == frame.info.raster.height
                && desc.DepthOrArraySize == 1
                && desc.MipLevels == 1
                && desc.SampleDesc.Count == 1
                && desc.SampleDesc.Quality == 0
                && desc.Format.0 as u32 == frame.dxgi_format
                && desc.Layout == D3D12_TEXTURE_LAYOUT_UNKNOWN
                && desc.Flags == D3D12_RESOURCE_FLAG_NONE,
            "imported resource descriptor differs from negotiated copy-only texture",
        )?;
        let mut flags = D3D12_HEAP_FLAG_NONE;
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        unsafe {
            resource
                .GetHeapProperties(Some(&mut heap), Some(&mut flags))
                .map_err(error)?;
        }
        require(
            flags.contains(D3D12_HEAP_FLAG_SHARED) && heap.Type == D3D12_HEAP_TYPE_DEFAULT,
            "resource is not a shared default-heap allocation",
        )?;
        for child in [
            resource.cast::<ID3D12DeviceChild>().map_err(error)?,
            ready.cast().map_err(error)?,
            done.cast().map_err(error)?,
        ] {
            let mut parent: Option<ID3D12Device> = None;
            unsafe {
                child.GetDevice(&mut parent).map_err(error)?;
            }
            require(
                parent
                    .as_ref()
                    .is_some_and(|parent| parent.as_raw() == self.device.as_raw()),
                "import belongs to another Host device",
            )?;
        }
        for fence in [&ready, &done] {
            let fence1: ID3D12Fence1 = fence.cast().map_err(error)?;
            require(
                unsafe { fence1.GetCreationFlags() }.contains(D3D12_FENCE_FLAG_SHARED),
                "fence is not shared",
            )?;
            require(
                unsafe { fence.GetCompletedValue() } != u64::MAX,
                "shared fence device lost",
            )?;
        }
        let format = match frame.dxgi_format {
            abi::D3D12_FORMAT_RGBA8_UNORM_SRGB => wgpu::TextureFormat::Rgba8UnormSrgb,
            abi::D3D12_FORMAT_BGRA8_UNORM_SRGB => wgpu::TextureFormat::Bgra8UnormSrgb,
            _ => return Err(error("unsupported DXGI format")),
        };
        let size = wgpu::Extent3d {
            width: frame.info.raster.width,
            height: frame.info.raster.height,
            depth_or_array_layers: 1,
        };
        let hal = unsafe {
            wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                format,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            )
        };
        // Native has queued initialization and ready. The imported resource is
        // never used before the Host queue's Wait below. This path must NOT
        // create an ordinary wgpu texture whose lazy clear would erase pixels.
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("astra D3D12 imported native export"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                },
                wgpu::TextureUses::UNINITIALIZED,
            )
        };
        let imported = Arc::new(Imported {
            texture,
            ready,
            done,
            handles,
        });
        self.imports.push((key, imported.clone()));
        Ok(imported)
    }

    pub fn copy(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        owner: &Arc<D3D12FrameOwner>,
        input: &wgpu::Texture,
    ) -> Result<()> {
        self.reap()?;
        require(
            self.pending.len() < 3,
            "Host consumption queue exceeds three slots",
        )?;
        let imported = self.import(device, owner)?;
        let frame = owner.descriptor();
        require(
            input.size() == imported.texture.size()
                && input.format().remove_srgb_suffix()
                    == imported.texture.format().remove_srgb_suffix(),
            "private filter input does not match native export",
        )?;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra native export to private filter input"),
        });
        encoder.copy_texture_to_texture(
            imported.texture.as_image_copy(),
            input.as_image_copy(),
            input.size(),
        );
        encoder.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture: &imported.texture,
                selector: None,
                state: wgpu::TextureUses::PRESENT,
            }),
        );
        let command = encoder.finish();
        owner.claim_consumption().map_err(error)?;
        unsafe {
            self.queue
                .Wait(&imported.ready, frame.lease.ready_value)
                .map_err(error)?;
        }
        queue.submit([command]);
        // Retain before Signal: a failure after submission still must not free
        // imported COM objects while the queue can reference them.
        self.pending.push(Pending {
            imported: imported.clone(),
            owner: owner.clone(),
        });
        unsafe {
            self.queue
                .Signal(&imported.done, frame.lease.done_value)
                .map_err(error)?;
            owner.mark_consumed();
        }
        Ok(())
    }
}

impl Importer {
    pub fn drain(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Err(cause) = unsafe { self.device.GetDeviceRemovedReason() } {
                self.pending.clear();
                GRAPHICS_FAILED.store(true, Ordering::Release);
                return Err(error(cause));
            }
            self.pending.retain(|pending| !pending.completed());
            if self.pending.is_empty() {
                return Ok(());
            }
            if GRAPHICS_FAILED.load(Ordering::Acquire) || Instant::now() >= deadline {
                GRAPHICS_FAILED.store(true, Ordering::Release);
                return Err(error(
                    "consumption fence timeout; bounded GPU resources retained, reopening disabled",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Importer {
    fn drop(&mut self) {
        if !GRAPHICS_FAILED.load(Ordering::Acquire) {
            if let Err(cause) = self.drain() {
                tracing::error!(event = "astra.emu.d3d12.cleanup_failed", error = %cause);
            }
        }
        self.pending.retain(|pending| !pending.completed());
        if !self.pending.is_empty() && unsafe { self.device.GetDeviceRemovedReason() }.is_ok() {
            // Final process-exit failure policy: no detached/live worker. These
            // at most three in-flight imports cannot safely be freed. Keep their
            // ownership until process reclamation and report incomplete cleanup.
            GRAPHICS_FAILED.store(true, Ordering::Release);
            tracing::error!(
                event = "astra.emu.d3d12.cleanup_incomplete",
                retained = self.pending.len()
            );
            std::mem::forget(std::mem::take(&mut self.pending));
        }
    }
}
