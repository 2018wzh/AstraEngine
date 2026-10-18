//! Input is copied at the ABI boundary and consumed by the native owner.
use crate::error::{self, NativeResult};
use art3m1s_core::runtime::CoreRuntime;
use astra_emu_abi::*;
pub(crate) enum Input {
    Key(u32, bool),
    Move(f64, f64, FrameIdentity),
    Button(u32, bool, f64, f64, FrameIdentity),
    Window(WindowState),
}
pub(crate) unsafe fn decode(event: &InputEvent) -> NativeResult<Input> {
    let invalid = || {
        error::invalid(
            "ASTRA_EMU_ARTEMIS_INPUT",
            "unsupported or invalid native input",
        )
    };
    if event.pressed > 1 || event.repeat > 1 {
        return Err(invalid());
    }
    Ok(match event.kind {
        INPUT_KEY => {
            let name = unsafe { event.physical_key.text(128) }.map_err(|_| invalid())?;
            Input::Key(key(name).ok_or_else(invalid)?, event.pressed != 0)
        }
        INPUT_POINTER_MOVE | INPUT_POINTER_BUTTON => {
            if !event.x.is_finite() || !event.y.is_finite() {
                return Err(invalid());
            }
            if event.kind == INPUT_POINTER_MOVE {
                Input::Move(event.x, event.y, event.displayed)
            } else {
                let code = match event.button {
                    BUTTON_PRIMARY => 1,
                    BUTTON_SECONDARY => 2,
                    BUTTON_MIDDLE => 4,
                    _ => return Err(invalid()),
                };
                Input::Button(code, event.pressed != 0, event.x, event.y, event.displayed)
            }
        }
        INPUT_WINDOW => {
            if event.window.focused > 1
                || event.window.suspended > 1
                || !event.window.scale_factor.is_finite()
                || event.window.scale_factor <= 0.0
            {
                return Err(invalid());
            }
            Input::Window(event.window)
        }
        _ => return Err(invalid()),
    })
}
pub(crate) fn apply(
    rt: &CoreRuntime,
    input: Input,
    info: SurfaceInfo,
    identity: FrameIdentity,
) -> NativeResult<()> {
    let position = |x: f64, y: f64, frame: FrameIdentity| -> NativeResult<(i32, i32)> {
        if frame.session_epoch != identity.session_epoch
            || frame.output_epoch != identity.output_epoch
            || frame.generation == 0
            || frame.generation > identity.generation
        {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_STALE_INPUT",
                "pointer frame is no longer valid",
            ));
        }
        if x < 0.0
            || y < 0.0
            || x >= f64::from(info.raster.width)
            || y >= f64::from(info.raster.height)
        {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_INPUT",
                "pointer lies outside the displayed frame",
            ));
        }
        Ok((
            (x * f64::from(info.logical.width) / f64::from(info.raster.width)) as i32,
            (y * f64::from(info.logical.height) / f64::from(info.raster.height)) as i32,
        ))
    };
    match input {
        Input::Key(key, down) => {
            if down {
                rt.feed_key_down(key)
            } else {
                rt.feed_key_up(key)
            }
        }
        Input::Move(x, y, frame) => {
            let (x, y) = position(x, y, frame)?;
            rt.feed_mouse(x, y);
        }
        Input::Button(code, down, x, y, frame) => {
            let (x, y) = position(x, y, frame)?;
            rt.feed_mouse(x, y);
            rt.feed_mouse_button(code, down);
            if code == 1 && down {
                rt.feed_click();
            }
        }
        Input::Window(_) => unreachable!("window state is handled by the native scheduler"),
    }
    Ok(())
}
fn key(name: &str) -> Option<u32> {
    Some(match name {
        "Escape" => 0x1b,
        "Enter" | "NumpadEnter" => 0x0d,
        "Tab" => 9,
        "Backspace" => 8,
        "Space" => 0x20,
        "Insert" => 0x2d,
        "Delete" => 0x2e,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        "ControlLeft" | "ControlRight" => 0x11,
        "ShiftLeft" | "ShiftRight" => 0x10,
        "AltLeft" | "AltRight" => 0x12,
        "NumpadAdd" => 0x6b,
        "NumpadSubtract" => 0x6d,
        "NumpadMultiply" => 0x6a,
        "NumpadDivide" => 0x6f,
        "Minus" => 0xbd,
        "Equal" => 0xbb,
        "Comma" => 0xbc,
        "Period" => 0xbe,
        "Slash" => 0xbf,
        "Semicolon" => 0xba,
        "Quote" => 0xde,
        "BracketLeft" => 0xdb,
        "BracketRight" => 0xdd,
        "Backslash" => 0xdc,
        "Backquote" => 0xc0,
        _ => {
            if let Some(letter) = name.strip_prefix("Key").filter(|s| s.len() == 1) {
                let c = letter.as_bytes()[0];
                if c.is_ascii_uppercase() {
                    return Some(u32::from(c));
                }
            }
            if let Some(digit) = name.strip_prefix("Digit").filter(|s| s.len() == 1) {
                let c = digit.as_bytes()[0];
                if c.is_ascii_digit() {
                    return Some(u32::from(c));
                }
            }
            if let Some(n) = name
                .strip_prefix("Numpad")
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| *n < 10)
            {
                return Some(0x60 + n);
            }
            if let Some(n) = name
                .strip_prefix('F')
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| (1..=24).contains(n))
            {
                return Some(0x6f + n);
            }
            return None;
        }
    })
}
