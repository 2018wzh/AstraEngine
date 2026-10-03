use super::abi;
use crate::wire::{self, Failure, FrameInfo, Reply, Status, WindowCommand};

pub fn frame(frame: abi::FrameInfo) -> Result<FrameInfo, Failure> {
    let frame = FrameInfo {
        width: frame.width,
        height: frame.height,
        logical_width: frame.logical_width,
        logical_height: frame.logical_height,
        stride: frame.stride,
    };
    frame.required_bytes().map_err(|_| Failure::Frame)?;
    Ok(frame)
}

unsafe fn borrowed<'a>(
    pointer: *const u8,
    length: u64,
    maximum: usize,
) -> Result<&'a [u8], Failure> {
    let length = usize::try_from(length).map_err(|_| Failure::Protocol)?;
    if length > maximum || (length > 0 && pointer.is_null()) {
        return Err(Failure::Protocol);
    }
    if length == 0 {
        return Ok(&[]);
    }
    // NativeOwner.h guarantees readable storage through the next advance.
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
}

pub unsafe fn advance(value: &abi::AdvanceResult) -> Result<Reply, Failure> {
    let status = match value.state {
        0 => Status::Running,
        1 => Status::Waiting,
        2 if value.exit_code == 0 => Status::Finished,
        2 => return Err(Failure::Advance),
        _ => return Err(Failure::Protocol),
    };
    if value.reset_clock > 1 {
        return Err(Failure::Protocol);
    }
    let command = &value.window;
    let window = match command.kind {
        0 => None,
        1 if command.fullscreen <= 1 => Some(WindowCommand::SetFullscreen(command.fullscreen == 1)),
        2 => {
            let bytes = unsafe { borrowed(command.data, command.data_bytes, 4096)? };
            let title = std::str::from_utf8(bytes).map_err(|_| Failure::Protocol)?;
            if title.chars().any(char::is_control) {
                return Err(Failure::Protocol);
            }
            Some(WindowCommand::SetTitle(
                wire::Text::new(title.to_owned()).map_err(|_| Failure::Protocol)?,
            ))
        }
        3 if command.icon_width > 0
            && command.icon_height > 0
            && command.icon_width <= 256
            && command.icon_height <= 256 =>
        {
            let size = u64::from(command.icon_width) * u64::from(command.icon_height) * 4;
            if command.data_bytes != size {
                return Err(Failure::Protocol);
            }
            let bytes = unsafe { borrowed(command.data, size, 262144)? };
            Some(WindowCommand::SetIcon {
                width: command.icon_width,
                height: command.icon_height,
                rgba: wire::Items::new(bytes.to_vec()).map_err(|_| Failure::Protocol)?,
            })
        }
        _ => return Err(Failure::Protocol),
    };
    Ok(Reply::Advanced {
        status,
        window,
        reset_clock: value.reset_clock == 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_frame_preserves_different_raster_and_stage_aspects() {
        let native = abi::FrameInfo {
            width: 1920,
            height: 1080,
            logical_width: 1024,
            logical_height: 768,
            stride: 7680,
        };
        let frame = frame(native).unwrap();
        assert_eq!((frame.logical_width, frame.logical_height), (1024, 768));
        assert_eq!(frame.required_bytes().unwrap(), 8294400);
        assert!(super::frame(abi::FrameInfo::default()).is_err());
    }
    #[test]
    fn borrowed_outputs_copy_and_reject_invalid_lengths_before_read() {
        let title = "游戏窗口".as_bytes().to_vec();
        let mut output = abi::AdvanceResult {
            state: 1,
            reset_clock: 1,
            window: abi::WindowCommand {
                kind: 2,
                data: title.as_ptr(),
                data_bytes: title.len() as u64,
                ..Default::default()
            },
            ..Default::default()
        };
        let reply = unsafe { advance(&output) }.unwrap();
        drop(title);
        assert!(matches!(
            reply,
            Reply::Advanced {
                status: Status::Waiting,
                window: Some(WindowCommand::SetTitle(_)),
                reset_clock: true
            }
        ));
        output.window.data = std::ptr::null();
        assert!(unsafe { advance(&output) }.is_err());
        output.window.data_bytes = u64::MAX;
        assert!(unsafe { advance(&output) }.is_err());
        output.window.kind = 99;
        assert!(unsafe { advance(&output) }.is_err());
        output.window.kind = 0;
        output.state = 2;
        output.exit_code = 1;
        assert_eq!(unsafe { advance(&output) }, Err(Failure::Advance));
    }
}
