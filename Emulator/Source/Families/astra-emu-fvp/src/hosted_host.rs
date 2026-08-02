//! Bounded in-memory host ports for RFVP hosted-core.
//!
//! The dynamic VFS bridge is added separately; this port deliberately owns no
//! platform renderer/audio object and is usable for registered case images.

use std::{collections::BTreeMap, sync::Arc};

use rfvp_hosted::host_api::{
    AudioParams, AudioStreamDesc, AudioStreamId, ColorRgba, DrawSolidCommand, DrawSpriteCommand,
    EncodedAudioKind, RfvpAudio, RfvpClock, RfvpError, RfvpFile, RfvpFileInfo, RfvpFileSystem,
    RfvpHost, RfvpRenderer, RfvpResult, TextureDesc, TextureId, TextureRect,
};

pub const MAX_HOSTED_FILES: usize = 65_536;
pub const MAX_HOSTED_FILE_BYTES: usize = 512 * 1024 * 1024;

pub struct HostedMemoryHost {
    fs: HostedMemoryFileSystem,
    renderer: RejectingRenderer,
    audio: RejectingAudio,
    clock: StepClock,
}

impl HostedMemoryHost {
    pub fn new(files: BTreeMap<String, Vec<u8>>) -> RfvpResult<Self> {
        if files.len() > MAX_HOSTED_FILES
            || files
                .values()
                .any(|bytes| bytes.len() > MAX_HOSTED_FILE_BYTES)
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
                files: Arc::new(normalized),
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
    files: Arc<BTreeMap<String, Vec<u8>>>,
}
pub struct HostedMemoryFile {
    bytes: Vec<u8>,
}

impl RfvpFileSystem for HostedMemoryFileSystem {
    type File = HostedMemoryFile;
    fn open(&mut self, path: &str) -> RfvpResult<Self::File> {
        self.files
            .get(&normalize(path)?)
            .cloned()
            .map(|bytes| HostedMemoryFile { bytes })
            .ok_or(RfvpError::NotFound)
    }
    fn metadata(&mut self, path: &str) -> RfvpResult<RfvpFileInfo> {
        self.files
            .get(&normalize(path)?)
            .map(|bytes| RfvpFileInfo::file(bytes.len() as u64))
            .ok_or(RfvpError::NotFound)
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
        let ext = ext.strip_prefix('.').ok_or(RfvpError::InvalidArgument)?;
        if ext.is_empty() {
            return Err(RfvpError::InvalidArgument);
        }
        for (path, bytes) in self.files.iter() {
            if (root.is_empty()
                || path
                    .strip_prefix(&root)
                    .is_some_and(|tail| tail.starts_with('/')))
                && path
                    .rsplit_once('.')
                    .is_some_and(|(_, value)| value.eq_ignore_ascii_case(ext))
            {
                visitor(path, RfvpFileInfo::file(bytes.len() as u64))?;
            }
        }
        Ok(())
    }
}

impl RfvpFile for HostedMemoryFile {
    fn len(&mut self) -> RfvpResult<u64> {
        Ok(self.bytes.len() as u64)
    }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> RfvpResult<usize> {
        let offset = usize::try_from(offset).map_err(|_| RfvpError::EndOfFile)?;
        if offset >= self.bytes.len() {
            return Ok(0);
        }
        let len = out.len().min(self.bytes.len() - offset);
        out[..len].copy_from_slice(&self.bytes[offset..offset + len]);
        Ok(len)
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
    }
}
