use super::*;

pub(super) struct SystemRegistry;

#[cfg(not(windows))]
impl Registry for SystemRegistry {
    fn number(&self, _: &str, _: &str) -> Result<Option<u32>, CoreError> {
        Err(unavailable())
    }
    fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
        Err(unavailable())
    }
}

#[cfg(not(windows))]
fn unavailable() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_REGISTRY_UNAVAILABLE",
        "native registry settings require Windows",
    )
}

#[cfg(windows)]
impl Registry for SystemRegistry {
    fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError> {
        use windows::Win32::System::Registry::RRF_RT_REG_DWORD;
        read(key, value, RRF_RT_REG_DWORD)?
            .map(|bytes| {
                let bytes: [u8; 4] = bytes.try_into().map_err(|_| failure())?;
                Ok(u32::from_le_bytes(bytes))
            })
            .transpose()
    }
    fn string(&self, key: &str, value: &str) -> Result<Option<String>, CoreError> {
        use windows::Win32::System::Registry::RRF_RT_REG_SZ;
        read(key, value, RRF_RT_REG_SZ)?
            .map(|bytes| {
                if bytes.len() % 2 != 0 {
                    return Err(failure());
                }
                let mut words = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>();
                if words.last() == Some(&0) {
                    words.pop();
                }
                if words.contains(&0) {
                    return Err(failure());
                }
                String::from_utf16(&words).map_err(|_| failure())
            })
            .transpose()
    }
}

#[cfg(windows)]
fn read(
    key: &str,
    value: &str,
    kind: windows::Win32::System::Registry::REG_ROUTINE_FLAGS,
) -> Result<Option<Vec<u8>>, CoreError> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
            System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_SUBKEY_WOW6432KEY},
        },
    };
    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(key);
    if key.is_empty() || encoded.len() >= 64 - "Software\\".len() || invalid || key.contains('\0') {
        return Err(failure());
    }
    let key = format!("Software\\{}", key.trim_end_matches('\\'))
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let value = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut size = 0;
    let flags = kind | RRF_SUBKEY_WOW6432KEY;
    // Read only, explicitly using the original 32-bit process's registry view.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            flags,
            None,
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS || size > 512 {
        return Err(failure());
    }
    let mut bytes = vec![0u8; size as usize];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            flags,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS || size as usize > bytes.len() {
        return Err(failure());
    }
    bytes.truncate(size as usize);
    Ok(Some(bytes))
}
