//! Publish a private profile atomically without replacing any existing file.
use std::{io::Write, path::Path};

pub fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().ok_or("ASTRA_EMU_GARBRO_OUTPUT_PARENT")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    // NamedTempFile creates mode 0600 on Unix. On Windows protect the empty
    // temporary before any private bytes are written.
    #[cfg(windows)]
    restrict_windows_path(temporary.path())?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map_err(|_| "ASTRA_EMU_GARBRO_OUTPUT_PUBLISH")?;
    Ok(())
}

#[cfg(windows)]
fn restrict_windows_path(path: &Path) -> std::io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_preserves_existing_profile_and_cleans_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("musica.profile.json");
        write_new_private(&output, b"first").unwrap();
        assert!(write_new_private(&output, b"second").is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"first");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(output).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
