use super::*;

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
pub(super) struct CapturedFrame {
    pub(super) generation: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) stride: u32,
    pub(super) pixels: Arc<Vec<u8>>,
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
            validate_alpha(source_row, alpha)?;
            target_row.copy_from_slice(source_row);
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

    pub(super) fn snapshot(&self) -> Result<Option<CapturedFrame>, String> {
        self.state
            .lock()
            .map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.frame.clone())
    }
}

fn validate_alpha(row: &[u8], alpha: FrameAlpha) -> FamilyResult<()> {
    if matches!(alpha, FrameAlpha::Opaque)
        && row
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != u8::MAX)
    {
        return Err(astra_emu_family_api::FamilyError::invalid(
            "ASTRA_EMU_HOST_FRAME_ALPHA",
            "opaque family frame contains a non-opaque pixel",
        ));
    }
    Ok(())
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
