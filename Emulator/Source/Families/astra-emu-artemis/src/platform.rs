use astra_emu_abi::*;
use crate::{error::{self, NativeResult}, media::WindowCommand, session::Platform};
pub(crate) fn request(platform: Platform, command: WindowCommand, epoch: u64, context: u64, id: u64) -> NativeResult<()> {
    let callback = platform.0.request.ok_or_else(|| error::invalid("ASTRA_EMU_ARTEMIS_PLATFORM_UNAVAILABLE", "native window service is unavailable"))?;
    let mut request = WindowRequest { session_epoch: epoch, context_epoch: context, request_id: id, kind: 0, flags: 0, text: Bytes::EMPTY, pixels: Bytes::EMPTY, width: 0, height: 0, x: 0, y: 0, autohide_ms: 0, buttons: std::ptr::null(), button_count: 0, caption: Bytes::EMPTY, initial_text: Bytes::EMPTY };
    match &command {
        WindowCommand::Title(title) => { request.kind = WINDOW_TITLE; request.text = Bytes::new(title.as_bytes()); }
        WindowCommand::Pointer { x, y, visible, autohide_ms } => {
            request.kind = WINDOW_POINTER;
            if let Some(x) = x { request.flags |= POINTER_SET_X; request.x = *x; }
            if let Some(y) = y { request.flags |= POINTER_SET_Y; request.y = *y; }
            if let Some(visible) = visible { request.flags |= POINTER_SET_VISIBILITY; if *visible { request.flags |= POINTER_VISIBLE; } }
            if let Some(ms) = autohide_ms { request.flags |= POINTER_SET_AUTOHIDE; request.autohide_ms = *ms; }
        }
    }
    let mut result = WindowResult { selected_button: 0, reserved: 0, valid_fields: 0, fullscreen: 0, width: 0, height: 0, x: 0, y: 0, input: TextBuffer { data: std::ptr::null_mut(), capacity: 0, len: 0 }, visible: 0, minimized: 0 };
    let mut error = Error::default();
    let status = unsafe { callback(platform.0.context, &request, &mut result, &mut error) };
    if status != OK { return Err(crate::error::invalid("ASTRA_EMU_ARTEMIS_PLATFORM", format!("native platform request failed (status {status}): {}", String::from_utf8_lossy(&error.message[..(error.message_len as usize).min(error.message.len())])))); }
    Ok(())
}
