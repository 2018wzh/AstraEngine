use std::path::Path;

/// Restrict a private directory to its owner before it is used for staging or
/// writable game state. Permission failures are returned to the caller so the
/// operation can fail closed.
pub fn enforce_private_directory_permissions(path: &Path) -> std::io::Result<()> {
    restrict_path(path, true)
}

/// Restrict a private file to its owner before it is committed or read back.
pub fn enforce_private_file_permissions(path: &Path) -> std::io::Result<()> {
    restrict_path(path, false)
}

#[cfg(unix)]
fn restrict_path(path: &Path, directory: bool) -> std::io::Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let mode = if directory { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(windows)]
fn restrict_path(path: &Path, _directory: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                },
                SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR,
            },
        },
    };

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)".encode_utf16().chain(Some(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        let result = SetFileSecurityW(
            PCWSTR(path.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
        .ok()
        .map_err(|error| std::io::Error::other(error.to_string()));
        let released = LocalFree(Some(HLOCAL(descriptor.0)));
        if !released.is_invalid() {
            return Err(std::io::Error::other("security descriptor release failed"));
        }
        result
    }
}

#[cfg(not(any(unix, windows)))]
fn restrict_path(_path: &Path, _directory: bool) -> std::io::Result<()> {
    Ok(())
}
