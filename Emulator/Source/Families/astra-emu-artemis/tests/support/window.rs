use astra_emu_abi::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowLongPtrW, IsIconic, IsWindow, GWL_STYLE,
        WS_OVERLAPPEDWINDOW,
    },
};

static QUERIES: AtomicUsize = AtomicUsize::new(0);

/// Owns a real hidden, decorated Win32 window on the test thread.
pub(super) struct Window(HWND);

impl Window {
    pub(super) fn new() -> Self {
        const CLASS: [u16; 7] = [83, 84, 65, 84, 73, 67, 0]; // STATIC
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS.as_ptr(),
                CLASS.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                64,
                48,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "{}", std::io::Error::last_os_error());
        QUERIES.store(0, Ordering::Release);
        Self(hwnd)
    }

    pub(super) fn queries(&self) -> usize {
        QUERIES.load(Ordering::Acquire)
    }

    pub(super) fn service(
        &self,
        invalidate: unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64),
    ) -> WindowService {
        WindowService {
            context: self.0,
            request: Some(query),
            invalidate: Some(invalidate),
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        assert_ne!(unsafe { DestroyWindow(self.0) }, 0);
    }
}

unsafe extern "C" fn query(
    context: *mut std::ffi::c_void,
    request: *const WindowRequest,
    result: *mut WindowResult,
    error: *mut Error,
) -> Status {
    if request.is_null() || result.is_null() {
        return INVALID_ARGUMENT;
    }
    let request = unsafe { &*request };
    if request.kind != WINDOW_QUERY_STATE || request.flags != 0 {
        return UNSUPPORTED;
    }
    let style = unsafe { GetWindowLongPtrW(context, GWL_STYLE) };
    if unsafe { IsWindow(context) } == 0
        || style & WS_OVERLAPPEDWINDOW as isize != WS_OVERLAPPEDWINDOW as isize
    {
        if !error.is_null() {
            unsafe { &mut *error }.set(
                "ARTEMIS_TEST_WINDOW_STATE",
                "query requires the actual decorated test window",
            );
        }
        return CORE_ERROR;
    }
    let result = unsafe { &mut *result };
    result.valid_fields = WINDOW_RESULT_MODE | WINDOW_RESULT_MINIMIZED;
    // This host never changes display mode; the queried decorated HWND is windowed.
    result.fullscreen = FULLSCREEN_WINDOWED;
    result.minimized = u32::from(unsafe { IsIconic(context) } != 0);
    QUERIES.fetch_add(1, Ordering::Release);
    OK
}
