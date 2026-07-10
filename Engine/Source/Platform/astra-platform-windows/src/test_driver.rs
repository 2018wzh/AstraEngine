use std::{
    ffi::c_void,
    thread,
    time::{Duration, Instant},
};

use astra_platform::{PlatformError, PlatformErrorCode};
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
            GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
            DIB_RGB_COLORS, SRCCOPY,
        },
        UI::{
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                KEYEVENTF_KEYUP, VIRTUAL_KEY,
            },
            WindowsAndMessaging::{
                EnumWindows, GetClientRect, GetForegroundWindow, GetWindowTextLengthW,
                GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
                ShowWindow, SW_RESTORE,
            },
        },
    },
};

pub struct WindowsTestDriver;

impl WindowsTestDriver {
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
            if let Some(window) = find_window(process_id, None) {
                return Ok(TestWindow { window });
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
        send_keyboard(virtual_key, KEYBD_EVENT_FLAGS::default())?;
        send_keyboard(virtual_key, KEYEVENTF_KEYUP)
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

fn driver_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}
