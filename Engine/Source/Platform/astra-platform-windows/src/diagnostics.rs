pub fn diagnostics_root(app_id: &str) -> Result<std::path::PathBuf, String> {
    if app_id.is_empty()
        || !app_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err("diagnostics app id must be a safe symbol".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        use windows::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
        };
        let root = unsafe {
            let path = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)
                .map_err(|error| format!("diagnostics known folder lookup failed: {error}"))?;
            let root = path
                .to_string()
                .map_err(|error| format!("diagnostics known folder conversion failed: {error}"))?;
            CoTaskMemFree(Some(path.as_ptr() as *const c_void));
            root
        };
        Ok(std::path::PathBuf::from(root)
            .join("AstraEngine")
            .join(app_id))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Windows diagnostics root is unavailable on this platform".to_string())
    }
}
