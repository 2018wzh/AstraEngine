use std::{
    ffi::c_void,
    thread,
    time::{Duration, Instant},
};

use astra_platform::{PlatformError, PlatformErrorCode};
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC,
            DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
            BI_RGB, DIB_RGB_COLORS, SRCCOPY,
        },
        UI::{
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
                KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
                MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
                VIRTUAL_KEY,
            },
            WindowsAndMessaging::{
                EnumWindows, GetClientRect, GetForegroundWindow, GetSystemMetrics,
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
                PostMessageW, SetForegroundWindow, ShowWindow, SM_CXVIRTUALSCREEN,
                SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, WM_CLOSE,
            },
        },
    },
};

pub struct WindowsTestDriver;

impl WindowsTestDriver {
    pub fn find_process_window(process_id: u32) -> Option<TestWindow> {
        find_window(process_id, None).map(|window| TestWindow { window })
    }

    pub fn wait_for_window(
        process_id: u32,
        title: &str,
        timeout: Duration,
    ) -> Result<TestWindow, PlatformError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(window) = find_window(process_id, Some(title)) {
                return Ok(TestWindow { window });
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(driver_error(
            "test_driver.window.wait",
            "test driver could not find the requested window",
        ))
    }

    pub fn wait_for_process_window(
        process_id: u32,
        timeout: Duration,
    ) -> Result<TestWindow, PlatformError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(window) = Self::find_process_window(process_id) {
                return Ok(window);
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(driver_error(
            "test_driver.window.wait",
            "test driver could not find a visible process window",
        ))
    }
}

#[derive(Debug, Clone)]
pub struct TestCapturedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

pub struct TestWindow {
    window: HWND,
}

impl TestWindow {
    pub fn focus(&self) -> Result<(), PlatformError> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let _ = send_keyboard(0x12, KEYBD_EVENT_FLAGS::default());
            let _ = send_keyboard(0x12, KEYEVENTF_KEYUP);
            unsafe {
                let _ = ShowWindow(self.window, SW_RESTORE);
                if GetForegroundWindow() == self.window
                    || SetForegroundWindow(self.window).as_bool()
                {
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(driver_error(
            "test_driver.window.focus",
            "test driver could not focus the requested window",
        ))
    }

    pub fn send_key(&self, virtual_key: u16) -> Result<(), PlatformError> {
        self.send_key_state(virtual_key, true)?;
        self.send_key_state(virtual_key, false)
    }

    /// Sends a real OS keyboard transition. Callers that replay an input sequence must preserve
    /// its pressed/released edges instead of synthesizing a click for every keyboard event.
    pub fn send_key_state(&self, virtual_key: u16, pressed: bool) -> Result<(), PlatformError> {
        send_keyboard(
            virtual_key,
            if pressed {
                KEYBD_EVENT_FLAGS::default()
            } else {
                KEYEVENTF_KEYUP
            },
        )
    }

    /// Moves the real OS cursor to a client-relative coordinate. The coordinate must be inside
    /// the visible client region so a test cannot silently target another window.
    pub fn move_pointer(&self, x: u32, y: u32) -> Result<(), PlatformError> {
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(self.window, &mut rect).map_err(|_| {
                driver_error(
                    "test_driver.pointer.client_rect",
                    "client rect query failed",
                )
            })?;
        }
        let width = u32::try_from((rect.right - rect.left).max(0)).map_err(|_| {
            driver_error("test_driver.pointer.client_rect", "client area is invalid")
        })?;
        let height = u32::try_from((rect.bottom - rect.top).max(0)).map_err(|_| {
            driver_error("test_driver.pointer.client_rect", "client area is invalid")
        })?;
        if width == 0 || height == 0 || x >= width || y >= height {
            return Err(driver_error(
                "test_driver.pointer.bounds",
                "pointer coordinate is outside the client area",
            ));
        }
        let mut point = POINT {
            x: x as i32,
            y: y as i32,
        };
        unsafe {
            if !ClientToScreen(self.window, &mut point).as_bool() {
                return Err(driver_error(
                    "test_driver.pointer.client_to_screen",
                    "client coordinate conversion failed",
                ));
            }
        }
        let (left, top, virtual_width, virtual_height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        if virtual_width <= 0 || virtual_height <= 0 {
            return Err(driver_error(
                "test_driver.pointer.desktop",
                "virtual desktop bounds are invalid",
            ));
        }
        let absolute_x = normalize_absolute_coordinate(point.x, left, virtual_width)?;
        let absolute_y = normalize_absolute_coordinate(point.y, top, virtual_height)?;
        send_mouse(
            absolute_x,
            absolute_y,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )
    }

    pub fn send_primary_button(&self, pressed: bool) -> Result<(), PlatformError> {
        send_mouse(
            0,
            0,
            0,
            if pressed {
                MOUSEEVENTF_LEFTDOWN
            } else {
                MOUSEEVENTF_LEFTUP
            },
        )
    }

    pub fn send_secondary_button(&self, pressed: bool) -> Result<(), PlatformError> {
        send_mouse(
            0,
            0,
            0,
            if pressed {
                MOUSEEVENTF_RIGHTDOWN
            } else {
                MOUSEEVENTF_RIGHTUP
            },
        )
    }

    pub fn send_wheel(&self, delta_y: i32) -> Result<(), PlatformError> {
        if delta_y == 0 {
            return Err(driver_error(
                "test_driver.pointer.wheel",
                "wheel delta must be non-zero",
            ));
        }
        send_mouse(0, 0, delta_y, MOUSEEVENTF_WHEEL)
    }

    /// Requests the application's normal close path after a physical input transcript completes.
    /// This is deliberately distinct from process termination so shutdown evidence remains valid.
    pub fn request_close(&self) -> Result<(), PlatformError> {
        unsafe { PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(
            |_| {
                driver_error(
                    "test_driver.window.close",
                    "test driver could not request window close",
                )
            },
        )?;
        Ok(())
    }

    pub fn capture_rgba(&self) -> Result<TestCapturedFrame, PlatformError> {
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(self.window, &mut rect)
                .map_err(|_| driver_error("test_driver.capture", "client rect query failed"))?;
            let width = (rect.right - rect.left).max(0);
            let height = (rect.bottom - rect.top).max(0);
            if width <= 0 || height <= 0 {
                return Err(driver_error("test_driver.capture", "client area is empty"));
            }
            let window_dc = GetDC(Some(self.window));
            if window_dc.0.is_null() {
                return Err(driver_error(
                    "test_driver.capture",
                    "window DC is unavailable",
                ));
            }
            let memory_dc = CreateCompatibleDC(Some(window_dc));
            let bitmap = CreateCompatibleBitmap(window_dc, width, height);
            let old_object = SelectObject(memory_dc, bitmap.into());
            let result = (|| {
                BitBlt(
                    memory_dc,
                    0,
                    0,
                    width,
                    height,
                    Some(window_dc),
                    0,
                    0,
                    SRCCOPY,
                )
                .map_err(|_| driver_error("test_driver.capture", "pixel copy failed"))?;
                let mut info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bgra = vec![0u8; width as usize * height as usize * 4];
                let lines = GetDIBits(
                    memory_dc,
                    bitmap,
                    0,
                    height as u32,
                    Some(bgra.as_mut_ptr().cast::<c_void>()),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                if lines == 0 {
                    return Err(driver_error("test_driver.capture", "pixel readback failed"));
                }
                let mut rgba8 = Vec::with_capacity(bgra.len());
                for pixel in bgra.chunks_exact(4) {
                    rgba8.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
                Ok(TestCapturedFrame {
                    width: width as u32,
                    height: height as u32,
                    rgba8,
                })
            })();
            if !old_object.0.is_null() {
                let _ = SelectObject(memory_dc, old_object);
            }
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(Some(self.window), window_dc);
            result
        }
    }
}

fn find_window(process_id: u32, expected_title: Option<&str>) -> Option<HWND> {
    struct Search<'a> {
        process_id: u32,
        expected_title: Option<&'a str>,
        result: HWND,
    }

    unsafe extern "system" fn callback(window: HWND, parameter: LPARAM) -> BOOL {
        let search = unsafe { &mut *(parameter.0 as *mut Search<'_>) };
        if !search.result.0.is_null() {
            return BOOL(0);
        }
        if !unsafe { IsWindowVisible(window) }.as_bool() {
            return BOOL(1);
        }
        let mut window_process_id = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut window_process_id)) };
        if window_process_id != search.process_id {
            return BOOL(1);
        }
        let length = unsafe { GetWindowTextLengthW(window) };
        let mut text = vec![0u16; length as usize + 1];
        let read = unsafe { GetWindowTextW(window, &mut text) };
        let title = String::from_utf16_lossy(&text[..read as usize]);
        if search
            .expected_title
            .is_none_or(|expected| title == expected)
        {
            search.result = window;
            BOOL(0)
        } else {
            BOOL(1)
        }
    }

    let mut search = Search {
        process_id,
        expected_title,
        result: HWND::default(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut search as *mut Search<'_> as isize),
        );
    }
    (!search.result.0.is_null()).then_some(search.result)
}

fn send_keyboard(virtual_key: u16, flags: KEYBD_EVENT_FLAGS) -> Result<(), PlatformError> {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err(driver_error(
            "test_driver.input.send",
            "test driver could not send keyboard input",
        ));
    }
    Ok(())
}

fn normalize_absolute_coordinate(
    value: i32,
    offset: i32,
    extent: i32,
) -> Result<i32, PlatformError> {
    let relative = value
        .checked_sub(offset)
        .ok_or_else(|| driver_error("test_driver.pointer.desktop", "coordinate overflow"))?;
    if relative < 0 || relative >= extent {
        return Err(driver_error(
            "test_driver.pointer.desktop",
            "client coordinate is outside the virtual desktop",
        ));
    }
    let scaled = i64::from(relative) * 65_535 / i64::from((extent - 1).max(1));
    i32::try_from(scaled).map_err(|_| {
        driver_error(
            "test_driver.pointer.desktop",
            "normalized coordinate overflow",
        )
    })
}

fn send_mouse(
    dx: i32,
    dy: i32,
    mouse_data: i32,
    flags: MOUSE_EVENT_FLAGS,
) -> Result<(), PlatformError> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err(driver_error(
            "test_driver.input.send",
            "test driver could not send mouse input",
        ));
    }
    Ok(())
}

fn driver_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

#[cfg(test)]
mod tests {
    use super::normalize_absolute_coordinate;

    #[test]
    fn absolute_pointer_coordinates_are_bounded_and_normalized() {
        assert_eq!(normalize_absolute_coordinate(100, 100, 200).unwrap(), 0);
        assert_eq!(
            normalize_absolute_coordinate(299, 100, 200).unwrap(),
            65_535
        );
        assert!(normalize_absolute_coordinate(99, 100, 200).is_err());
        assert!(normalize_absolute_coordinate(300, 100, 200).is_err());
    }
}
