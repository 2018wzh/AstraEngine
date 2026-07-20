use std::{fs, path::Path};

use crate::{
    error::{PatchError, PatchResult},
    filesystem::{hash_file, join_relative},
    WINDOWED_PROJECTOR_NAME, WINDOW_LAUNCHER_NAME,
};

pub fn install(root: &Path) -> PatchResult<()> {
    let source = std::env::current_exe().map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LAUNCHER_SOURCE_UNAVAILABLE",
            "resolve patcher executable",
            error,
        )
    })?;
    verify_i686(&source)?;
    let target = join_relative(root, WINDOW_LAUNCHER_NAME)?;
    fs::copy(source, target).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LAUNCHER_INSTALL_FAILED",
            "install windowed game launcher",
            error,
        )
    })?;
    let original_projector = join_relative(root, "SETUP.exe")?;
    let windowed_projector = join_relative(root, WINDOWED_PROJECTOR_NAME)?;
    fs::copy(original_projector, windowed_projector).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_PROJECTOR_COPY_FAILED",
            "install windowed projector copy",
            error,
        )
    })?;
    Ok(())
}

pub fn verify_installed(root: &Path) -> PatchResult<()> {
    let launcher = join_relative(root, WINDOW_LAUNCHER_NAME)?;
    let metadata = fs::metadata(&launcher).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LAUNCHER_MISSING",
            "inspect windowed game launcher",
            error,
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || hash_file(&launcher)?.len() != 64 {
        return Err(PatchError::validation(
            "TSUI_PATCH_LAUNCHER_INVALID",
            "windowed game launcher is missing or invalid",
        ));
    }
    verify_i686(&launcher)?;
    let original = join_relative(root, "SETUP.exe")?;
    let projector = join_relative(root, WINDOWED_PROJECTOR_NAME)?;
    if hash_file(&original)? != hash_file(&projector)? {
        return Err(PatchError::validation(
            "TSUI_PATCH_PROJECTOR_COPY_MISMATCH",
            "windowed projector copy does not match SETUP.exe",
        ));
    }
    Ok(())
}

fn verify_i686(path: &Path) -> PatchResult<()> {
    let bytes = fs::read(path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LAUNCHER_READ_FAILED",
            "read windowed game launcher",
            error,
        )
    })?;
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return Err(PatchError::validation(
            "TSUI_PATCH_LAUNCHER_ARCHITECTURE_INVALID",
            "windowed game launcher must be a 32-bit PE executable",
        ));
    }
    let pe_offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().expect("fixed slice")) as usize;
    if pe_offset.checked_add(6).is_none_or(|end| end > bytes.len())
        || &bytes[pe_offset..pe_offset + 4] != b"PE\0\0"
        || u16::from_le_bytes(
            bytes[pe_offset + 4..pe_offset + 6]
                .try_into()
                .expect("fixed slice"),
        ) != 0x014c
    {
        return Err(PatchError::validation(
            "TSUI_PATCH_LAUNCHER_ARCHITECTURE_INVALID",
            "windowed game launcher must target i686-pc-windows-msvc",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub fn launch(game_root: &Path) -> PatchResult<()> {
    windows::launch(game_root)
}

#[cfg(not(windows))]
pub fn launch(_game_root: &Path) -> PatchResult<()> {
    Err(PatchError::validation(
        "TSUI_PATCH_LAUNCHER_PLATFORM_UNSUPPORTED",
        "the patched 1999 game launcher requires Windows",
    ))
}

#[cfg(windows)]
mod windows {
    use std::{
        collections::BTreeSet,
        mem::size_of,
        path::Path,
        thread,
        time::{Duration, Instant},
    };

    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClientRect, GetWindowLongPtrW, GetWindowThreadProcessId,
            IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SET_WINDOW_POS_FLAGS,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, WS_BORDER, WS_CAPTION, WS_MAXIMIZEBOX,
            WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
        },
    };

    use crate::error::{PatchError, PatchResult};

    const WINDOW_TIMEOUT: Duration = Duration::from_secs(20);
    const POLL_INTERVAL: Duration = Duration::from_millis(50);
    const STAGE_WIDTH: i32 = 800;
    const STAGE_HEIGHT: i32 = 600;

    pub fn launch(game_root: &Path) -> PatchResult<()> {
        let root = crate::filesystem::canonical_existing_directory(game_root, "patched game")?;
        crate::verify(&root)?;
        let process =
            crate::locale_emulator::windows::launch(&root, crate::WINDOWED_PROJECTOR_NAME)?;
        supervise(&process)
    }

    fn supervise(process: &crate::locale_emulator::windows::LocaleProcess) -> PatchResult<()> {
        let deadline = Instant::now() + WINDOW_TIMEOUT;
        let mut framed = BTreeSet::new();
        loop {
            if let Some(status) = process.poll_exit()? {
                return if status == 0 {
                    Ok(())
                } else {
                    Err(PatchError::validation(
                        "TSUI_PATCH_GAME_EXIT_FAILED",
                        "patched game exited with an error",
                    ))
                };
            }
            for window in find_stage_windows(process.process_id) {
                apply_frame(window)?;
                framed.insert(window as isize);
            }
            if framed.is_empty() && Instant::now() >= deadline {
                process.terminate();
                return Err(PatchError::validation(
                    "TSUI_PATCH_GAME_WINDOW_TIMEOUT",
                    "patched game did not create the verified 800x600 stage window",
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    struct WindowSearch {
        process_id: u32,
        windows: Vec<HWND>,
    }

    unsafe extern "system" fn collect_window(hwnd: HWND, parameter: LPARAM) -> i32 {
        let search = &mut *(parameter as *mut WindowSearch);
        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        let mut client = RECT::default();
        if process_id == search.process_id
            && IsWindowVisible(hwnd) != 0
            && GetClientRect(hwnd, &mut client) != 0
            && client.right - client.left == STAGE_WIDTH
            && client.bottom - client.top == STAGE_HEIGHT
        {
            search.windows.push(hwnd);
        }
        1
    }

    fn find_stage_windows(process_id: u32) -> Vec<HWND> {
        let mut search = WindowSearch {
            process_id,
            windows: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(collect_window),
                &mut search as *mut WindowSearch as LPARAM,
            );
        }
        search.windows
    }

    fn apply_frame(hwnd: HWND) -> PatchResult<()> {
        unsafe {
            let old_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let style = (old_style | WS_POPUP)
                & !(WS_BORDER
                    | WS_CAPTION
                    | WS_SYSMENU
                    | WS_MINIMIZEBOX
                    | WS_THICKFRAME
                    | WS_MAXIMIZEBOX);
            if SetWindowLongPtrW(hwnd, GWL_STYLE, style as _) == 0 && old_style != 0 {
                return Err(PatchError::validation(
                    "TSUI_PATCH_WINDOW_STYLE_FAILED",
                    "failed to apply the verified borderless window frame",
                ));
            }
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            };
            if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
                return Err(PatchError::validation(
                    "TSUI_PATCH_WINDOW_MONITOR_FAILED",
                    "failed to resolve the game window monitor",
                ));
            }
            let width = STAGE_WIDTH;
            let height = STAGE_HEIGHT;
            let x = info.rcWork.left + (info.rcWork.right - info.rcWork.left - width) / 2;
            let y = info.rcWork.top + (info.rcWork.bottom - info.rcWork.top - height) / 2;
            let flags: SET_WINDOW_POS_FLAGS = SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER;
            if SetWindowPos(hwnd, std::ptr::null_mut(), x, y, width, height, flags) == 0 {
                return Err(PatchError::validation(
                    "TSUI_PATCH_WINDOW_POSITION_FAILED",
                    "failed to center the fixed game window",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_launcher_name_is_stable() {
        assert_eq!(WINDOW_LAUNCHER_NAME, "TsuiNoSoraWindowed.exe");
        assert_eq!(WINDOWED_PROJECTOR_NAME, "TsuiNoSoraProjector.exe");
    }

    #[test]
    fn non_pe_launcher_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let executable = directory.path().join("launcher.exe");
        fs::write(&executable, b"not a PE image").expect("write fixture");
        let error = verify_i686(&executable).expect_err("non-PE input must fail");
        assert_eq!(error.code, "TSUI_PATCH_LAUNCHER_ARCHITECTURE_INVALID");
    }
}
