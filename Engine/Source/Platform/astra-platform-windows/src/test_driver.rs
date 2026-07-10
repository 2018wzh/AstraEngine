use std::{
    thread,
    time::{Duration, Instant},
};

use astra_platform::{PlatformError, PlatformErrorCode};
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM},
        UI::{
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                KEYEVENTF_KEYUP, VIRTUAL_KEY,
            },
            WindowsAndMessaging::{
                EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
                IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
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
            if let Some(window) = find_window(process_id, title) {
                return Ok(TestWindow { window });
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(driver_error(
            "test_driver.window.wait",
            "test driver could not find the requested window",
        ))
    }
}

pub struct TestWindow {
    window: HWND,
}

impl TestWindow {
    pub fn focus(&self) -> Result<(), PlatformError> {
        unsafe {
            let _ = ShowWindow(self.window, SW_RESTORE);
            if !SetForegroundWindow(self.window).as_bool() {
                return Err(driver_error(
                    "test_driver.window.focus",
                    "test driver could not focus the requested window",
                ));
            }
        }
        Ok(())
    }

    pub fn send_key(&self, virtual_key: u16) -> Result<(), PlatformError> {
        send_keyboard(virtual_key, KEYBD_EVENT_FLAGS::default())?;
        send_keyboard(virtual_key, KEYEVENTF_KEYUP)
    }
}

fn find_window(process_id: u32, expected_title: &str) -> Option<HWND> {
    struct Search<'a> {
        process_id: u32,
        expected_title: &'a str,
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
        if title == search.expected_title {
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
