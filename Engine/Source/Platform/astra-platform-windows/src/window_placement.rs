use astra_platform::{PlatformError, PlatformErrorCode};
use windows::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub(crate) fn fit_to_work_area(window: &Window) -> Result<(), PlatformError> {
    let fail = || {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            "window.create",
            "monitor work area is unavailable",
        )
    };
    let handle = window.window_handle().map_err(|_| fail())?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(fail());
    };
    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // The handle belongs to this live winit window; MONITORINFO has the required size.
    unsafe {
        let native = MonitorFromWindow(HWND(handle.hwnd.get() as *mut _), MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(native, &mut monitor)
            .ok()
            .map_err(|_| fail())?;
    }
    let work = monitor.rcWork;
    let inner = window.inner_size();
    let outer = window.outer_size();
    let (size, position) = placement([work.left, work.top, work.right, work.bottom], inner, outer);
    if size != inner {
        let _ = window.request_inner_size(size);
    }
    window.set_outer_position(position);
    Ok(())
}

fn placement(
    work: [i32; 4],
    inner: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
) -> (PhysicalSize<u32>, PhysicalPosition<i32>) {
    let width = (i64::from(work[2]) - i64::from(work[0])).max(1) as u32;
    let height = (i64::from(work[3]) - i64::from(work[1])).max(1) as u32;
    let outer_width = outer.width.min(width);
    let outer_height = outer.height.min(height);
    let size = PhysicalSize::new(
        outer_width
            .saturating_sub(outer.width.saturating_sub(inner.width))
            .max(1),
        outer_height
            .saturating_sub(outer.height.saturating_sub(inner.height))
            .max(1),
    );
    let position = PhysicalPosition::new(
        (i64::from(work[0]) + i64::from(width - outer_width) / 2) as i32,
        (i64::from(work[1]) + i64::from(height - outer_height) / 2) as i32,
    );
    (size, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_window_fits_taskbar_work_area_and_negative_monitor_coordinates() {
        let (size, position) = placement(
            [0, 0, 1280, 672],
            PhysicalSize::new(800, 600),
            PhysicalSize::new(816, 639),
        );
        assert_eq!(size, PhysicalSize::new(800, 600));
        assert_eq!(position, PhysicalPosition::new(232, 16));
        let (size, position) = placement(
            [-1280, 40, 0, 720],
            PhysicalSize::new(1920, 1080),
            PhysicalSize::new(1936, 1119),
        );
        assert_eq!(size, PhysicalSize::new(1264, 641));
        assert_eq!(position, PhysicalPosition::new(-1280, 40));
    }
}
