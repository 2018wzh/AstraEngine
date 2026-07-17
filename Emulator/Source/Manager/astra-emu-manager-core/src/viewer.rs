use std::{io::Cursor, sync::Arc};

use astra_emu_family_api::{LegacyMountedVfs, LegacyProviderError, LegacyVfsNode, LegacyVfsStat};
use encoding_rs::{Encoding, SHIFT_JIS, UTF_16BE, UTF_16LE, UTF_8};
use image::ImageReader;
use symphonia::core::{
    formats::{probe::Hint, FormatOptions, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

const MAX_VIEWER_PAGE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerPage {
    pub uri: String,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerPreview {
    Text {
        encoding: String,
        text: String,
        truncated: bool,
    },
    Image {
        format: String,
        width: u32,
        height: u32,
    },
    Audio {
        codec: String,
        sample_rate: Option<u32>,
        channels: Option<u32>,
    },
    Hex {
        text: String,
        truncated: bool,
    },
}

pub struct LegacyVfsViewer {
    vfs: Arc<dyn LegacyMountedVfs>,
}

impl LegacyVfsViewer {
    pub fn new(vfs: Arc<dyn LegacyMountedVfs>) -> Self {
        Self { vfs }
    }
    pub fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyProviderError> {
        self.vfs.read_dir(uri)
    }
    pub fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyProviderError> {
        self.vfs.stat(uri)
    }
    pub fn page(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<ViewerPage, LegacyProviderError> {
        if length == 0 || length > MAX_VIEWER_PAGE_BYTES {
            return Err(error(
                "ASTRA_EMU_VIEWER_PAGE_LIMIT",
                "viewer page length is outside the supported bound",
            ));
        }
        let read = self.vfs.read_range(uri, offset, length)?;
        Ok(ViewerPage {
            uri: read.uri,
            offset: read.offset,
            bytes: read.bytes,
            eof: read.eof,
            cache_hit: read.cache_hit,
        })
    }
    pub fn search(
        &self,
        root: &str,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<LegacyVfsNode>, LegacyProviderError> {
        if query.is_empty() || query.len() > 256 || max_results == 0 || max_results > 4096 {
            return Err(error(
                "ASTRA_EMU_VIEWER_SEARCH_LIMIT",
                "viewer search request is outside the supported bound",
            ));
        }
        let needle = query.to_lowercase();
        let mut pending = vec![root.to_owned()];
        let mut results = Vec::new();
        while let Some(directory) = pending.pop() {
            for node in self.vfs.read_dir(&directory)? {
                if node.name.to_lowercase().contains(&needle) {
                    results.push(node.clone());
                    if results.len() == max_results {
                        return Ok(results);
                    }
                }
                if matches!(
                    node.kind,
                    astra_emu_family_api::LegacyVfsNodeKind::Directory
                ) {
                    pending.push(node.uri);
                }
            }
        }
        Ok(results)
    }
    pub fn preview(
        &self,
        uri: &str,
        encoding: Option<&str>,
    ) -> Result<ViewerPreview, LegacyProviderError> {
        let stat = self.vfs.stat(uri)?;
        let length = stat.size.min(MAX_PREVIEW_BYTES);
        let bytes = self.vfs.read_range(uri, 0, length)?.bytes;
        let truncated = stat.size > length;
        if let Some(label) = encoding {
            return text_preview(&bytes, label, truncated);
        }
        let reader = ImageReader::new(Cursor::new(&bytes));
        if let Ok(reader) = reader.with_guessed_format() {
            if let Some(format) = reader.format() {
                if let Ok((width, height)) = reader.into_dimensions() {
                    return Ok(ViewerPreview::Image {
                        format: format!("{format:?}").to_lowercase(),
                        width,
                        height,
                    });
                }
            }
        }
        let mut hint = Hint::new();
        if let Some(extension) = uri.rsplit('.').next() {
            hint.with_extension(extension);
        }
        let stream =
            MediaSourceStream::new(Box::new(Cursor::new(bytes.clone())), Default::default());
        if let Ok(probed) = symphonia::default::get_probe().probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        ) {
            if let Some(track) = probed.default_track(TrackType::Audio) {
                if let Some(params) = track.codec_params.as_ref().and_then(|value| value.audio()) {
                    return Ok(ViewerPreview::Audio {
                        codec: format!("{:?}", params.codec),
                        sample_rate: params.sample_rate,
                        channels: params.channels.as_ref().map(|value| value.count() as u32),
                    });
                }
            }
        }
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if text
                .chars()
                .filter(|value| value.is_control() && !matches!(value, '\n' | '\r' | '\t'))
                .count()
                * 100
                <= text.chars().count().max(1)
            {
                return Ok(ViewerPreview::Text {
                    encoding: "utf-8".into(),
                    text: text.into(),
                    truncated,
                });
            }
        }
        Ok(ViewerPreview::Hex {
            text: hex_page(&bytes),
            truncated,
        })
    }
}

fn text_preview(
    bytes: &[u8],
    label: &str,
    truncated: bool,
) -> Result<ViewerPreview, LegacyProviderError> {
    let encoding = match label.to_ascii_lowercase().as_str() {
        "utf-8" => UTF_8,
        "cp932" | "shift_jis" => SHIFT_JIS,
        "utf-16le" => UTF_16LE,
        "utf-16be" => UTF_16BE,
        _ => Encoding::for_label(label.as_bytes()).ok_or_else(|| {
            error(
                "ASTRA_EMU_VIEWER_ENCODING",
                "requested text encoding is unknown",
            )
        })?,
    };
    let (text, _, malformed) = encoding.decode(bytes);
    if malformed {
        return Err(error(
            "ASTRA_EMU_VIEWER_TEXT_DECODE",
            "preview bytes are invalid for the selected encoding",
        ));
    }
    Ok(ViewerPreview::Text {
        encoding: encoding.name().to_ascii_lowercase(),
        text: text.into_owned(),
        truncated,
    })
}
fn hex_page(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            format!(
                "{:08x}  {}",
                row * 16,
                chunk
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn error(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use astra_core::Hash256;
    use astra_emu_family_api::{
        LegacyPackManifest, LegacyVfsEntry, LegacyVfsNodeKind, LegacyVfsReadResult, LegacyVfsStream,
    };

    use super::*;

    struct FixtureVfs {
        manifest: LegacyPackManifest,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl FixtureVfs {
        fn new(files: BTreeMap<String, Vec<u8>>) -> Self {
            let entries = files
                .iter()
                .enumerate()
                .map(|(index, (uri, bytes))| LegacyVfsEntry {
                    uri: uri.clone(),
                    entry_id: format!("entry-{index}"),
                    offset: 0,
                    size: bytes.len() as u64,
                    content_hash: Hash256::from_sha256(bytes),
                    media_kind: "fixture".into(),
                })
                .collect();
            Self {
                manifest: LegacyPackManifest {
                    mount_id: "viewer-fixture".into(),
                    prefix: "fixture:/".into(),
                    reader_id: "viewer-fixture".into(),
                    reader_hash: Hash256::from_sha256(b"viewer-fixture"),
                    entries,
                },
                files,
            }
        }
    }

    impl LegacyMountedVfs for FixtureVfs {
        fn mount_id(&self) -> &str {
            &self.manifest.mount_id
        }

        fn manifest(&self) -> &LegacyPackManifest {
            &self.manifest
        }

        fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyProviderError> {
            let base = if uri.ends_with('/') {
                uri.to_owned()
            } else {
                format!("{uri}/")
            };
            let mut names = BTreeSet::new();
            let mut nodes = Vec::new();
            for candidate in self.files.keys().filter(|value| value.starts_with(&base)) {
                let suffix = &candidate[base.len()..];
                let name = suffix.split('/').next().unwrap_or_default();
                if name.is_empty() || !names.insert(name.to_owned()) {
                    continue;
                }
                nodes.push(LegacyVfsNode {
                    uri: format!("{base}{name}"),
                    name: name.into(),
                    kind: if suffix.contains('/') {
                        LegacyVfsNodeKind::Directory
                    } else {
                        LegacyVfsNodeKind::File
                    },
                });
            }
            Ok(nodes)
        }

        fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyProviderError> {
            if let Some(bytes) = self.files.get(uri) {
                return Ok(LegacyVfsStat {
                    uri: uri.into(),
                    entry_id: Some(uri.into()),
                    kind: LegacyVfsNodeKind::File,
                    size: bytes.len() as u64,
                    content_hash: Some(Hash256::from_sha256(bytes)),
                    archive_role: Some("fixture".into()),
                    method: Some("raw".into()),
                });
            }
            Ok(LegacyVfsStat {
                uri: uri.into(),
                entry_id: None,
                kind: LegacyVfsNodeKind::Directory,
                size: 0,
                content_hash: None,
                archive_role: None,
                method: None,
            })
        }

        fn read_range(
            &self,
            uri: &str,
            offset: u64,
            length: u64,
        ) -> Result<LegacyVfsReadResult, LegacyProviderError> {
            let bytes = self
                .files
                .get(uri)
                .ok_or_else(|| error("ASTRA_EMU_VFS_NOT_FOUND", "fixture entry is missing"))?;
            let end = offset
                .checked_add(length)
                .filter(|end| *end <= bytes.len() as u64)
                .ok_or_else(|| error("ASTRA_EMU_VFS_READ_BOUNDS", "fixture read is invalid"))?;
            Ok(LegacyVfsReadResult {
                uri: uri.into(),
                offset,
                bytes: bytes[offset as usize..end as usize].to_vec(),
                eof: end == bytes.len() as u64,
                cache_hit: true,
            })
        }

        fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyProviderError> {
            let bytes = self
                .files
                .get(uri)
                .ok_or_else(|| error("ASTRA_EMU_VFS_NOT_FOUND", "fixture entry is missing"))?;
            Ok(Box::new(Cursor::new(bytes.clone())))
        }
    }

    fn wave_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&38u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes
    }

    fn viewer() -> LegacyVfsViewer {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 3)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        LegacyVfsViewer::new(Arc::new(FixtureVfs::new(BTreeMap::from([
            ("fixture:/docs/readme.txt".into(), b"fixture text".to_vec()),
            ("fixture:/media/image.png".into(), png.into_inner()),
            ("fixture:/media/sound.wav".into(), wave_fixture()),
            ("fixture:/raw/data.bin".into(), vec![0, 1, 2, 255]),
        ]))))
    }

    #[test]
    fn tree_search_page_and_limits_are_bounded() {
        let viewer = viewer();
        assert_eq!(viewer.read_dir("fixture:/").unwrap().len(), 3);
        assert_eq!(viewer.search("fixture:/", "read", 4).unwrap().len(), 1);
        let page = viewer.page("fixture:/docs/readme.txt", 2, 4).unwrap();
        assert_eq!(page.bytes, b"xtur");
        assert!(page.cache_hit);
        assert_eq!(
            viewer
                .page("fixture:/docs/readme.txt", 0, 0)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VIEWER_PAGE_LIMIT"
        );
    }

    #[test]
    fn text_image_audio_and_hex_previews_are_classified() {
        let viewer = viewer();
        assert!(matches!(
            viewer
                .preview("fixture:/docs/readme.txt", Some("utf-8"))
                .unwrap(),
            ViewerPreview::Text { .. }
        ));
        assert!(matches!(
            viewer.preview("fixture:/media/image.png", None).unwrap(),
            ViewerPreview::Image {
                width: 2,
                height: 3,
                ..
            }
        ));
        assert!(matches!(
            viewer.preview("fixture:/media/sound.wav", None).unwrap(),
            ViewerPreview::Audio { .. }
        ));
        assert!(matches!(
            viewer.preview("fixture:/raw/data.bin", None).unwrap(),
            ViewerPreview::Hex { .. }
        ));
    }
}
