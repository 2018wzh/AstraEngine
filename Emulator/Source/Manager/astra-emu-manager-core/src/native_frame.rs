use crate::model::*;
use astra_emu_abi as abi;

pub(super) unsafe fn take_surface(
    surface: abi::FrameSurface,
    epoch: u64,
    after: Option<FrameIdentity>,
) -> FamilyResult<FramePoll> {
    let identity = surface.identity.into();
    if surface.state != abi::FRAME_READY {
        let validation = surface.validate_cpu(epoch);
        if let Some(release) = surface.cpu.release {
            unsafe { release(surface.cpu.owner) };
        }
        validation
            .map_err(|_| FamilyError::invalid("ASTRA_EMU_CORE_SURFACE", "invalid CPU surface"))?;
        return match surface.state {
            abi::FRAME_PENDING => Ok(FramePoll::Pending(identity)),
            abi::FRAME_UNCHANGED if after == Some(identity) => Ok(FramePoll::Unchanged(identity)),
            _ => Err(FamilyError::invalid(
                "ASTRA_EMU_FRAME_BASELINE",
                "unchanged surface has no matching baseline",
            )),
        };
    }
    // Shared ABI owner adopts the release even if validation fails; its storage
    // is immutable and independently valid after native close.
    let owner = unsafe { abi::CpuFrame::adopt(surface, epoch) }
        .map_err(|_| FamilyError::invalid("ASTRA_EMU_CORE_SURFACE", "invalid CPU surface"))?;
    let surface = owner.info();
    let timestamp = owner.timestamp_ns();
    let info = surface_info(&surface)?;
    let mut frame = FrameLease::from_owner(owner, abi::CpuFrame::pixels, info, identity)?;
    frame.timestamp_ns = timestamp;
    Ok(FramePoll::Ready(frame))
}

fn surface_info(surface: &abi::SurfaceInfo) -> FamilyResult<FrameInfo> {
    let alpha = match surface.alpha {
        abi::ALPHA_OPAQUE => FrameAlpha::Opaque,
        abi::ALPHA_IGNORED => FrameAlpha::Ignored,
        _ => {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_ALPHA",
                "complete game frame requires opaque or ignored alpha",
            ));
        }
    };
    let format = match (surface.pixel_format, surface.color) {
        (abi::PIXEL_RGBA8, abi::COLOR_SRGB) => FrameFormat::Rgba8Srgb { alpha },
        (abi::PIXEL_BGRA8, abi::COLOR_SRGB) => FrameFormat::Bgra8Srgb { alpha },
        (abi::PIXEL_RGBA8, abi::COLOR_LINEAR_SRGB) => FrameFormat::Rgba8Linear { alpha },
        (abi::PIXEL_BGRA8, abi::COLOR_LINEAR_SRGB) => FrameFormat::Bgra8Linear { alpha },
        _ => {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_FORMAT",
                "unsupported surface format",
            ));
        }
    };
    let info = FrameInfo {
        width: surface.raster.width,
        height: surface.raster.height,
        logical_width: surface.logical.width,
        logical_height: surface.logical.height,
        stride: u32::try_from(surface.stride).map_err(|_| {
            FamilyError::invalid("ASTRA_EMU_FRAME_STRIDE", "stride exceeds host limit")
        })?,
        format,
        rows: if surface.rows == abi::ROWS_TOP_DOWN {
            FrameRowOrder::TopToBottom
        } else {
            FrameRowOrder::BottomToTop
        },
    };
    Ok(info)
}
