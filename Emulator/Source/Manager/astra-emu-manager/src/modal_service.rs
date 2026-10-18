use astra_emu_manager_core::model::{FamilyError, FamilyResult, ModalRequest, ModalResponse};
use parking_lot::{Condvar, Mutex};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::HostWake;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalId {
    pub epoch: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone)]
pub struct ModalPresentation {
    pub id: ModalId,
    pub request: Arc<ModalRequest>,
}

#[derive(Debug, Clone)]
pub enum ModalUiUpdate {
    Show(ModalPresentation),
    Dismiss(ModalId),
}

struct Pending {
    id: ModalId,
    request: Arc<ModalRequest>,
    response: Option<ModalResponse>,
    presented: bool,
}

#[derive(Default)]
struct State {
    sequence: u64,
    pending: Option<Pending>,
    dismissed: Option<ModalId>,
}

struct Shared {
    epoch: u64,
    state: Mutex<State>,
    completed: Condvar,
    cancelled: AtomicBool,
    wake: HostWake,
}

/// One session, one outstanding request. No worker or per-frame payload copy.
#[derive(Clone)]
pub struct ModalBridge(Arc<Shared>);

impl ModalBridge {
    pub fn new(epoch: u64, wake: HostWake) -> FamilyResult<Self> {
        if epoch == 0 {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_MODAL_EPOCH",
                "modal service requires a nonzero session epoch",
            ));
        }
        Ok(Self(Arc::new(Shared {
            epoch,
            state: Mutex::new(State::default()),
            completed: Condvar::new(),
            cancelled: AtomicBool::new(false),
            wake,
        })))
    }

    pub fn epoch(&self) -> u64 {
        self.0.epoch
    }

    pub fn take_ui_update(&self) -> Option<ModalUiUpdate> {
        let mut state = self.0.state.lock();
        if let Some(id) = state.dismissed.take() {
            return Some(ModalUiUpdate::Dismiss(id));
        }
        let pending = state.pending.as_mut()?;
        if pending.presented || pending.response.is_some() {
            return None;
        }
        pending.presented = true;
        Some(ModalUiUpdate::Show(ModalPresentation {
            id: pending.id,
            request: Arc::clone(&pending.request),
        }))
    }

    pub fn respond(&self, id: ModalId, response: ModalResponse) -> FamilyResult<()> {
        let mut state = self.0.state.lock();
        if self.is_cancelled() || id.epoch != self.0.epoch {
            return Err(stale_response());
        }
        let pending = state.pending.as_mut().ok_or_else(stale_response)?;
        if pending.id != id || pending.response.is_some() {
            return Err(stale_response());
        }
        response.validate_for_request(&pending.request)?;
        pending.response = Some(response);
        state.dismissed = Some(id);
        self.0.completed.notify_all();
        drop(state);
        (self.0.wake)();
        Ok(())
    }

    pub fn cancel_pending(&self) {
        let mut state = self.0.state.lock();
        if self.0.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(pending) = state.pending.take() {
            state.dismissed = Some(pending.id);
        }
        self.0.completed.notify_all();
        drop(state);
        (self.0.wake)();
    }

    pub fn request(&self, request: ModalRequest) -> FamilyResult<ModalResponse> {
        self.request_if(request, || true)
    }
    pub fn request_if(
        &self,
        request: ModalRequest,
        valid: impl Fn() -> bool,
    ) -> FamilyResult<ModalResponse> {
        request.validate()?;
        let mut state = self.0.state.lock();
        if self.is_cancelled() || !valid() {
            return Ok(ModalResponse::Cancelled);
        }
        if state.pending.is_some() {
            return Err(FamilyError::new(
                "ASTRA_EMU_MODAL_QUEUE_FULL",
                "the session already has an outstanding modal request",
            ));
        }
        state.sequence = state.sequence.checked_add(1).ok_or_else(|| {
            FamilyError::new("ASTRA_EMU_MODAL_SEQUENCE", "modal sequence exhausted")
        })?;
        let id = ModalId {
            epoch: self.0.epoch,
            sequence: state.sequence,
        };
        state.pending = Some(Pending {
            id,
            request: Arc::new(request),
            response: None,
            presented: false,
        });
        drop(state);
        (self.0.wake)();
        let mut state = self.0.state.lock();
        loop {
            if self.is_cancelled() || !valid() {
                return Ok(ModalResponse::Cancelled);
            }
            let pending = state.pending.as_mut().ok_or_else(stale_response)?;
            if let Some(response) = pending.response.take() {
                state.pending = None;
                return Ok(response);
            }
            self.0.completed.wait(&mut state);
        }
    }
}

fn stale_response() -> FamilyError {
    FamilyError::new(
        "ASTRA_EMU_MODAL_STALE_RESPONSE",
        "modal request is no longer active",
    )
}

impl ModalBridge {
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn invalidate_pending(&self) {
        let mut state = self.0.state.lock();
        if let Some(pending) = state.pending.take() {
            state.dismissed = Some(pending.id);
        }
        self.0.completed.notify_all();
        drop(state);
        (self.0.wake)();
    }
}

#[cfg(test)]
#[path = "modal_service_tests.rs"]
mod tests;
