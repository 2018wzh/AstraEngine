use super::*;
use astra_emu_manager_core::model::{FamilyResult, FrameIdentity, FrameLease};

/// Latest complete immutable CPU lease. No repacking, alpha scan or pixel copy.
#[derive(Clone, Default)]
pub(crate) struct FrameMailbox {
    state: Arc<Mutex<FrameMailboxState>>,
}

#[derive(Default)]
struct FrameMailboxState {
    session_epoch: Option<u64>,
    frame: Option<FrameLease>,
    displayed: Option<(FrameIdentity, astra_emu_manager_core::model::FrameInfo)>,
}

impl FrameMailbox {
    pub(crate) fn new() -> Self { Self::default() }

    pub(crate) fn activate(&self, session_epoch: u64) -> FamilyResult<()> {
        FrameIdentity { session_epoch, output_epoch: 0, generation: 0 }.validate(false)?;
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        state.session_epoch = Some(session_epoch);
        state.frame = None;
        state.displayed = None;
        Ok(())
    }

    pub(crate) fn publish(&self, frame: FrameLease) -> FamilyResult<()> {
        frame.validate()?;
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        if state.session_epoch != Some(frame.identity.session_epoch) {
            return Err(astra_emu_manager_core::model::FamilyError::invalid(
                "ASTRA_EMU_HOST_FRAME_EPOCH", "frame belongs to an inactive session",
            ));
        }
        if let Some(previous) = &state.frame {
            if frame.identity.output_epoch < previous.identity.output_epoch || (frame.identity.output_epoch == previous.identity.output_epoch && frame.identity.generation < previous.identity.generation) {
                return Err(astra_emu_manager_core::model::FamilyError::invalid(
                    "ASTRA_EMU_HOST_FRAME_GENERATION", "completed frame generation moved backwards",
                ));
            }
            if frame.identity == previous.identity {
                if frame.info != previous.info || frame.pixels().as_ptr() != previous.pixels().as_ptr() {
                    return Err(astra_emu_manager_core::model::FamilyError::invalid(
                        "ASTRA_EMU_HOST_FRAME_GENERATION", "same frame generation refers to different output",
                    ));
                }
                return Ok(());
            }
        }
        state.frame = Some(frame);
        Ok(())
    }

    pub(crate) fn identity(&self) -> Result<Option<FrameIdentity>, String> {
        self.state.lock().map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.frame.as_ref().map(|frame| frame.identity))
    }
    pub(crate) fn presented(&self, identity: FrameIdentity, info: astra_emu_manager_core::model::FrameInfo) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK")?;
        if state.session_epoch == Some(identity.session_epoch) { state.displayed = Some((identity, info)); }
        Ok(())
    }
    pub(crate) fn displayed_identity(&self) -> Result<Option<FrameIdentity>, String> {
        self.state.lock().map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned()).map(|state| state.displayed.map(|v| v.0))
    }
    pub(crate) fn displayed(&self) -> Result<Option<(FrameIdentity, astra_emu_manager_core::model::FrameInfo)>, String> {
        self.state.lock().map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned()).map(|state| state.displayed)
    }

    /// Late shutdown from an old owner cannot clear the new session baseline.
    pub(crate) fn clear(&self, session_epoch: u64) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned())?;
        if state.session_epoch == Some(session_epoch) {
            state.frame = None;
            state.session_epoch = None;
            state.displayed = None;
        }
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Result<Option<FrameLease>, String> {
        self.state.lock().map_err(|_| "ASTRA_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.frame.clone())
    }
}

fn lock_error() -> astra_emu_manager_core::model::FamilyError {
    astra_emu_manager_core::model::FamilyError::invalid("ASTRA_EMU_HOST_FRAME_LOCK", "frame mailbox lock is poisoned")
}
