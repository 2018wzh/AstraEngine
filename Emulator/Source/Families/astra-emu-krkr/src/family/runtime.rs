use super::error;
use astra_emu_family_api::FamilyResult;
use std::path::{Path, PathBuf};

pub(super) fn game_directory(path: &Path) -> FamilyResult<String> {
    let root = path
        .canonicalize()
        .map_err(|_| error("ASTRA_EMU_KRKR_GAME_PATH", "game directory is unavailable"))?;
    if !root.is_dir() {
        return Err(error(
            "ASTRA_EMU_KRKR_GAME_PATH",
            "KrKr requires a native game directory",
        ));
    }
    let path = root
        .to_str()
        .filter(|path| path.len() <= 4096 && !path.contains('\0'))
        .ok_or_else(|| {
            error(
                "ASTRA_EMU_KRKR_GAME_PATH",
                "game directory exceeds the native UTF-8 bound",
            )
        })?;
    Ok(path.to_owned())
}

#[cfg(windows)]
pub(super) fn child_executable() -> FamilyResult<PathBuf> {
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    let mut module = std::ptr::null_mut();
    // The anchor is in the Family DLL, unlike current_exe (the Manager).
    // UNCHANGED_REFCOUNT is valid because Family libraries remain resident.
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            super::construct_module as *const () as *const u16,
            &mut module,
        )
    };
    if found == 0 {
        return Err(error(
            "ASTRA_EMU_KRKR_RUNTIME",
            "KrKr runtime module is unavailable",
        ));
    }
    let mut buffer = vec![0_u16; 32768];
    let len =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(error(
            "ASTRA_EMU_KRKR_RUNTIME",
            "KrKr runtime module path is unavailable",
        ));
    }
    use std::os::windows::ffi::OsStringExt;
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len]));
    let root = path.parent().ok_or_else(|| {
        error(
            "ASTRA_EMU_KRKR_RUNTIME",
            "KrKr runtime module directory is unavailable",
        )
    })?;
    astra_emu_sdk::resolve_game_file(root, "astra-emu-krkr-child.exe").map_err(|_| {
        error(
            "ASTRA_EMU_KRKR_RUNTIME",
            "KrKr native child is unavailable beside the Family module",
        )
    })
}

#[cfg(not(windows))]
pub(super) fn child_executable() -> FamilyResult<PathBuf> {
    Err(error(
        "ASTRA_EMU_KRKR_NATIVE_UNAVAILABLE",
        "KrKr native owner currently requires Windows",
    ))
}
