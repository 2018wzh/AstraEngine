use crate::{HostWake, modal_service::ModalBridge};
use astra_emu_abi as abi;
use astra_emu_manager_core::model::{
    FamilyPointerRequest, FamilyWindowIcon, ModalRequest, ModalResponse,
};
use parking_lot::{Condvar, Mutex};
use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[derive(Debug, Clone)]
pub enum WindowCommand {
    QueryState,
    Title(String),
    Fullscreen { mode: u32, width: u32, height: u32 },
    Size { width: u32, height: u32 },
    Position { x: i32, y: i32 },
    Visibility(bool),
    Minimized(bool),
    Icon(FamilyWindowIcon),
    Pointer(FamilyPointerRequest),
}
#[derive(Debug, Clone)]
pub struct WindowPresentation {
    pub session_epoch: u64,
    pub context_epoch: u64,
    pub request_id: u64,
    pub command: WindowCommand,
}
#[derive(Debug, Clone, Default)]
pub struct WindowOutcome {
    pub valid_fields: u32,
    pub fullscreen: u32,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub visible: u32,
    pub minimized: u32,
}
struct Pending {
    value: WindowPresentation,
    taken: bool,
    result: Option<Result<WindowOutcome, String>>,
}
struct Shared {
    epoch: u64,
    context: AtomicU64,
    closed: AtomicBool,
    pending: Mutex<Option<Pending>>,
    changed: Condvar,
    modal: ModalBridge,
    wake: HostWake,
}
#[derive(Clone)]
pub struct WindowBridge(Arc<Shared>);
impl WindowBridge {
    pub fn new(epoch: u64, modal: ModalBridge, wake: HostWake) -> Self {
        Self(Arc::new(Shared {
            epoch,
            context: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            pending: Mutex::new(None),
            changed: Condvar::new(),
            modal,
            wake,
        }))
    }
    pub fn sink(&self) -> abi::WindowService {
        abi::WindowService {
            context: Arc::as_ptr(&self.0).cast_mut().cast(),
            request: Some(request),
            invalidate: Some(invalidate),
        }
    }
    pub fn take(&self) -> Option<WindowPresentation> {
        let mut state = self.0.pending.lock();
        let pending = state.as_mut()?;
        if pending.taken {
            return None;
        }
        pending.taken = true;
        Some(pending.value.clone())
    }
    pub fn respond(
        &self,
        value: &WindowPresentation,
        result: Result<WindowOutcome, String>,
    ) -> Result<(), String> {
        let mut state = self.0.pending.lock();
        let pending = state.as_mut().ok_or("ASTRA_EMU_WINDOW_STALE_RESULT")?;
        if pending.value.session_epoch != value.session_epoch
            || pending.value.context_epoch != value.context_epoch
            || pending.value.request_id != value.request_id
            || pending.result.is_some()
        {
            return Err("ASTRA_EMU_WINDOW_STALE_RESULT".into());
        }
        pending.result = Some(result);
        self.0.changed.notify_all();
        Ok(())
    }
    pub fn close(&self) {
        self.0.closed.store(true, Ordering::Release);
        self.0.modal.cancel_pending();
        self.0.pending.lock().take();
        self.0.changed.notify_all();
    }
}
unsafe extern "C" fn invalidate(context: *mut c_void, epoch: u64, next: u64) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() {
            return;
        }
        let state = unsafe { &*context.cast::<Shared>() };
        if epoch != state.epoch {
            return;
        }
        state.context.store(next, Ordering::Release);
        state.pending.lock().take();
        state.changed.notify_all();
        state.modal.invalidate_pending();
    }));
}
unsafe extern "C" fn request(
    context: *mut c_void,
    request: *const abi::WindowRequest,
    result: *mut abi::WindowResult,
    error: *mut abi::Error,
) -> abi::Status {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        execute(context, request, result)
    })) {
        Ok(Ok(())) => abi::OK,
        Ok(Err((status, code))) => {
            if let Some(error) = unsafe { error.as_mut() } {
                error.set(code, "native window service request failed");
            }
            status
        }
        Err(_) => {
            if let Some(error) = unsafe { error.as_mut() } {
                error.set("ASTRA_EMU_WINDOW_PANIC", "window service panicked");
            }
            abi::PANICKED
        }
    }
}
unsafe fn execute(
    context: *mut c_void,
    raw: *const abi::WindowRequest,
    result: *mut abi::WindowResult,
) -> Result<(), (abi::Status, &'static str)> {
    let invalid = || (abi::INVALID_ARGUMENT, "ASTRA_EMU_WINDOW_REQUEST");
    let state = unsafe { context.cast::<Shared>().as_ref() }.ok_or_else(invalid)?;
    let raw = unsafe { raw.as_ref() }.ok_or_else(invalid)?;
    if raw.context_epoch == 0 {
        return Err(invalid());
    }
    let _ =
        state
            .context
            .compare_exchange(0, raw.context_epoch, Ordering::AcqRel, Ordering::Acquire);
    let output = unsafe { result.as_mut() }.ok_or_else(invalid)?;
    let stale = || {
        state.closed.load(Ordering::Acquire)
            || raw.session_epoch != state.epoch
            || raw.context_epoch != state.context.load(Ordering::Acquire)
    };
    if stale() {
        return Err((abi::CANCELLED, "ASTRA_EMU_WINDOW_CANCELLED"));
    }
    if raw.request_id == 0 {
        return Err(invalid());
    }
    output.selected_button = 0;
    output.reserved = 0;
    output.valid_fields = 0;
    if raw.kind == abi::WINDOW_MODAL {
        if raw.flags & !abi::MODAL_TEXT_INPUT != 0 {
            return Err(invalid());
        }
        let input_dialog = raw.flags & abi::MODAL_TEXT_INPUT != 0;
        if input_dialog && output.input.capacity < abi::MAX_MODAL_TEXT {
            output.input.len = abi::MAX_MODAL_TEXT;
            return Err((abi::CAPACITY, "ASTRA_EMU_MODAL_INPUT_CAPACITY"));
        }
        if input_dialog && output.input.data.is_null() {
            return Err(invalid());
        }
        output.input.len = 0;
        let buttons = unsafe { abi::borrowed_array(raw.buttons, raw.button_count, 16) }
            .map_err(|_| invalid())?;
        if buttons.is_empty() {
            return Err(invalid());
        }
        let mut labels = Vec::with_capacity(buttons.len());
        for (i, button) in buttons.iter().enumerate() {
            if button.reserved != 0 || buttons[..i].iter().any(|b| b.id == button.id) {
                return Err(invalid());
            }
            labels.push(
                unsafe { button.label.text(256) }
                    .map_err(|_| invalid())?
                    .to_owned(),
            );
        }
        let message = unsafe { raw.text.text(abi::MAX_MODAL_TEXT) }
            .map_err(|_| invalid())?
            .to_owned();
        let caption = unsafe { raw.caption.text(abi::MAX_MODAL_TEXT) }
            .map_err(|_| invalid())?
            .to_owned();
        let initial_text = if input_dialog {
            Some(
                unsafe { raw.initial_text.text(abi::MAX_MODAL_TEXT) }
                    .map_err(|_| invalid())?
                    .to_owned(),
            )
        } else {
            None
        };
        match state
            .modal
            .request_if(
                ModalRequest {
                    caption,
                    message,
                    buttons: labels,
                    initial_text,
                },
                || !stale(),
            )
            .map_err(|_| (abi::CANCELLED, "ASTRA_EMU_MODAL_CANCELLED"))?
        {
            ModalResponse::Submitted { button_index, text } if !stale() => {
                if let Some(text) = text {
                    unsafe {
                        std::ptr::copy_nonoverlapping(text.as_ptr(), output.input.data, text.len());
                    }
                    output.input.len = text.len();
                }
                output.selected_button = buttons[button_index as usize].id;
                output.valid_fields = abi::WINDOW_RESULT_MODAL;
                return Ok(());
            }
            _ => return Err((abi::CANCELLED, "ASTRA_EMU_MODAL_CANCELLED")),
        }
    }
    let command = match raw.kind {
        abi::WINDOW_QUERY_STATE => {
            if raw.flags != 0 {
                return Err(invalid());
            }
            WindowCommand::QueryState
        }
        abi::WINDOW_TITLE => WindowCommand::Title(
            unsafe { raw.text.text(4096) }
                .map_err(|_| invalid())?
                .to_owned(),
        ),
        abi::WINDOW_FULLSCREEN if raw.flags <= 2 => WindowCommand::Fullscreen {
            mode: raw.flags,
            width: raw.width,
            height: raw.height,
        },
        abi::WINDOW_SIZE
            if raw.width != 0 && raw.height != 0 && raw.width <= 16384 && raw.height <= 16384 =>
        {
            WindowCommand::Size {
                width: raw.width,
                height: raw.height,
            }
        }
        abi::WINDOW_POSITION => WindowCommand::Position { x: raw.x, y: raw.y },
        abi::WINDOW_VISIBILITY if raw.flags <= 1 => WindowCommand::Visibility(raw.flags != 0),
        abi::WINDOW_MINIMIZED if raw.flags <= 1 => WindowCommand::Minimized(raw.flags != 0),
        abi::WINDOW_ICON => {
            let bytes = unsafe { raw.pixels.as_slice(256 * 256 * 4) }.map_err(|_| invalid())?;
            let icon = FamilyWindowIcon {
                width: raw.width,
                height: raw.height,
                rgba: bytes.to_vec(),
            };
            icon.validate().map_err(|_| invalid())?;
            WindowCommand::Icon(icon)
        }
        abi::WINDOW_POINTER
            if raw.flags & !63 == 0
                && !(raw.flags & abi::POINTER_SET_POSITION != 0
                    && raw.flags & (abi::POINTER_SET_X | abi::POINTER_SET_Y) != 0) =>
        {
            WindowCommand::Pointer(FamilyPointerRequest {
                logical_x: (raw.flags & (abi::POINTER_SET_POSITION | abi::POINTER_SET_X) != 0)
                    .then_some(raw.x),
                logical_y: (raw.flags & (abi::POINTER_SET_POSITION | abi::POINTER_SET_Y) != 0)
                    .then_some(raw.y),
                visible: (raw.flags & abi::POINTER_SET_VISIBILITY != 0)
                    .then_some(raw.flags & abi::POINTER_VISIBLE != 0),
                autohide_ms: (raw.flags & abi::POINTER_SET_AUTOHIDE != 0)
                    .then_some(raw.autohide_ms),
            })
        }
        _ => return Err((abi::UNSUPPORTED, "ASTRA_EMU_WINDOW_UNSUPPORTED")),
    };
    let mut pending = state.pending.lock();
    if stale() {
        return Err((abi::CANCELLED, "ASTRA_EMU_WINDOW_CANCELLED"));
    }
    if pending.is_some() {
        return Err((abi::CAPACITY, "ASTRA_EMU_WINDOW_BUSY"));
    }
    *pending = Some(Pending {
        value: WindowPresentation {
            session_epoch: raw.session_epoch,
            context_epoch: raw.context_epoch,
            request_id: raw.request_id,
            command,
        },
        taken: false,
        result: None,
    });
    drop(pending);
    (state.wake)();
    let mut pending = state.pending.lock();
    loop {
        if stale() {
            pending.take();
            return Err((abi::CANCELLED, "ASTRA_EMU_WINDOW_CANCELLED"));
        }
        let value = pending
            .as_mut()
            .ok_or((abi::CANCELLED, "ASTRA_EMU_WINDOW_CANCELLED"))?;
        if let Some(result) = value.result.take() {
            pending.take();
            let result = result.map_err(|_| (abi::CORE_ERROR, "ASTRA_EMU_WINDOW_EXECUTION"))?;
            output.valid_fields = result.valid_fields;
            output.fullscreen = result.fullscreen;
            output.width = result.width;
            output.height = result.height;
            output.x = result.x;
            output.y = result.y;
            output.visible = result.visible;
            output.minimized = result.minimized;
            return Ok(());
        }
        state.changed.wait(&mut pending);
    }
}

#[cfg(test)]
#[path = "window_service_tests.rs"]
mod tests;
