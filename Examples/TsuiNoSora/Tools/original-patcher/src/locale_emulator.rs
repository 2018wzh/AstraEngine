use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{PatchError, PatchResult},
    filesystem::{hash_file, join_relative},
};

pub const DIRECTORY: &str = "LocaleEmulator-2.5.0.1";
const VERSION: &str = "2.5.0.1";
const REVISION: &str = "db03abf6914beeca09ee975120ff5ce2091c8dca";
const FILES: &[(&str, &str)] = &[
    (
        "LoaderDll.dll",
        "82fae0f44f4ca0c9c37907df74cef2415eeb5fae1cf8d4f36f34ffcaf7e3cc0c",
    ),
    (
        "LocaleEmulator.dll",
        "c79c175fdad174aa46a72197d148316299a56f950aaab1b84930d09ee1084a88",
    ),
    (
        "COPYING",
        "8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903",
    ),
    (
        "COPYING.LESSER",
        "da7eabb7bafdf7d3ae5e9f223aa5bdc1eece45ac569dc21b3b037520b4464768",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocaleEmulatorRecord {
    pub id: String,
    pub version: String,
    pub revision: String,
    pub license: String,
    pub ansi_code_page: u32,
    pub locale_id: u32,
    pub charset: u32,
    pub timezone: String,
}

pub fn record() -> LocaleEmulatorRecord {
    LocaleEmulatorRecord {
        id: "Locale-Emulator-Core".to_owned(),
        version: VERSION.to_owned(),
        revision: REVISION.to_owned(),
        license: "LGPL-3.0".to_owned(),
        ansi_code_page: 932,
        locale_id: 0x0411,
        charset: 128,
        timezone: "Tokyo Standard Time".to_owned(),
    }
}

pub fn install(explicit: Option<&Path>, output: &Path) -> PatchResult<()> {
    let source = resolve_source(explicit)?;
    validate_directory(&source)?;
    let destination = join_relative(output, DIRECTORY)?;
    fs::create_dir(&destination).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LOCALE_DIRECTORY_CREATE_FAILED",
            "create locale emulator directory",
            error,
        )
    })?;
    for (name, _) in FILES {
        let target = if name.ends_with(".dll") {
            output.join(name)
        } else {
            destination.join(name)
        };
        fs::copy(source.join(name), target).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_LOCALE_COPY_FAILED",
                "copy locale emulator file",
                error,
            )
        })?;
    }
    Ok(())
}

pub fn verify_installed(root: &Path) -> PatchResult<()> {
    let licenses = join_relative(root, DIRECTORY)?;
    for (name, expected) in FILES {
        let path = if name.ends_with(".dll") {
            root.join(name)
        } else {
            licenses.join(name)
        };
        validate_file(&path, expected)?;
    }
    Ok(())
}

fn resolve_source(explicit: Option<&Path>) -> PatchResult<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    let executable = std::env::current_exe().map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_EXECUTABLE_PATH_UNAVAILABLE",
            "resolve patcher executable",
            error,
        )
    })?;
    let parent = executable.parent().ok_or_else(|| {
        PatchError::validation(
            "TSUI_PATCH_LOCALE_LOCATION_INVALID",
            "patcher executable has no parent directory",
        )
    })?;
    Ok(parent.join(DIRECTORY))
}

fn validate_directory(directory: &Path) -> PatchResult<()> {
    if !directory.is_dir() {
        return Err(PatchError::helper(
            "TSUI_PATCH_LOCALE_MISSING",
            "bundled Locale Emulator directory is missing",
        ));
    }
    for (name, expected) in FILES {
        validate_file(&directory.join(name), expected)?;
    }
    Ok(())
}

fn validate_file(path: &Path, expected: &str) -> PatchResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LOCALE_FILE_MISSING",
            "inspect Locale Emulator file",
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hash_file(path)? != expected {
        return Err(PatchError::helper(
            "TSUI_PATCH_LOCALE_HASH_MISMATCH",
            "bundled Locale Emulator file hash is not approved",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_locale_contract_is_stable() {
        let identity = record();
        assert_eq!(identity.version, "2.5.0.1");
        assert_eq!(identity.revision, REVISION);
        assert_eq!(identity.ansi_code_page, 932);
        assert_eq!(identity.locale_id, 0x0411);
        assert_eq!(identity.charset, 128);
        assert_eq!(identity.timezone, "Tokyo Standard Time");
    }

    #[test]
    fn altered_helper_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        for (name, _) in FILES {
            fs::write(directory.path().join(name), b"altered").expect("write fixture");
        }
        let error = validate_directory(directory.path()).expect_err("hash mismatch must fail");
        assert_eq!(error.code, "TSUI_PATCH_LOCALE_HASH_MISMATCH");
    }
}

#[cfg(windows)]
pub mod windows {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path};

    use libloading::{Library, Symbol};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{
            GetExitCodeProcess, TerminateProcess, WaitForSingleObject, PROCESS_INFORMATION,
            STARTUPINFOW,
        },
    };

    use crate::{
        error::{PatchError, PatchResult},
        filesystem::join_relative,
    };

    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;

    #[repr(C)]
    struct TimeFields {
        year: u16,
        month: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
        weekday: u16,
    }
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: TimeFields,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: TimeFields,
        daylight_bias: i32,
    }
    #[repr(C)]
    struct LocaleEnvironment {
        ansi_code_page: u32,
        oem_code_page: u32,
        locale_id: u32,
        default_charset: u32,
        hook_ui_language_api: u32,
        default_face_name: [u8; 64],
        timezone: TimeZoneInformation,
    }
    #[repr(C)]
    struct LocaleEnvironmentBlock {
        environment: LocaleEnvironment,
        registry_redirect_count: u64,
    }

    type LeCreateProcess = unsafe extern "system" fn(
        *const core::ffi::c_void,
        *const u16,
        *mut u16,
        *const u16,
        u32,
        *mut STARTUPINFOW,
        *mut PROCESS_INFORMATION,
        *const core::ffi::c_void,
        *const core::ffi::c_void,
        *const core::ffi::c_void,
        HANDLE,
    ) -> u32;

    pub struct LocaleProcess {
        pub process_id: u32,
        handle: HANDLE,
        _library: Library,
    }

    impl LocaleProcess {
        pub fn poll_exit(&self) -> PatchResult<Option<u32>> {
            let wait = unsafe { WaitForSingleObject(self.handle, 0) };
            if wait == WAIT_TIMEOUT {
                return Ok(None);
            }
            if wait != WAIT_OBJECT_0 {
                return Err(PatchError::validation(
                    "TSUI_PATCH_LOCALE_WAIT_FAILED",
                    "failed to wait for locale-emulated game",
                ));
            }
            let mut code = 0;
            if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
                return Err(PatchError::validation(
                    "TSUI_PATCH_LOCALE_EXIT_CODE_FAILED",
                    "failed to read locale-emulated game exit code",
                ));
            }
            Ok(Some(code))
        }
        pub fn terminate(&self) {
            unsafe {
                TerminateProcess(self.handle, 2);
            }
        }
    }
    impl Drop for LocaleProcess {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }

    pub fn launch(root: &Path, executable_name: &str) -> PatchResult<LocaleProcess> {
        let loader_path = root.join("LoaderDll.dll");
        let library = unsafe { Library::new(loader_path) }.map_err(|error| {
            PatchError::helper(
                "TSUI_PATCH_LOCALE_LOAD_FAILED",
                format!("load approved Locale Emulator core: {error}"),
            )
        })?;
        let create: Symbol<LeCreateProcess> = unsafe { library.get(b"LeCreateProcess\0") }
            .map_err(|_| {
                PatchError::helper(
                    "TSUI_PATCH_LOCALE_SYMBOL_MISSING",
                    "approved Locale Emulator core is missing LeCreateProcess",
                )
            })?;
        let executable = join_relative(root, executable_name)?;
        let application = wide(executable.as_os_str());
        let mut command_line = application.clone();
        let current_directory = wide(root.as_os_str());
        let environment = japanese_environment_block();
        let mut startup = unsafe { std::mem::zeroed::<STARTUPINFOW>() };
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };
        let result = unsafe {
            create(
                (&environment as *const LocaleEnvironmentBlock).cast(),
                application.as_ptr(),
                command_line.as_mut_ptr(),
                current_directory.as_ptr(),
                0,
                &mut startup,
                &mut process,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if result != 0 {
            return Err(PatchError::helper(
                "TSUI_PATCH_LOCALE_PROCESS_FAILED",
                format!("Locale Emulator core returned status {result}"),
            ));
        }
        unsafe {
            CloseHandle(process.hThread);
        }
        Ok(LocaleProcess {
            process_id: process.dwProcessId,
            handle: process.hProcess,
            _library: library,
        })
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }
    fn name(value: &str) -> [u16; 32] {
        let mut out = [0; 32];
        for (slot, unit) in out.iter_mut().zip(value.encode_utf16()) {
            *slot = unit;
        }
        out
    }
    fn japanese_environment() -> LocaleEnvironment {
        let zero = || TimeFields {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
            weekday: 0,
        };
        LocaleEnvironment {
            ansi_code_page: 932,
            oem_code_page: 932,
            locale_id: 0x0411,
            default_charset: 128,
            hook_ui_language_api: 0,
            default_face_name: [0; 64],
            timezone: TimeZoneInformation {
                bias: -540,
                standard_name: name("Tokyo Standard Time"),
                standard_date: zero(),
                standard_bias: 0,
                daylight_name: name("Tokyo Standard Time"),
                daylight_date: zero(),
                daylight_bias: 0,
            },
        }
    }

    fn japanese_environment_block() -> LocaleEnvironmentBlock {
        // LoaderDll expects the LEB to be followed by the 64-bit registry redirect count.
        // Zero declares that no registry values are redirected by this launcher.
        LocaleEnvironmentBlock {
            environment: japanese_environment(),
            registry_redirect_count: 0,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn environment_block_includes_empty_registry_table() {
            let block = japanese_environment_block();
            assert_eq!(
                std::mem::size_of_val(&block),
                std::mem::size_of::<LocaleEnvironment>() + std::mem::size_of::<u64>()
            );
            assert_eq!(block.registry_redirect_count, 0);
        }
    }
}
