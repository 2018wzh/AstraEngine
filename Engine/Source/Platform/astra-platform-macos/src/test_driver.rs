use std::ffi::c_void;

use astra_platform::{PlatformError, PlatformErrorCode};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

type CGEventRef = *mut c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CGEventRef;
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        event_type: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGPreflightScreenCaptureAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}

pub struct MacosTestDriver;

impl MacosTestDriver {
    pub fn open() -> Result<Self, PlatformError> {
        if !unsafe { AXIsProcessTrusted() } {
            return Err(driver_error(
                "test_driver.cgevent.open",
                "Accessibility permission is required for CGEvent automation",
            ));
        }
        if !unsafe { CGPreflightScreenCaptureAccess() } {
            return Err(driver_error(
                "test_driver.screen_capture.open",
                "Screen Recording permission is required for capture evidence",
            ));
        }
        Ok(Self)
    }

    pub fn send_key(&self, virtual_key: u16) -> Result<(), PlatformError> {
        for down in [true, false] {
            let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), virtual_key, down) };
            post(event, "test_driver.cgevent.key")?;
        }
        Ok(())
    }

    pub fn move_mouse(&self, x: f64, y: f64) -> Result<(), PlatformError> {
        let event = unsafe { CGEventCreateMouseEvent(std::ptr::null(), 5, CGPoint { x, y }, 0) };
        post(event, "test_driver.cgevent.pointer")
    }

    pub fn click_primary(&self, x: f64, y: f64) -> Result<(), PlatformError> {
        for event_type in [1, 2] {
            let event = unsafe {
                CGEventCreateMouseEvent(std::ptr::null(), event_type, CGPoint { x, y }, 0)
            };
            post(event, "test_driver.cgevent.pointer")?;
        }
        Ok(())
    }
}

fn post(event: CGEventRef, operation: &'static str) -> Result<(), PlatformError> {
    if event.is_null() {
        return Err(driver_error(operation, "CGEvent creation failed"));
    }
    unsafe {
        CGEventPost(0, event);
        CFRelease(event);
    }
    Ok(())
}

fn driver_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}
