//! Bounded in-memory host ports for RFVP hosted-core.
//!
//! The dynamic VFS bridge is added separately; this port deliberately owns no
//! platform renderer/audio object and is usable for registered case images.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_byte_source::{ByteRange, SourceRevision};
use astra_emu_family_api::LegacyVfsReader;
use rfvp_hosted::host_api::{
    AudioParams, AudioStreamDesc, AudioStreamId, ColorRgba, DrawSolidCommand, DrawSpriteCommand,
    EncodedAudioKind, RfvpAudio, RfvpClock, RfvpError, RfvpFile, RfvpFileInfo, RfvpFileSystem,
    RfvpHost, RfvpRenderer, RfvpResult, TextureDesc, TextureId, TextureRect,
};

pub const MAX_HOSTED_FILES: usize = 65_536;
// FVP installations may contain multi-gigabyte graph archives.  Hosted VFS
// keeps those as bounded, paged host-port files; this is a metadata/identity
// limit, not an allocation allowance.  Whole-file reads remain capped by the
// caller and every individual VFS range remains at most 16 MiB.
pub const MAX_HOSTED_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_HOSTED_RANGE_BYTES: usize = 16 * 1024 * 1024;

pub enum HostedFileSource {
    Memory(Arc<BTreeMap<String, Vec<u8>>>),
    Vfs {
        reader: Arc<dyn LegacyVfsReader>,
        mount_set_id: String,
        pack_paths: BTreeSet<String>,
    },
}

pub struct HostedMemoryHost {
    fs: HostedMemoryFileSystem,
    renderer: RejectingRenderer,
    audio: RejectingAudio,
    clock: StepClock,
}

pub type HostedVfsHost = HostedMemoryHost;

impl HostedMemoryHost {
    pub fn new(files: BTreeMap<String, Vec<u8>>) -> RfvpResult<Self> {
        if files.len() > MAX_HOSTED_FILES
            || files
                .values()
                .any(|bytes| bytes.len() as u64 > MAX_HOSTED_FILE_BYTES)
        {
            return Err(RfvpError::CapacityExceeded);
        }
        let mut normalized = BTreeMap::new();
        for (path, bytes) in files {
            let path = normalize(&path)?;
            if normalized.insert(path, bytes).is_some() {
                return Err(RfvpError::InvalidData);
            }
        }
        Ok(Self {
            fs: HostedMemoryFileSystem {
                source: HostedFileSource::Memory(Arc::new(normalized)),
            },
            renderer: RejectingRenderer,
            audio: RejectingAudio,
            clock: StepClock::default(),
        })
    }

    pub fn from_vfs(
        reader: Arc<dyn LegacyVfsReader>,
        mount_set_id: String,
        pack_paths: Vec<String>,
    ) -> RfvpResult<Self> {
        if mount_set_id.is_empty() {
            return Err(RfvpError::InvalidArgument);
        }
        if pack_paths.len() > MAX_HOSTED_FILES {
            return Err(RfvpError::CapacityExceeded);
        }
        let mut normalized_pack_paths = BTreeSet::new();
        for path in pack_paths {
            let path = normalize(&path)?;
            if !path.ends_with(".bin") || !normalized_pack_paths.insert(path) {
                return Err(RfvpError::InvalidArgument);
            }
        }
        Ok(Self {
            fs: HostedMemoryFileSystem {
                source: HostedFileSource::Vfs {
                    reader,
                    mount_set_id,
                    pack_paths: normalized_pack_paths,
                },
            },
            renderer: RejectingRenderer,
            audio: RejectingAudio,
            clock: StepClock::default(),
        })
    }

    pub fn advance(&mut self, delta_ns: u64) -> RfvpResult<()> {
        let delta_us = delta_ns / 1_000;
        if delta_us == 0 {
            return Err(RfvpError::InvalidArgument);
        }
        self.clock.now_us = self
            .clock
            .now_us
            .checked_add(delta_us)
            .ok_or(RfvpError::CapacityExceeded)?;
        Ok(())
    }

    /// Deterministic session time exposed for hosted lifecycle validation.
    /// It is deliberately a scalar rather than a platform clock handle.
    pub fn elapsed_us(&self) -> u64 {
        self.clock.now_us
    }
}

impl RfvpHost for HostedMemoryHost {
    type FileSystem = HostedMemoryFileSystem;
    type Renderer = RejectingRenderer;
    type Audio = RejectingAudio;
    type Clock = StepClock;
    fn fs(&mut self) -> &mut Self::FileSystem {
        &mut self.fs
    }
    fn renderer(&mut self) -> &mut Self::Renderer {
        &mut self.renderer
    }
    fn audio(&mut self) -> &mut Self::Audio {
        &mut self.audio
    }
    fn clock(&mut self) -> &mut Self::Clock {
        &mut self.clock
    }
}

pub struct HostedMemoryFileSystem {
    source: HostedFileSource,
}
pub enum HostedMemoryFile {
    Memory {
        bytes: Vec<u8>,
    },
    Vfs {
        reader: Arc<dyn LegacyVfsReader>,
        mount_set_id: String,
        uri: String,
        len: u64,
        revision: SourceRevision,
    },
}

impl RfvpFileSystem for HostedMemoryFileSystem {
    type File = HostedMemoryFile;
    fn open(&mut self, path: &str) -> RfvpResult<Self::File> {
        let path = normalize(path)?;
        match &self.source {
            HostedFileSource::Memory(files) => files
                .get(&path)
                .cloned()
                .map(|bytes| HostedMemoryFile::Memory { bytes })
                .ok_or(RfvpError::NotFound),
            HostedFileSource::Vfs {
                reader,
                mount_set_id,
                ..
            } => {
                let stat = reader
                    .stat_file(mount_set_id, &path)
                    .map_err(map_vfs_error)?;
                if stat.len > MAX_HOSTED_FILE_BYTES {
                    return Err(RfvpError::CapacityExceeded);
                }
                Ok(HostedMemoryFile::Vfs {
                    reader: Arc::clone(reader),
                    mount_set_id: mount_set_id.clone(),
                    uri: path,
                    len: stat.len,
                    revision: stat.revision,
                })
            }
        }
    }
    fn metadata(&mut self, path: &str) -> RfvpResult<RfvpFileInfo> {
        let path = normalize(path)?;
        match &self.source {
            HostedFileSource::Memory(files) => files
                .get(&path)
                .map(|bytes| RfvpFileInfo::file(bytes.len() as u64))
                .ok_or(RfvpError::NotFound),
            HostedFileSource::Vfs {
                reader,
                mount_set_id,
                ..
            } => reader
                .stat_file(mount_set_id, &path)
                .map(|stat| RfvpFileInfo::file(stat.len))
                .map_err(map_vfs_error),
        }
    }
    fn enumerate_by_extension(
        &mut self,
        root: &str,
        ext: &str,
        visitor: &mut dyn FnMut(&str, RfvpFileInfo) -> RfvpResult<()>,
    ) -> RfvpResult<()> {
        let root = if root == "." {
            String::new()
        } else {
            normalize(root)?
        };
        if ext.is_empty() || ext.starts_with('.') || ext.contains(['/', '\\']) {
            return Err(RfvpError::InvalidArgument);
        }
        match &self.source {
            HostedFileSource::Memory(files) => {
                for (path, bytes) in files.iter() {
                    if in_root_with_extension(path, &root, ext) {
                        visitor(path, RfvpFileInfo::file(bytes.len() as u64))?;
                    }
                }
            }
            HostedFileSource::Vfs {
                reader,
                mount_set_id,
                pack_paths,
            } => {
                if ext.eq_ignore_ascii_case("bin") {
                    for path in pack_paths {
                        if in_root_with_extension(path, &root, ext) {
                            let stat = reader
                                .stat_file(mount_set_id, path)
                                .map_err(map_vfs_error)?;
                            visitor(path, RfvpFileInfo::file(stat.len))?;
                        }
                    }
                    return Ok(());
                }
                let entries = reader
                    .enumerate_by_extension(mount_set_id, &root, ext, MAX_HOSTED_FILES as u32)
                    .map_err(map_vfs_error)?;
                if entries.len() > MAX_HOSTED_FILES {
                    return Err(RfvpError::CapacityExceeded);
                }
                for entry in entries {
                    let path = normalize(&entry.uri)?;
                    if !in_root_with_extension(&path, &root, ext) {
                        return Err(RfvpError::InvalidData);
                    }
                    visitor(&path, RfvpFileInfo::file(entry.stat.len))?;
                }
            }
        }
        Ok(())
    }
}

impl RfvpFile for HostedMemoryFile {
    fn len(&mut self) -> RfvpResult<u64> {
        match self {
            Self::Memory { bytes } => Ok(bytes.len() as u64),
            Self::Vfs { len, .. } => Ok(*len),
        }
    }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> RfvpResult<usize> {
        if out.len() > MAX_HOSTED_RANGE_BYTES {
            return Err(RfvpError::CapacityExceeded);
        }
        match self {
            Self::Memory { bytes } => {
                let offset = usize::try_from(offset).map_err(|_| RfvpError::EndOfFile)?;
                if offset >= bytes.len() {
                    return Ok(0);
                }
                let len = out.len().min(bytes.len() - offset);
                out[..len].copy_from_slice(&bytes[offset..offset + len]);
                Ok(len)
            }
            Self::Vfs {
                reader,
                mount_set_id,
                uri,
                len,
                revision,
            } => {
                if offset >= *len || out.is_empty() {
                    return Ok(0);
                }
                let bytes = (out.len() as u64).min(*len - offset);
                let result = reader
                    .read_file_range(
                        mount_set_id,
                        uri,
                        revision.clone(),
                        ByteRange { offset, len: bytes },
                        MAX_HOSTED_RANGE_BYTES as u64,
                    )
                    .map_err(map_vfs_error)?;
                if result.bytes.len() != bytes as usize {
                    return Err(RfvpError::Io);
                }
                out[..result.bytes.len()].copy_from_slice(&result.bytes);
                Ok(result.bytes.len())
            }
        }
    }
}

#[derive(Default)]
pub struct StepClock {
    now_us: u64,
}
impl RfvpClock for StepClock {
    fn ticks_us(&mut self) -> u64 {
        self.now_us
    }
}
pub struct RejectingRenderer;
impl RfvpRenderer for RejectingRenderer {
    fn create_texture(&mut self, _: TextureId, _: TextureDesc, _: Option<&[u8]>) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn update_texture(&mut self, _: TextureId, _: TextureRect, _: &[u8]) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn destroy_texture(&mut self, _: TextureId) {}
    fn begin_frame(&mut self, _: u32, _: u32, _: Option<ColorRgba>) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn draw_sprite(&mut self, _: &DrawSpriteCommand) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn draw_solid(&mut self, _: &DrawSolidCommand) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn end_frame(&mut self) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn present(&mut self) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
}
pub struct RejectingAudio;
impl RfvpAudio for RejectingAudio {
    fn load_encoded(&mut self, _: AudioStreamId, _: EncodedAudioKind, _: &[u8]) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn create_stream(&mut self, _: AudioStreamId, _: AudioStreamDesc) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn submit_i16(&mut self, _: AudioStreamId, _: &[i16]) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn submit_f32(&mut self, _: AudioStreamId, _: &[f32]) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn play(&mut self, _: AudioStreamId, _: AudioParams, _: u32) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn stop(&mut self, _: AudioStreamId, _: u32) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn pause(&mut self, _: AudioStreamId) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn resume(&mut self, _: AudioStreamId) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn set_params(&mut self, _: AudioStreamId, _: AudioParams) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn set_master_volume(&mut self, _: f32) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
    fn destroy_stream(&mut self, _: AudioStreamId) {}
    fn tick(&mut self, _: u64) -> RfvpResult<()> {
        Err(RfvpError::Backend)
    }
}
fn normalize(path: &str) -> RfvpResult<String> {
    let path = path.strip_prefix("./").unwrap_or(path);
    if path.is_empty()
        || path.contains('\\')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(RfvpError::InvalidArgument);
    }
    Ok(path.to_ascii_lowercase())
}
fn in_root_with_extension(path: &str, root: &str, extension: &str) -> bool {
    (root.is_empty()
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/')))
        && path
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case(extension))
}
fn map_vfs_error(error: astra_emu_family_api::LegacyProviderError) -> RfvpError {
    match error.code() {
        "ASTRA_EMU_VFS_NOT_FOUND" => RfvpError::NotFound,
        "ASTRA_EMU_VFS_BOUNDS" => RfvpError::CapacityExceeded,
        _ => RfvpError::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_port_normalizes_and_bounds_files() {
        let mut host = HostedMemoryHost::new(BTreeMap::from([
            ("GAME.HCB".into(), vec![1, 2]),
            ("movie/opening.wmv".into(), vec![3]),
        ]))
        .expect("valid hosted files");
        let mut file = host.fs().open("game.hcb").expect("normalized open");
        let mut bytes = [0u8; 2];
        assert_eq!(file.read_at(0, &mut bytes).expect("read"), 2);
        assert_eq!(bytes, [1, 2]);
        assert!(host.fs().open("../game.hcb").is_err());
        host.advance(16_667_000).expect("fixed clock advances");
        assert_eq!(host.clock().ticks_us(), 16_667);
        assert_eq!(host.elapsed_us(), 16_667);
    }
}
