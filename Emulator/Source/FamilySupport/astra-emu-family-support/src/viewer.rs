use std::sync::Arc;

use astra_emu_family_core::{
    LegacyCoreError, LegacyMountedVfs, LegacyVfsNode, LegacyVfsNodeKind, LegacyVfsStat,
};
use astra_media::{
    DecodeBindingContext, DecodeKind, DecodeOutput, DecodeProviderRegistry, DecodeRequest,
};
use encoding_rs::{Encoding, SHIFT_JIS, UTF_16BE, UTF_16LE, UTF_8};

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
    Hex {
        text: String,
        truncated: bool,
    },
    Media {
        provider_id: String,
        kind: DecodeKind,
        codec: String,
        output: DecodeOutput,
    },
}

pub struct LegacyVfsViewer {
    vfs: Arc<dyn LegacyMountedVfs>,
}

impl LegacyVfsViewer {
    pub fn new(vfs: Arc<dyn LegacyMountedVfs>) -> Self {
        Self { vfs }
    }

    pub fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyCoreError> {
        self.vfs.read_dir(uri)
    }
    pub fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyCoreError> {
        self.vfs.stat(uri)
    }

    pub fn page(&self, uri: &str, offset: u64, length: u64) -> Result<ViewerPage, LegacyCoreError> {
        if length == 0 || length > MAX_VIEWER_PAGE_BYTES {
            return Err(invalid(
                "ASTRA_EMU_VFS_VIEWER_PAGE_LIMIT",
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
    ) -> Result<Vec<LegacyVfsNode>, LegacyCoreError> {
        if query.is_empty() || query.len() > 256 || max_results == 0 || max_results > 4096 {
            return Err(invalid(
                "ASTRA_EMU_VFS_VIEWER_SEARCH_LIMIT",
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
                if node.kind == LegacyVfsNodeKind::Directory {
                    pending.push(node.uri);
                }
            }
        }
        Ok(results)
    }

    pub fn preview_text(
        &self,
        uri: &str,
        encoding: &str,
    ) -> Result<ViewerPreview, LegacyCoreError> {
        let stat = self.vfs.stat(uri)?;
        let length = stat.size.min(MAX_PREVIEW_BYTES);
        let bytes = self.vfs.read_range(uri, 0, length)?.bytes;
        text_preview(&bytes, encoding, stat.size > length)
    }

    pub fn preview_hex(&self, uri: &str) -> Result<ViewerPreview, LegacyCoreError> {
        let stat = self.vfs.stat(uri)?;
        let length = stat.size.min(MAX_VIEWER_PAGE_BYTES);
        let bytes = self.vfs.read_range(uri, 0, length)?.bytes;
        Ok(ViewerPreview::Hex {
            text: hex_page(&bytes),
            truncated: stat.size > length,
        })
    }

    pub fn preview_media(
        &self,
        uri: &str,
        registry: &DecodeProviderRegistry,
        binding: &DecodeBindingContext,
    ) -> Result<ViewerPreview, LegacyCoreError> {
        let entry = self
            .vfs
            .manifest()
            .entries
            .iter()
            .find(|entry| entry.uri == uri)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_VIEWER_ENTRY",
                    "preview URI is not a manifest entry",
                )
            })?;
        let kind = match entry.media_kind.as_str() {
            "image" => DecodeKind::Image,
            "audio" => DecodeKind::Audio,
            "video" => DecodeKind::Video,
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_VFS_VIEWER_MEDIA_KIND",
                    "entry is not declared as image, audio, or video",
                ))
            }
        };
        if entry.decoded_size > MAX_PREVIEW_BYTES {
            return Err(invalid(
                "ASTRA_EMU_VFS_VIEWER_MEDIA_LIMIT",
                "media preview exceeds the input byte budget",
            ));
        }
        let codec = uri
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_VIEWER_CODEC",
                    "media URI has no codec extension",
                )
            })?;
        let bytes = self.vfs.read_range(uri, 0, entry.decoded_size)?.bytes;
        let result = registry
            .decode(
                &DecodeRequest {
                    kind,
                    codec: codec.clone(),
                    bytes,
                    profile: binding.profile.clone(),
                },
                binding,
            )
            .map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_VIEWER_DECODE",
                    "bound media decode provider rejected the preview",
                )
            })?;
        Ok(ViewerPreview::Media {
            provider_id: result.provider_id,
            kind,
            codec,
            output: result.output,
        })
    }
}

fn text_preview(
    bytes: &[u8],
    label: &str,
    truncated: bool,
) -> Result<ViewerPreview, LegacyCoreError> {
    let encoding = match label.to_ascii_lowercase().as_str() {
        "utf-8" => UTF_8,
        "cp932" | "shift_jis" => SHIFT_JIS,
        "utf-16le" => UTF_16LE,
        "utf-16be" => UTF_16BE,
        _ => Encoding::for_label(label.as_bytes()).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_VIEWER_ENCODING",
                "requested text encoding is unknown",
            )
        })?,
    };
    let (text, _, malformed) = encoding.decode(bytes);
    if malformed {
        return Err(invalid(
            "ASTRA_EMU_VFS_VIEWER_TEXT_DECODE",
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

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use astra_media::{DecodeBindingContext, DecodeProviderRegistry};

    use crate::test_support::MemoryVfs;

    use super::{LegacyVfsViewer, ViewerPreview};

    #[test]
    fn bounded_text_hex_page_and_search_use_the_vfs() {
        let viewer = LegacyVfsViewer::new(Arc::new(MemoryVfs::new(&[
            ("test:/scr/route.sc", "hello".as_bytes(), "script"),
            ("test:/sys/icon.png", b"not-an-image", "image"),
        ])));
        assert_eq!(
            viewer.page("test:/scr/route.sc", 1, 3).unwrap().bytes,
            b"ell"
        );
        assert_eq!(viewer.search("test:/", "route", 1).unwrap().len(), 1);
        assert!(matches!(
            viewer.preview_text("test:/scr/route.sc", "utf-8").unwrap(),
            ViewerPreview::Text { ref text, .. } if text == "hello"
        ));
        assert!(matches!(
            viewer.preview_hex("test:/scr/route.sc").unwrap(),
            ViewerPreview::Hex { ref text, .. } if text.contains("68 65 6c 6c 6f")
        ));
        assert_eq!(
            viewer.page("test:/scr/route.sc", 0, 0).unwrap_err().code(),
            "ASTRA_EMU_VFS_VIEWER_PAGE_LIMIT"
        );
    }

    #[test]
    fn media_preview_has_no_unbound_fallback() {
        let viewer = LegacyVfsViewer::new(Arc::new(MemoryVfs::new(&[(
            "test:/sys/icon.png",
            b"not-an-image",
            "image",
        )])));
        let registry = DecodeProviderRegistry::default();
        let binding = DecodeBindingContext::shipping("missing", "windows", "test");
        assert_eq!(
            viewer
                .preview_media("test:/sys/icon.png", &registry, &binding)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_VIEWER_DECODE"
        );
    }
}
