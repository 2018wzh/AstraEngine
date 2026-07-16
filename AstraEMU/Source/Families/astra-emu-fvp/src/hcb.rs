use std::collections::BTreeSet;

use astra_core::Hash256;
use encoding_rs::{Encoding, GBK, SHIFT_JIS, UTF_8};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_SYSCALLS: usize = 4096;
const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FvpNls {
    ShiftJis,
    Gbk,
    Utf8,
}

impl FvpNls {
    fn encoding(self) -> &'static Encoding {
        match self {
            Self::ShiftJis => SHIFT_JIS,
            Self::Gbk => GBK,
            Self::Utf8 => UTF_8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FvpSyscallDescriptor {
    pub id: u16,
    pub args: u8,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FvpHcbHeader {
    pub content_hash: Hash256,
    pub sys_desc_offset: u32,
    pub entry_point: u32,
    pub non_volatile_global_count: u16,
    pub volatile_global_count: u16,
    pub game_mode: u8,
    pub width: u32,
    pub height: u32,
    pub title_hash: Hash256,
    pub title_byte_len: u32,
    pub syscalls: Vec<FvpSyscallDescriptor>,
    pub custom_syscall_count: u16,
}

#[derive(Debug, Clone)]
pub struct FvpHcbScript {
    pub header: FvpHcbHeader,
    bytes: Vec<u8>,
    nls: FvpNls,
}

impl FvpHcbScript {
    pub fn parse(bytes: Vec<u8>, nls: FvpNls) -> Result<Self, FvpFormatError> {
        if bytes.len() < 4 {
            return Err(FvpFormatError::new(
                "FVP_HCB_HEADER",
                "HCB is shorter than its offset header",
            ));
        }
        let sys_desc_offset = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let offset = sys_desc_offset as usize;
        if offset < 4 || offset >= bytes.len() {
            return Err(FvpFormatError::new(
                "FVP_HCB_DESCRIPTOR_OFFSET",
                "system descriptor offset is outside the file",
            ));
        }
        let mut cursor = Cursor::new(&bytes, offset);
        let entry_point = cursor.u32()?;
        if entry_point < 4 || entry_point >= sys_desc_offset {
            return Err(FvpFormatError::new(
                "FVP_HCB_ENTRY_POINT",
                "entry point is outside the code area",
            ));
        }
        let non_volatile_global_count = cursor.u16()?;
        let volatile_global_count = cursor.u16()?;
        let game_mode = cursor.u8()?;
        let _reserved = cursor.u8()?;
        let (width, height) = game_mode_size(game_mode)
            .ok_or_else(|| FvpFormatError::new("FVP_HCB_GAME_MODE", "game mode is unsupported"))?;
        let title_len = cursor.u8()? as usize;
        let title_bytes = cursor.bytes(title_len)?;
        validate_c_string(title_bytes, "title")?;
        let title = decode_c_string(title_bytes, nls)?;
        let syscall_count = cursor.u16()? as usize;
        if syscall_count > MAX_SYSCALLS {
            return Err(FvpFormatError::new(
                "FVP_HCB_SYSCALL_COUNT",
                "syscall count exceeds the supported bound",
            ));
        }
        let mut syscalls = Vec::with_capacity(syscall_count);
        let mut names = BTreeSet::new();
        for id in 0..syscall_count {
            let args = cursor.u8()?;
            let len = cursor.u8()? as usize;
            let name_bytes = cursor.bytes(len)?;
            validate_c_string(name_bytes, "syscall name")?;
            let name = decode_c_string(name_bytes, nls)?;
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(FvpFormatError::new(
                    "FVP_HCB_SYSCALL_NAME",
                    "syscall name is not a safe symbol",
                ));
            }
            if !names.insert(name.clone()) {
                return Err(FvpFormatError::new(
                    "FVP_HCB_SYSCALL_DUPLICATE",
                    "syscall name is duplicated",
                ));
            }
            syscalls.push(FvpSyscallDescriptor {
                id: id as u16,
                args,
                name,
            });
        }
        let custom_syscall_count = cursor.u16()?;
        Ok(Self {
            header: FvpHcbHeader {
                content_hash: Hash256::from_sha256(&bytes),
                sys_desc_offset,
                entry_point,
                non_volatile_global_count,
                volatile_global_count,
                game_mode,
                width,
                height,
                title_hash: Hash256::from_sha256(title.as_bytes()),
                title_byte_len: title_bytes.len() as u32,
                syscalls,
                custom_syscall_count,
            },
            bytes,
            nls,
        })
    }

    pub fn code(&self) -> &[u8] {
        &self.bytes[4..self.header.sys_desc_offset as usize]
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn nls(&self) -> FvpNls {
        self.nls
    }
    pub fn syscall(&self, id: u16) -> Option<&FvpSyscallDescriptor> {
        self.header.syscalls.get(id as usize)
    }
    pub fn decode_script_string(&self, bytes: &[u8]) -> Result<String, FvpFormatError> {
        decode_c_string(bytes, self.nls)
    }
}

pub fn game_mode_size(mode: u8) -> Option<(u32, u32)> {
    const MODES: [(u32, u32); 16] = [
        (640, 480),
        (800, 600),
        (1024, 768),
        (1280, 960),
        (1600, 1200),
        (640, 480),
        (1024, 576),
        (1024, 640),
        (1280, 720),
        (1280, 800),
        (1440, 810),
        (1440, 900),
        (1680, 945),
        (1680, 1050),
        (1920, 1080),
        (1920, 1200),
    ];
    MODES.get(mode as usize).copied()
}

fn validate_c_string(bytes: &[u8], subject: &str) -> Result<(), FvpFormatError> {
    if bytes.is_empty()
        || bytes.len() > MAX_STRING_BYTES
        || bytes.last() != Some(&0)
        || bytes[..bytes.len() - 1].contains(&0)
    {
        return Err(FvpFormatError::new(
            "FVP_HCB_C_STRING",
            format!("{subject} is not a bounded, single NUL-terminated string"),
        ));
    }
    Ok(())
}

fn decode_c_string(bytes: &[u8], nls: FvpNls) -> Result<String, FvpFormatError> {
    validate_c_string(bytes, "string")?;
    let (value, _, malformed) = nls.encoding().decode(&bytes[..bytes.len() - 1]);
    if malformed {
        return Err(FvpFormatError::new(
            "FVP_HCB_ENCODING",
            "string cannot be decoded with the selected NLS",
        ));
    }
    Ok(value.into_owned())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], position: usize) -> Self {
        Self { bytes, position }
    }
    fn bytes(&mut self, count: usize) -> Result<&'a [u8], FvpFormatError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| FvpFormatError::new("FVP_HCB_CURSOR_OVERFLOW", "cursor overflowed"))?;
        let value = self.bytes.get(self.position..end).ok_or_else(|| {
            FvpFormatError::new("FVP_HCB_TRUNCATED", "HCB ended while reading a field")
        })?;
        self.position = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, FvpFormatError> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, FvpFormatError> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, FvpFormatError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{code}: {message}")]
pub struct FvpFormatError {
    code: &'static str,
    message: String,
}
impl FvpFormatError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn code(&self) -> &'static str {
        self.code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn parses_synthetic_header() {
        let mut bytes = vec![0; 64];
        bytes[0..4].copy_from_slice(&16u32.to_le_bytes());
        let mut desc = Vec::new();
        desc.extend_from_slice(&4u32.to_le_bytes());
        desc.extend_from_slice(&1u16.to_le_bytes());
        desc.extend_from_slice(&2u16.to_le_bytes());
        desc.extend_from_slice(&[8, 0, 2, b'X', 0]);
        desc.extend_from_slice(&1u16.to_le_bytes());
        desc.extend_from_slice(&[1, 5, b'W', b'a', b'i', b't', 0]);
        desc.extend_from_slice(&0u16.to_le_bytes());
        bytes.truncate(16);
        bytes.extend_from_slice(&desc);
        let script = FvpHcbScript::parse(bytes, FvpNls::Utf8).unwrap();
        assert_eq!((script.header.width, script.header.height), (1280, 720));
        assert_eq!(script.header.syscalls[0].name, "Wait");
    }

    proptest! {
        #[test]
        fn arbitrary_hcb_bytes_are_total_and_deterministic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let first = FvpHcbScript::parse(bytes.clone(), FvpNls::Utf8)
                .map(|script| script.header)
                .map_err(|error| error.code());
            let second = FvpHcbScript::parse(bytes, FvpNls::Utf8)
                .map(|script| script.header)
                .map_err(|error| error.code());
            prop_assert_eq!(first, second);
        }
    }
}
