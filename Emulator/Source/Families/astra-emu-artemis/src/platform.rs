use crate::{
    error::{self, NativeResult},
    media::WindowCommand,
    session::Platform,
};
use astra_emu_abi::*;
use std::ops::ControlFlow;
pub(crate) fn request(
    platform: Platform,
    command: WindowCommand,
    epoch: u64,
    context: u64,
    id: u64,
) -> NativeResult<ControlFlow<()>> {
    let operation = match &command {
        WindowCommand::Title(_) => "window.title",
        WindowCommand::Pointer { .. } => "window.pointer",
    };
    let mut request = window_request(epoch, context, id, 0);
    match &command {
        WindowCommand::Title(title) => {
            request.kind = WINDOW_TITLE;
            request.text = Bytes::new(title.as_bytes());
        }
        WindowCommand::Pointer {
            x,
            y,
            visible,
            autohide_ms,
        } => {
            request.kind = WINDOW_POINTER;
            if let Some(x) = x {
                request.flags |= POINTER_SET_X;
                request.x = *x;
            }
            if let Some(y) = y {
                request.flags |= POINTER_SET_Y;
                request.y = *y;
            }
            if let Some(visible) = visible {
                request.flags |= POINTER_SET_VISIBILITY;
                if *visible {
                    request.flags |= POINTER_VISIBLE;
                }
            }
            if let Some(ms) = autohide_ms {
                request.flags |= POINTER_SET_AUTOHIDE;
                request.autohide_ms = *ms;
            }
        }
    }
    invoke(platform, &request, operation).map(|result| result.map_continue(|_| ()))
}

pub(crate) fn query_state(
    platform: Platform,
    epoch: u64,
    context: u64,
    id: u64,
) -> NativeResult<ControlFlow<(), i32>> {
    let request = window_request(epoch, context, id, WINDOW_QUERY_STATE);
    let result = match invoke(platform, &request, "window.query_state")? {
        ControlFlow::Continue(result) => result,
        ControlFlow::Break(()) => return Ok(ControlFlow::Break(())),
    };
    state_flags(result.fullscreen, result.minimized, result.valid_fields).map(ControlFlow::Continue)
}

fn state_flags(mode: u32, minimized: u32, valid_fields: u32) -> NativeResult<i32> {
    let required = WINDOW_RESULT_MODE | WINDOW_RESULT_MINIMIZED;
    if valid_fields & required != required
        || !matches!(
            mode,
            FULLSCREEN_WINDOWED | FULLSCREEN_BORDERLESS | FULLSCREEN_EXCLUSIVE
        )
        || minimized > 1
    {
        return Err(error::invalid(
            "ASTRA_EMU_ARTEMIS_WINDOW_STATE",
            "window.query_state did not return valid mode and minimized fields",
        ));
    }
    Ok(i32::from(mode != FULLSCREEN_WINDOWED) | ((minimized as i32) << 1))
}

fn window_request(epoch: u64, context: u64, id: u64, kind: u32) -> WindowRequest {
    WindowRequest {
        session_epoch: epoch,
        context_epoch: context,
        request_id: id,
        kind,
        flags: 0,
        text: Bytes::EMPTY,
        pixels: Bytes::EMPTY,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        autohide_ms: 0,
        buttons: std::ptr::null(),
        button_count: 0,
        caption: Bytes::EMPTY,
        initial_text: Bytes::EMPTY,
    }
}

fn invoke(
    platform: Platform,
    request: &WindowRequest,
    operation: &str,
) -> NativeResult<ControlFlow<(), WindowResult>> {
    let callback = platform.0.request.ok_or_else(|| {
        error::invalid(
            "ASTRA_EMU_ARTEMIS_PLATFORM_UNAVAILABLE",
            format!("native window service is unavailable for {operation}"),
        )
    })?;
    let mut result = WindowResult {
        selected_button: 0,
        reserved: 0,
        valid_fields: 0,
        fullscreen: 0,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        input: TextBuffer {
            data: std::ptr::null_mut(),
            capacity: 0,
            len: 0,
        },
        visible: 0,
        minimized: 0,
    };
    let mut error = Error::default();
    let status = unsafe { callback(platform.0.context, request, &mut result, &mut error) };
    if status == CANCELLED {
        return Ok(ControlFlow::Break(()));
    }
    if status != OK {
        return Err(crate::error::invalid(
            "ASTRA_EMU_ARTEMIS_PLATFORM",
            format!(
                "native platform request {operation} failed (status {status}): {}",
                String::from_utf8_lossy(
                    &error.message[..(error.message_len as usize).min(error.message.len())]
                )
            ),
        ));
    }
    Ok(ControlFlow::Continue(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_requests_preserve_partial_updates_at_the_host_boundary() {
        #[derive(Debug, Default, PartialEq)]
        struct Capture {
            kind: u32,
            flags: u32,
            x: i32,
            y: i32,
            autohide_ms: u64,
        }
        unsafe extern "C" fn capture(
            context: *mut std::ffi::c_void,
            request: *const WindowRequest,
            _: *mut WindowResult,
            _: *mut Error,
        ) -> Status {
            let request = unsafe { &*request };
            unsafe {
                *context.cast::<Capture>() = Capture {
                    kind: request.kind,
                    flags: request.flags,
                    x: request.x,
                    y: request.y,
                    autohide_ms: request.autohide_ms,
                };
            }
            OK
        }
        for visible in [None, Some(false), Some(true)] {
            for autohide_ms in [None, Some(0), Some(1000)] {
                let mut actual = Capture::default();
                let platform = Platform(
                    WindowService {
                        context: (&mut actual as *mut Capture).cast(),
                        request: Some(capture),
                        invalidate: None,
                    },
                    CancellationService {
                        context: std::ptr::null_mut(),
                        is_cancelled: None,
                    },
                );
                assert!(request(
                    platform,
                    WindowCommand::Pointer {
                        x: Some(12),
                        y: None,
                        visible,
                        autohide_ms,
                    },
                    1,
                    1,
                    1,
                )
                .unwrap()
                .is_continue());
                let mut flags = POINTER_SET_X;
                if let Some(visible) = visible {
                    flags |= POINTER_SET_VISIBILITY;
                    if visible {
                        flags |= POINTER_VISIBLE;
                    }
                }
                if autohide_ms.is_some() {
                    flags |= POINTER_SET_AUTOHIDE;
                }
                assert_eq!(
                    actual,
                    Capture {
                        kind: WINDOW_POINTER,
                        flags,
                        x: 12,
                        y: 0,
                        autohide_ms: autohide_ms.unwrap_or(0),
                    }
                );
            }
        }
    }

    #[test]
    fn native_window_flags_use_only_actual_mode_and_minimized() {
        let valid = WINDOW_RESULT_MODE | WINDOW_RESULT_MINIMIZED;
        for mode in [
            FULLSCREEN_WINDOWED,
            FULLSCREEN_BORDERLESS,
            FULLSCREEN_EXCLUSIVE,
        ] {
            for minimized in 0..=1 {
                let expected = i32::from(mode != FULLSCREEN_WINDOWED) | (minimized << 1) as i32;
                assert_eq!(state_flags(mode, minimized, valid).unwrap(), expected);
            }
        }
    }

    #[test]
    fn missing_or_invalid_window_state_is_not_invented() {
        let valid = WINDOW_RESULT_MODE | WINDOW_RESULT_MINIMIZED;
        for (mode, minimized, fields) in [
            (FULLSCREEN_WINDOWED, 0, WINDOW_RESULT_MODE),
            (FULLSCREEN_WINDOWED, 0, WINDOW_RESULT_MINIMIZED),
            (FULLSCREEN_WINDOWED, 0, 0),
            (3, 0, valid),
            (FULLSCREEN_WINDOWED, 2, valid),
        ] {
            let error = state_flags(mode, minimized, fields).unwrap_err();
            assert_eq!(error.code, "ASTRA_EMU_ARTEMIS_WINDOW_STATE");
            assert!(error.message.contains("window.query_state"));
        }
    }

    #[test]
    fn window_query_without_host_fails_explicitly() {
        let platform = Platform(
            WindowService {
                context: std::ptr::null_mut(),
                request: None,
                invalidate: None,
            },
            CancellationService {
                context: std::ptr::null_mut(),
                is_cancelled: None,
            },
        );
        let error = query_state(platform, 1, 1, 1).unwrap_err();
        assert_eq!(error.code, "ASTRA_EMU_ARTEMIS_PLATFORM_UNAVAILABLE");
        assert!(error.message.contains("window.query_state"));
    }

    #[test]
    fn cancelled_window_requests_remain_control_flow() {
        unsafe extern "C" fn cancelled(
            _: *mut std::ffi::c_void,
            _: *const WindowRequest,
            _: *mut WindowResult,
            _: *mut Error,
        ) -> Status {
            CANCELLED
        }
        let platform = Platform(
            WindowService {
                context: std::ptr::null_mut(),
                request: Some(cancelled),
                invalidate: None,
            },
            CancellationService {
                context: std::ptr::null_mut(),
                is_cancelled: None,
            },
        );
        assert!(query_state(platform, 1, 1, 1).unwrap().is_break());
        assert!(
            request(platform, WindowCommand::Title("title".into()), 1, 1, 2)
                .unwrap()
                .is_break()
        );
    }
}
