use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use astra_emu_family_api::{FamilyResult, FrameInfo, FramePublisher};
use siglus_scene_vm::render::Renderer;

use crate::error;

const READBACK_TIMEOUT: Duration = Duration::from_secs(5);

/// CPU storage remains reserved from map submission through publication.
/// A held lease can delay submission, but cannot consume the GPU map deadline.
pub(super) struct FrameReadback {
    pub(super) published: FramePublisher,
    pub(super) generation: u64,
    pixels: [Option<Arc<Vec<u8>>>; 3],
    reserved: Option<usize>,
    deadline: Option<Instant>,
}

impl FrameReadback {
    pub(super) fn new(published: FramePublisher) -> Self {
        Self {
            published,
            generation: 0,
            pixels: Default::default(),
            reserved: None,
            deadline: None,
        }
    }

    pub(super) fn cancel(&mut self, renderer: &mut Renderer) {
        renderer.cancel_frame_readback();
        self.reserved = None;
        self.deadline = None;
    }

    pub(super) fn capture(&mut self, renderer: &mut Renderer) -> FamilyResult<Option<FrameInfo>> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.cancel(renderer);
            return Err(error::invalid(
                "ASTRA_EMU_SIGLUS_READBACK_TIMEOUT",
                "native frame readback exceeded five seconds",
            ));
        }
        let index = if let Some(index) = self.reserved {
            index
        } else {
            let Some(index) = self.pixels.iter().position(|slot| {
                slot.as_ref()
                    .is_none_or(|pixels| Arc::strong_count(pixels) == 1)
            }) else {
                return Ok(None);
            };
            self.reserved = Some(index);
            self.deadline = Some(Instant::now() + READBACK_TIMEOUT);
            index
        };
        let slot = self.pixels[index].get_or_insert_with(|| Arc::new(Vec::new()));
        let pixels = Arc::get_mut(slot).expect("reserved CPU slot has no lease");
        let result = renderer.poll_frame_rgba(pixels);
        let (width, height, generation) = match result {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(None),
            Err(failure) => {
                self.cancel(renderer);
                return Err(error::engine("renderer.readback", failure));
            }
        };
        self.reserved = None;
        self.deadline = None;
        let info = super::frame_info(width, height)?;
        self.published
            .publish(Arc::clone(slot), |pixels| pixels.as_slice(), info)?;
        self.generation = generation;
        Ok(Some(info))
    }
}

#[cfg(test)]
#[path = "readback_tests.rs"]
mod tests;
