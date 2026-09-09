use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::mem::size_of;

use anyhow::{anyhow, bail, Result};

use crate::host_api::RfvpFile;
use crate::script::parser::Nls;
use crate::utils::stable_hash::StableHashMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Hosted resource streams deliberately stay on the session thread.  The
/// embedding owns cross-thread decoder scheduling; RFVP does not require a
/// `Send + Sync` file handle just to access a bounded pack subrange.
pub type VfsStream = Box<dyn ReadSeek>;

struct HostedPackReader {
    file: Rc<RefCell<Box<dyn RfvpFile>>>,
    base: u64,
    length: Option<u64>,
    position: u64,
}

impl HostedPackReader {
    fn new(file: Rc<RefCell<Box<dyn RfvpFile>>>, base: u64, length: Option<u64>) -> Self {
        Self {
            file,
            base,
            length,
            position: 0,
        }
    }

    fn io_error(error: crate::host_api::RfvpError) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, error.to_string())
    }

    fn total_length(&self) -> std::io::Result<u64> {
        match self.length {
            Some(length) => Ok(length),
            None => {
                let mut file = self.file.try_borrow_mut().map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::Other, "host pack is already borrowed")
                })?;
                let length = file.len().map_err(Self::io_error)?;
                length.checked_sub(self.base).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "host pack base exceeds file length",
                    )
                })
            }
        }
    }
}

impl Read for HostedPackReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.total_length()?.saturating_sub(self.position);
        if remaining == 0 || buffer.is_empty() {
            return Ok(0);
        }
        let request = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let offset = self.base.checked_add(self.position).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "host pack offset overflow")
        })?;
        let mut file = self.file.try_borrow_mut().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "host pack is already borrowed")
        })?;
        let read = file
            .read_at(offset, &mut buffer[..request])
            .map_err(Self::io_error)?;
        if read > request {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "host pack returned more bytes than requested",
            ));
        }
        self.position = self.position.checked_add(read as u64).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "host pack position overflow",
            )
        })?;
        Ok(read)
    }
}

impl Seek for HostedPackReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let end = self.total_length()?;
        let target = match from {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(end) + i128::from(delta),
        };
        if target < 0 || target > i128::from(end) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek falls outside hosted pack range",
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}

#[derive(Debug, Clone)]
pub struct VfsEntry {
    pub offset: u64,
    pub size: u64,
}

enum PackSource {
    Bytes(Vec<u8>),
    Hosted(Rc<RefCell<Box<dyn RfvpFile>>>),
}

impl core::fmt::Debug for PackSource {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bytes(bytes) => formatter.debug_tuple("Bytes").field(&bytes.len()).finish(),
            Self::Hosted(_) => formatter.write_str("Hosted(range-read)"),
        }
    }
}

#[derive(Debug)]
pub struct VfsFile {
    pub path: PathBuf,
    pub folder_name: String,
    pub file_count: u64,
    pub filename_table_size: u64,
    pub entries: StableHashMap<String, VfsEntry>,
    pub nls: Nls,
    source: PackSource,
    overrides: StableHashMap<String, Vec<u8>>,
}

impl VfsFile {
    pub fn from_pack_bytes(folder_name: String, pack_bytes: Vec<u8>, nls: Nls) -> Result<Self> {
        let mut cursor = Cursor::new(pack_bytes.clone());
        let (file_count, filename_table_size, entries) = Self::parse_reader(&mut cursor, nls)?;
        Ok(Self {
            path: PathBuf::from(format!("{folder_name}.bin").as_str()),
            folder_name,
            file_count,
            filename_table_size,
            entries,
            nls,
            source: PackSource::Bytes(pack_bytes),
            overrides: StableHashMap::default(),
        })
    }

    /// Parse only pack metadata from a host-owned range-readable file.  Pack
    /// bodies are never copied into RFVP memory; each entry is served by a
    /// bounded subrange reader when an engine subsystem actually opens it.
    pub fn from_host_file(folder_name: String, file: Box<dyn RfvpFile>, nls: Nls) -> Result<Self> {
        let source = Rc::new(RefCell::new(file));
        let mut reader = HostedPackReader::new(source.clone(), 0, None);
        let (file_count, filename_table_size, entries) = Self::parse_reader(&mut reader, nls)?;
        Ok(Self {
            path: PathBuf::from(format!("{folder_name}.bin").as_str()),
            folder_name,
            file_count,
            filename_table_size,
            entries,
            nls,
            source: PackSource::Hosted(source),
            overrides: StableHashMap::default(),
        })
    }

    fn read_u32le(reader: &mut (impl Read + Seek), offset: u64) -> Result<u32> {
        let mut buffer = [0u8; 4];
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut buffer)?;
        Ok(u32::from_le_bytes(buffer))
    }

    fn read_filename_table(
        reader: &mut (impl Read + Seek),
        offset: u64,
        size: u64,
        nls: Nls,
    ) -> Result<StableHashMap<u64, String>> {
        let size = usize::try_from(size).map_err(|_| anyhow!("filename table too large"))?;
        let mut buffer = Vec::new();
        buffer.resize(size, 0);
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut buffer)?;

        let mut results = StableHashMap::default();
        let mut start = 0usize;
        for (i, &b) in buffer.iter().enumerate() {
            if b == 0 {
                let s = match nls {
                    Nls::ShiftJIS => {
                        let (s, _, _) = encoding_rs::SHIFT_JIS.decode(&buffer[start..i]);
                        s.to_string()
                    }
                    Nls::GBK => {
                        let (s, _, _) = encoding_rs::GBK.decode(&buffer[start..i]);
                        s.to_string()
                    }
                    Nls::UTF8 => String::from_utf8_lossy(&buffer[start..i]).into_owned(),
                };
                results.insert(start as u64, s);
                start = i + 1;
            }
        }
        Ok(results)
    }

    fn parse_reader(
        reader: &mut (impl Read + Seek),
        nls: Nls,
    ) -> Result<(u64, u64, StableHashMap<String, VfsEntry>)> {
        let mut offset = 0u64;
        let file_count = Self::read_u32le(reader, offset)? as u64;
        offset += size_of::<u32>() as u64;

        let filename_table_size = Self::read_u32le(reader, offset)? as u64;
        offset += size_of::<u32>() as u64;

        let entries_offset = offset;
        let filename_table_offset = entries_offset
            .checked_add(
                file_count
                    .checked_mul(12)
                    .ok_or_else(|| anyhow!("pack entry table overflow"))?,
            )
            .ok_or_else(|| anyhow!("pack metadata overflow"))?;
        let filename_table =
            Self::read_filename_table(reader, filename_table_offset, filename_table_size, nls)?;

        reader.seek(SeekFrom::Start(entries_offset))?;
        let mut entries = StableHashMap::default();
        let mut cur = entries_offset;
        for _ in 0..file_count {
            let name_off = Self::read_u32le(reader, cur)? as u64;
            cur += 4;
            let data_off = Self::read_u32le(reader, cur)? as u64;
            cur += 4;
            let data_size = Self::read_u32le(reader, cur)? as u64;
            cur += 4;

            if let Some(name) = filename_table.get(&name_off) {
                let name = name.to_ascii_lowercase();
                if entries
                    .insert(
                        name.clone(),
                        VfsEntry {
                            offset: data_off,
                            size: data_size,
                        },
                    )
                    .is_some()
                {
                    bail!("duplicate case-folded host pack entry: {name}");
                }
            }
        }

        Ok((file_count, filename_table_size, entries))
    }

    pub fn add_override(&mut self, name: &str, bytes: Vec<u8>) {
        self.overrides.insert(name.to_ascii_lowercase(), bytes);
    }

    pub fn open_stream_with_len(&self, name: &str) -> Result<(VfsStream, Option<u64>)> {
        let name = name.to_ascii_lowercase();
        if let Some(bytes) = self.overrides.get(&name) {
            return Ok((
                Box::new(Cursor::new(bytes.clone())),
                Some(bytes.len() as u64),
            ));
        }

        let ent = self
            .entries
            .get(&name)
            .ok_or_else(|| anyhow!("file not found in host pack {}: {}", self.folder_name, name))?;
        match &self.source {
            PackSource::Bytes(pack_bytes) => {
                let start =
                    usize::try_from(ent.offset).map_err(|_| anyhow!("pack offset too large"))?;
                let size =
                    usize::try_from(ent.size).map_err(|_| anyhow!("pack entry too large"))?;
                let end = start
                    .checked_add(size)
                    .ok_or_else(|| anyhow!("pack entry overflow"))?;
                if end > pack_bytes.len() {
                    bail!(
                        "pack entry out of range: {}/{} offset={} size={} pack_size={}",
                        self.folder_name,
                        name,
                        ent.offset,
                        ent.size,
                        pack_bytes.len()
                    );
                }
                Ok((
                    Box::new(Cursor::new(pack_bytes[start..end].to_vec())),
                    Some(ent.size),
                ))
            }
            PackSource::Hosted(file) => Ok((
                Box::new(HostedPackReader::new(
                    file.clone(),
                    ent.offset,
                    Some(ent.size),
                )),
                Some(ent.size),
            )),
        }
    }

    pub fn open_stream(&self, name: &str) -> Result<VfsStream> {
        self.open_stream_with_len(name).map(|(stream, _)| stream)
    }

    pub fn read_file(&self, name: &str) -> Result<Vec<u8>> {
        let (mut stream, len) = self.open_stream_with_len(name)?;
        let mut out = Vec::with_capacity(len.and_then(|n| usize::try_from(n).ok()).unwrap_or(0));
        stream.read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn save(&mut self, name: &str, content: Vec<u8>) -> Result<()> {
        self.overrides.insert(name.to_ascii_lowercase(), content);
        Ok(())
    }
}

#[derive(Debug)]
pub struct Vfs {
    pub files: StableHashMap<String, VfsFile>,
    loose_files: StableHashMap<String, Vec<u8>>,
    pub nls: Nls,
}

impl Default for Vfs {
    fn default() -> Self {
        Self {
            files: StableHashMap::default(),
            loose_files: StableHashMap::default(),
            nls: Nls::ShiftJIS,
        }
    }
}

impl Vfs {
    pub fn new(nls: Nls) -> Result<Self> {
        Ok(Self {
            nls,
            ..Self::default()
        })
    }

    pub fn add_loose_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.loose_files.insert(normalize_vfs_key(path), bytes);
    }

    pub fn add_pack_bytes(&mut self, folder_name: &str, bytes: Vec<u8>) -> Result<()> {
        let folder = folder_name
            .strip_suffix(".bin")
            .unwrap_or(folder_name)
            .to_ascii_lowercase();
        let file = VfsFile::from_pack_bytes(folder.clone(), bytes, self.nls)?;
        self.files.insert(folder, file);
        Ok(())
    }

    pub fn add_host_pack(&mut self, folder_name: &str, file: Box<dyn RfvpFile>) -> Result<()> {
        let folder = folder_name
            .strip_suffix(".bin")
            .unwrap_or(folder_name)
            .to_ascii_lowercase();
        let file = VfsFile::from_host_file(folder.clone(), file, self.nls)?;
        self.files.insert(folder, file);
        Ok(())
    }

    pub fn open_stream_with_len(&self, path: &str) -> Result<(VfsStream, Option<u64>)> {
        let key = normalize_vfs_key(path);
        if let Some(bytes) = self.loose_files.get(&key) {
            return Ok((
                Box::new(Cursor::new(bytes.clone())),
                Some(bytes.len() as u64),
            ));
        }

        let (folder, inner) = key
            .split_once('/')
            .ok_or_else(|| anyhow!("file not found: {}", path))?;
        let file = self
            .files
            .get(&folder.to_ascii_lowercase())
            .ok_or_else(|| anyhow!("pack not found for folder '{}'", folder))?;
        file.open_stream_with_len(inner)
    }

    pub fn open_stream(&self, path: &str) -> Result<VfsStream> {
        self.open_stream_with_len(path).map(|(stream, _)| stream)
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let (mut stream, len) = self.open_stream_with_len(path)?;
        let mut out = Vec::with_capacity(len.and_then(|n| usize::try_from(n).ok()).unwrap_or(0));
        stream.read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn save(&mut self, path: &str, content: Vec<u8>) -> Result<()> {
        let key = normalize_vfs_key(path);
        if let Some((folder, inner)) = key.split_once('/') {
            if let Some(file) = self.files.get_mut(folder) {
                file.save(inner, content)?;
                return Ok(());
            }
        }
        self.loose_files.insert(key, content);
        Ok(())
    }

    pub fn find_ani(&self) -> Result<Vec<PathBuf>> {
        Ok(self
            .loose_files
            .keys()
            .filter(|key| {
                let lower = key.to_ascii_lowercase();
                lower.starts_with("cursor") && lower.ends_with(".ani")
            })
            .map(|key| PathBuf::from(key.as_str()))
            .collect())
    }
}

fn normalize_vfs_key(path: &str) -> String {
    path.trim_start_matches("./")
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, _) in entries {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }
        let data_offset = 8 + entries.len() * 12 + names.len();
        let mut next_data_offset = data_offset as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
        for ((_, data), name_offset) in entries.iter().zip(name_offsets) {
            bytes.extend_from_slice(&name_offset.to_le_bytes());
            bytes.extend_from_slice(&next_data_offset.to_le_bytes());
            bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
            next_data_offset += data.len() as u32;
        }
        bytes.extend_from_slice(&names);
        for (_, data) in entries {
            bytes.extend_from_slice(data);
        }
        bytes
    }

    #[test]
    fn hosted_pack_lookup_is_ascii_case_insensitive() {
        let mut vfs = Vfs::new(Nls::UTF8).expect("create hosted VFS");
        vfs.add_pack_bytes("graph.bin", pack(&[("title_bg1_A", b"pixels")]))
            .expect("parse hosted pack");
        assert_eq!(
            vfs.read_file("GRAPH/TITLE_BG1_a")
                .expect("read mixed-case path"),
            b"pixels"
        );
    }

    #[test]
    fn hosted_pack_rejects_case_folded_duplicates() {
        let error = VfsFile::from_pack_bytes(
            "graph".into(),
            pack(&[("Title", b"a"), ("title", b"b")]),
            Nls::UTF8,
        )
        .expect_err("case-folded duplicate must fail");
        assert!(error
            .to_string()
            .contains("duplicate case-folded host pack entry"));
    }
}
