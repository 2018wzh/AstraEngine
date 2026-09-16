mod index;
pub use index::*;
mod header;
pub use header::*;

use astra_emu_sdk::CoreError;
use md5::{Digest, Md5};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CmvsSchemeProfile;
use encoding_rs::SHIFT_JIS;

const CPZ5_INIT_CHECKSUM: u32 = 0x923a_564c;
const CPZ5_HEADER_BYTES: usize = 0x40;
const CPZ7_HEADER_BYTES: usize = 0x48;
const MAX_INDEX_BYTES: u64 = 512 * 1024 * 1024;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct CpzArchiveKey {
    pub index_dir: u32,
    pub index_entry: u32,
    pub entry_data_1: u32,
    pub entry_data_2: u32,
}

#[allow(dead_code)]
pub(crate) fn derive_archive_key(
    start: &[u8],
    archive_file_name: &str,
) -> Result<CpzArchiveKey, CoreError> {
    let table_count = le_u32(start, 0x10)? as usize;
    let bytecode_size = le_u32(start, 0x14)? as usize;
    let string_size = le_u32(start, 0x1c)? as usize;
    let bytecode_start = 0x30usize
        .checked_add(table_count.checked_mul(4).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
                "PS2A name table overflowed",
            )
        })?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
                "PS2A bytecode offset overflowed",
            )
        })?;
    let strings_start = bytecode_start.checked_add(bytecode_size).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
            "PS2A strings offset overflowed",
        )
    })?;
    let strings_end = strings_start
        .checked_add(string_size)
        .filter(|end| *end <= start.len())
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
                "PS2A strings are outside decoded script",
            )
        })?;
    let mut cursor = strings_start;
    let mut archive_id = None;
    while cursor < strings_end {
        let end = start[cursor..strings_end]
            .iter()
            .position(|byte| *byte == 0)
            .map(|size| cursor + size)
            .unwrap_or(strings_end);
        if end > cursor {
            let (name, _, malformed) = SHIFT_JIS.decode(&start[cursor..end]);
            if malformed {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
                    "PS2A archive string is not CP932",
                ));
            }
            if name.eq_ignore_ascii_case(archive_file_name) {
                archive_id = Some(u32::try_from(cursor - strings_start).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
                        "PS2A archive string id exceeds u32",
                    )
                })?);
                break;
            }
        }
        cursor = end.saturating_add(1);
    }
    let archive_id = archive_id
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
                "PS2A does not reference the CPZ archive",
            )
        })?
        .to_le_bytes();
    for position in bytecode_start
        ..bytecode_start
            .saturating_add(bytecode_size)
            .min(start.len())
    {
        if position < 0x30
            || position + 4 > start.len()
            || start[position..position + 4] != archive_id
        {
            continue;
        }
        if position < 0x30 || start.get(position - 0x33..position - 0x30) != Some(&[2, 0, 1]) {
            continue;
        }
        return Ok(CpzArchiveKey {
            index_dir: le_u32(start, position - 0x0c)?,
            index_entry: le_u32(start, position - 0x18)?,
            entry_data_1: le_u32(start, position - 0x24)?,
            entry_data_2: le_u32(start, position - 0x30)?,
        });
    }
    Err(invalid(
        "ASTRA_EMU_CMVS_CPZ_ARCHIVE_KEY",
        "PS2A archive key bytecode pattern is missing",
    ))
}

pub fn validate_archive_key(start: &[u8], archive_file_name: &str) -> Result<(), CoreError> {
    let header = crate::ps2a::parse_header_for_archive_key(start)?;
    let decoded = crate::ps2a::decode_ps2a(start, header)?;
    derive_archive_key(&decoded, archive_file_name).map(|_| ())
}

#[allow(dead_code)]
pub(crate) struct Cpz5Decoder {
    table: [u8; 256],
    scheme: CmvsSchemeProfile,
}

#[allow(dead_code)]
impl Cpz5Decoder {
    pub(crate) fn new(scheme: CmvsSchemeProfile, key: u32, summand: u32) -> Self {
        let mut decoder = Self {
            table: std::array::from_fn(|index| index as u8),
            scheme,
        };
        decoder.initialize(key, summand);
        decoder
    }

    pub(crate) fn initialize(&mut self, mut key: u32, summand: u32) {
        self.table = std::array::from_fn(|index| index as u8);
        for _ in 0..256 {
            self.table
                .swap(((key >> 16) & 0xff) as usize, (key & 0xff) as usize);
            self.table
                .swap(((key >> 8) & 0xff) as usize, (key >> 24) as usize);
            key =
                summand.wrapping_add(self.scheme.decoder_factor.wrapping_mul(key.rotate_right(2)));
        }
    }

    pub(crate) fn decode(&self, data: &mut [u8], xor: u8) {
        for byte in data {
            *byte = self.table[(xor ^ *byte) as usize];
        }
    }

    pub(crate) fn decrypt_entry(
        &self,
        data: &mut [u8],
        cmvs_md5: [u32; 4],
        seed: u32,
    ) -> Result<(), CoreError> {
        if self.scheme.cpz5_secret.len() < 16 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_SECRET",
                "CPZ scheme secret is too short",
            ));
        }
        let mut secret_bytes = Vec::with_capacity(64);
        for word in self.scheme.cpz5_secret.iter().take(16) {
            secret_bytes.extend_from_slice(&word.to_le_bytes());
        }
        let key = cmvs_md5[1] >> 2;
        for byte in &mut secret_bytes {
            *byte = (key as u8) ^ self.table[*byte as usize];
        }
        let mut secret = [0u32; 16];
        for (index, slot) in secret.iter_mut().enumerate() {
            *slot = u32::from_le_bytes(
                secret_bytes[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("secret word"),
            ) ^ seed;
        }
        let mut rolling = self.scheme.entry_init_key;
        let mut secret_index = usize::from(self.scheme.entry_key_pos & 15);
        let (words, tail) = data.as_chunks_mut::<4>();
        for chunk in words {
            let encrypted = u32::from_le_bytes(*chunk);
            let decoded = cmvs_md5[(rolling & 3) as usize]
                ^ ((encrypted
                    ^ secret[((rolling >> 6) & 15) as usize]
                    ^ (secret[secret_index] >> 1))
                    .wrapping_sub(seed));
            *chunk = decoded.to_le_bytes();
            rolling = rolling.wrapping_add(seed).wrapping_add(decoded);
            secret_index = (secret_index + 1) & 15;
        }
        for byte in tail {
            *byte = self.table[(*byte ^ self.scheme.entry_tail_key) as usize];
        }
        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cpz5Scheme {
    pub secret: [u32; 24],
    pub index_addend: u32,
    pub index_subtrahend: u8,
    pub index_seed: u32,
    pub directory_key_addend: [u32; 4],
}

/// A decoded CPZ5 index entry.  This remains crate-private because the name is
/// commercial case material until the family VFS has applied its URI and
/// manifest redaction rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpz5IndexEntry {
    pub relative_path: String,
    pub offset: u64,
    pub stored_size: u64,
    pub checksum: u32,
    pub key: u32,
}

impl Cpz5Scheme {
    fn from_profile(profile: &CmvsSchemeProfile) -> Result<Self, CoreError> {
        if profile.cpz5_secret.len() != 24 || profile.dir_key_addend.len() != 4 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_PROFILE",
                "CPZ5 profile has an invalid fixed-size key table",
            ));
        }
        let secret = profile.cpz5_secret.clone().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_PROFILE",
                "CPZ5 secret table length is invalid",
            )
        })?;
        let directory_key_addend = profile.dir_key_addend.clone().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_PROFILE",
                "CPZ5 directory key table length is invalid",
            )
        })?;
        let index_subtrahend = u8::try_from(profile.index_subtrahend).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_PROFILE",
                "CPZ5 index subtrahend exceeds one byte",
            )
        })?;
        Ok(Self {
            secret,
            index_addend: profile.index_addend,
            index_subtrahend,
            index_seed: profile.index_seed,
            directory_key_addend,
        })
    }
}

fn index_key(cmvs_md5: [u32; 4], master_key: u32) -> [u32; 4] {
    [
        cmvs_md5[0] ^ master_key.wrapping_add(0x76a3_bf29),
        cmvs_md5[1] ^ master_key,
        cmvs_md5[2] ^ master_key.wrapping_add(0x1000_0000),
        cmvs_md5[3] ^ master_key,
    ]
}

fn canonical_cmvs_path(directory: &str, entry: &str) -> Result<String, CoreError> {
    let path = if directory.eq_ignore_ascii_case("root") {
        entry.to_owned()
    } else {
        format!("{directory}/{entry}")
    };
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_PATH",
            "CPZ5 entry path is absolute or traverses outside its archive",
        ));
    }
    Ok(normalized)
}

fn decode_cp932_cstring(bytes: &[u8], field: &'static str) -> Result<String, CoreError> {
    // GARbro's bounded `GetCString` accepts a record-end terminated string.
    // The caller has already validated the enclosing directory or entry span,
    // so using that end preserves the format behavior without scanning beyond
    // the encrypted index record.
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    if end == 0 {
        return Err(invalid("ASTRA_EMU_CMVS_CPZ_STRING", "CPZ5 string is empty"));
    }
    let (text, _, malformed) = SHIFT_JIS.decode(&bytes[..end]);
    if malformed {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_STRING",
            "CPZ5 string is not CP932",
        ));
    }
    if text.len() > 4096 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_STRING",
            "CPZ5 string exceeds the bound",
        ));
    }
    let _ = field;
    Ok(text.into_owned())
}

fn checked_index_size(value: u64) -> Result<usize, CoreError> {
    usize::try_from(value).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_INDEX_SIZE",
            "CPZ5 index exceeds platform bounds",
        )
    })
}

fn usize_from_u32(value: u32) -> Result<usize, CoreError> {
    usize::try_from(value).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_INDEX_SIZE",
            "CPZ5 value exceeds platform bounds",
        )
    })
}

fn read_u32(index: &[u8], offset: usize, _field: &'static str) -> Result<u32, CoreError> {
    let bytes: [u8; 4] = index
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_RANGE",
                "CPZ5 index record is truncated",
            )
        })?
        .try_into()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_RANGE",
                "CPZ5 index record is truncated",
            )
        })?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(index: &[u8], offset: usize, _field: &'static str) -> Result<u64, CoreError> {
    let bytes: [u8; 8] = index
        .get(offset..offset.saturating_add(8))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_RANGE",
                "CPZ5 index record is truncated",
            )
        })?
        .try_into()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_RANGE",
                "CPZ5 index record is truncated",
            )
        })?;
    Ok(u64::from_le_bytes(bytes))
}

#[allow(dead_code)]
impl Cpz5Scheme {
    pub(crate) fn decrypt_index_stage_1(&self, data: &mut [u8], key: u32) {
        let secret = self.secret.map(|value| value.wrapping_sub(key));
        let shift = ((key >> 24) ^ (key >> 16) ^ (key >> 8) ^ key ^ 0x0b) & 0x0f;
        let shift = shift + 7;
        let mut secret_index = 5usize;
        let words = data.len() / 4;
        let remainder_length = data.len() - words * 4;
        for index in 0..words {
            let offset = index * 4;
            let encrypted = u32::from_le_bytes(data[offset..offset + 4].try_into().expect("word"));
            let decoded = (secret[secret_index] ^ encrypted)
                .wrapping_add(self.index_addend)
                .rotate_right(shift)
                .wrapping_add(0x0101_0101);
            data[offset..offset + 4].copy_from_slice(&decoded.to_le_bytes());
            secret_index = (secret_index + 1) % secret.len();
        }
        for (remainder_index, byte) in data[words * 4..].iter_mut().enumerate() {
            let shift = u32::try_from((remainder_length - remainder_index) * 4)
                .expect("small remainder shift");
            *byte =
                (*byte ^ (secret[secret_index] >> shift) as u8).wrapping_sub(self.index_subtrahend);
            secret_index = (secret_index + 1) % secret.len();
        }
    }

    pub(crate) fn decrypt_directory(&self, data: &mut [u8], key: [u32; 4], archive_key: u32) {
        let mut seed = 0x7654_8aef_u32;
        let words = data.len() / 4;
        for index in 0..words {
            let offset = index * 4;
            let encrypted = u32::from_le_bytes(data[offset..offset + 4].try_into().expect("word"));
            let decoded = (encrypted ^ key[index & 3])
                .wrapping_sub(0x4a91_c262)
                .rotate_left(3)
                .wrapping_sub(seed);
            data[offset..offset + 4].copy_from_slice(&decoded.to_le_bytes());
            seed = seed.wrapping_add(0x10fb_562a ^ archive_key);
        }
        for (remainder_index, byte) in data[words * 4..].iter_mut().enumerate() {
            *byte = (*byte ^ (key[(words + remainder_index) & 3] >> 6) as u8).wrapping_add(0x37);
        }
    }

    pub(crate) fn decrypt_entries(&self, data: &mut [u8], key: [u32; 4], archive_key: u32) {
        let mut seed = self.index_seed;
        let words = data.len() / 4;
        for index in 0..words {
            let offset = index * 4;
            let encrypted = u32::from_le_bytes(data[offset..offset + 4].try_into().expect("word"));
            let decoded = (encrypted ^ key[index & 3])
                .wrapping_sub(seed)
                .rotate_left(2)
                .wrapping_add(0x37a1_9e8b);
            data[offset..offset + 4].copy_from_slice(&decoded.to_le_bytes());
            seed = seed.wrapping_sub(0x0139_fa9b ^ archive_key);
        }
        for (remainder_index, byte) in data[words * 4..].iter_mut().enumerate() {
            *byte = (*byte ^ (key[(words + remainder_index) & 3] >> 4) as u8).wrapping_add(5);
        }
    }
}

fn additive_checksum(bytes: &[u8], mut value: u32) -> u32 {
    let (words, tail) = bytes.as_chunks::<4>();
    for word in words {
        value = value.wrapping_add(u32::from_le_bytes(*word));
    }
    for byte in tail {
        value = value.wrapping_add(u32::from(*byte));
    }
    value
}

fn le_u32(source: &[u8], offset: usize) -> Result<u32, CoreError> {
    let bytes: [u8; 4] = source
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_CPZ_HEADER", "CPZ header is truncated"))?
        .try_into()
        .map_err(|_| invalid("ASTRA_EMU_CMVS_CPZ_HEADER", "CPZ header is truncated"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
