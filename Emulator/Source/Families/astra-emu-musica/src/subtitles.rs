#[cfg(feature = "ffmpeg-vcpkg")]
use crate::resources::read_resource;
use crate::CoreError;
use astra_core::Hash256;
use std::{path::Path, sync::Arc};

pub(crate) struct SubtitleCatalog {
    identity: Hash256,
    #[cfg(feature = "ffmpeg-vcpkg")]
    tracks: std::collections::BTreeMap<String, Track>,
}
#[cfg(feature = "ffmpeg-vcpkg")]
struct Track {
    ass: Vec<u8>,
    fonts: Vec<astra_emu_sdk::subtitle::FontData>,
}
impl SubtitleCatalog {
    pub(crate) fn identity(&self) -> Hash256 {
        self.identity
    }

    pub(crate) fn load(root: &Path, relative: &str) -> Result<Option<Arc<Self>>, CoreError> {
        if relative.is_empty() {
            return Ok(None);
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        {
            let _ = root;
            Err(CoreError::invalid(
                "ASTRA_EMU_MUSICA_SUBTITLE_UNAVAILABLE",
                "movie subtitles require ffmpeg-vcpkg",
            ))
        }
        #[cfg(feature = "ffmpeg-vcpkg")]
        {
            use astra_emu_sdk::subtitle::FontData;
            use serde::Deserialize;
            use std::collections::BTreeMap;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Document {
                version: u32,
                tracks: Vec<Entry>,
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Entry {
                clock: String,
                resource: String,
                ass: String,
                fonts: Vec<String>,
            }
            let bytes = read_resource(root, relative, 1024 * 1024)?;
            let document: Document = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if document.version != 1 || document.tracks.len() > 256 {
                return Err(invalid());
            }
            let mut identity = bytes;
            let mut tracks = BTreeMap::new();
            let mut fonts: BTreeMap<String, (FontData, Hash256)> = BTreeMap::new();
            let mut resource_bytes = 0usize;
            for entry in document.tracks {
                // Only the actual movie clock has a native consumer. BGM mappings are rejected.
                if entry.clock != "movie"
                    || entry.resource.is_empty()
                    || entry.resource.len() > 256
                    || entry.resource.contains(['/', '\\', ':', '\0'])
                    || matches!(entry.resource.as_str(), "." | "..")
                    || entry.fonts.is_empty()
                    || entry.fonts.len() > 16
                {
                    return Err(invalid());
                }
                let key = entry.resource.to_ascii_lowercase();
                if tracks.contains_key(&key) {
                    return Err(invalid());
                }
                let ass = read_resource(root, &entry.ass, 16 * 1024 * 1024)?;
                resource_bytes = resource_bytes
                    .checked_add(ass.len())
                    .filter(|size| *size <= 128 * 1024 * 1024)
                    .ok_or_else(invalid)?;
                if std::str::from_utf8(&ass).is_err() || ass.contains(&0) {
                    return Err(invalid());
                }
                identity.extend_from_slice(Hash256::from_sha256(&ass).as_bytes());
                let mut track_fonts = Vec::with_capacity(entry.fonts.len());
                for path in entry.fonts {
                    if !fonts.contains_key(&path) {
                        let bytes = read_resource(root, &path, 64 * 1024 * 1024)?;
                        resource_bytes = resource_bytes
                            .checked_add(bytes.len())
                            .filter(|size| *size <= 128 * 1024 * 1024)
                            .ok_or_else(invalid)?;
                        astra_text::font_unicode_coverage(&bytes, 0).map_err(|_| invalid())?;
                        let hash = Hash256::from_sha256(&bytes);
                        fonts.insert(
                            path.clone(),
                            (
                                FontData {
                                    name: path.clone(),
                                    bytes: Arc::from(bytes),
                                },
                                hash,
                            ),
                        );
                    }
                    let (font, hash) = &fonts[&path];
                    identity.extend_from_slice(hash.as_bytes());
                    track_fonts.push(font.clone());
                }
                tracks.insert(
                    key,
                    Track {
                        ass,
                        fonts: track_fonts,
                    },
                );
            }
            Ok(Some(Arc::new(Self {
                identity: Hash256::from_sha256(&identity),
                tracks,
            })))
        }
    }

    #[cfg(feature = "ffmpeg-vcpkg")]
    pub(crate) fn open_movie(
        &self,
        uri: &str,
        serial: u64,
    ) -> Result<Option<astra_emu_sdk::AssScene>, CoreError> {
        let Some(resource) = uri.strip_prefix("musica:/mov/") else {
            return Err(invalid());
        };
        self.tracks
            .get(&resource.to_ascii_lowercase())
            .map(|track| {
                astra_emu_sdk::AssScene::new(
                    format!("musica.ass.{serial}"),
                    &track.ass,
                    &track.fonts,
                )
            })
            .transpose()
    }
}
#[cfg(feature = "ffmpeg-vcpkg")]
fn invalid() -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_MUSICA_SUBTITLE_MAP",
        "subtitle map must contain version 1 movie tracks with UTF-8 ASS and actual private fonts",
    )
}

#[cfg(feature = "ffmpeg-vcpkg")]
pub(crate) fn video_color(
    color: astra_emu_sdk::video::VideoColorMetadata,
) -> Result<astra_emu_sdk::subtitle::VideoColorSpace, CoreError> {
    use astra_emu_sdk::{
        subtitle::VideoColorSpace as S,
        video::{VideoColorMatrix as M, VideoColorRange as R, VideoColorTransfer as T},
    };
    if color.transfer != T::Sdr {
        return Err(CoreError::invalid(
            "ASTRA_EMU_MUSICA_SUBTITLE_COLOR",
            "subtitle color requires actual SDR video metadata",
        ));
    }
    Ok(match (color.matrix, color.range) {
        (M::Rgb, _) => S::Rgb,
        (M::Bt601, R::Limited) => S::Bt601Tv,
        (M::Bt601, R::Full) => S::Bt601Pc,
        (M::Bt709, R::Limited) => S::Bt709Tv,
        (M::Bt709, R::Full) => S::Bt709Pc,
        (M::Smpte240m, R::Limited) => S::Smpte240mTv,
        (M::Smpte240m, R::Full) => S::Smpte240mPc,
        (M::Fcc, R::Limited) => S::FccTv,
        (M::Fcc, R::Full) => S::FccPc,
        _ => {
            return Err(CoreError::invalid(
                "ASTRA_EMU_MUSICA_SUBTITLE_COLOR",
                "video matrix or range is absent or unsupported",
            ))
        }
    })
}

#[cfg(all(test, feature = "ffmpeg-vcpkg"))]
mod tests {
    use super::*;
    use astra_emu_sdk::video::{
        VideoColorMatrix as M, VideoColorMetadata, VideoColorRange as R, VideoColorTransfer as T,
    };
    #[test]
    fn color_mapping_requires_actual_supported_sdr_metadata() {
        use astra_emu_sdk::subtitle::VideoColorSpace as S;
        let color = VideoColorMetadata {
            matrix: M::Bt709,
            range: R::Limited,
            transfer: T::Sdr,
        };
        assert_eq!(video_color(color).unwrap(), S::Bt709Tv);
        assert_eq!(
            video_color(VideoColorMetadata {
                matrix: M::Rgb,
                ..color
            })
            .unwrap(),
            S::Rgb
        );
        for color in [
            VideoColorMetadata::default(),
            VideoColorMetadata {
                transfer: T::Hdr,
                ..color
            },
            VideoColorMetadata {
                range: R::Unknown,
                ..color
            },
            VideoColorMetadata {
                matrix: M::Unsupported,
                ..color
            },
        ] {
            assert_eq!(
                video_color(color).unwrap_err().code(),
                "ASTRA_EMU_MUSICA_SUBTITLE_COLOR"
            );
        }
    }
    #[test]
    fn catalog_rejects_missing_paths_bgm_clocks_and_case_collisions_and_binds_actual_assets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("font.ttf"),
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf"),
        )
        .unwrap();
        std::fs::write(
            root.path().join("public.ass"),
            b"[Script Info]\nScriptType: v4.00+\n",
        )
        .unwrap();
        let track = |clock: &str, resource: &str, font: &str| {
            serde_json::json!({
                "clock":clock, "resource":resource, "ass":"public.ass", "fonts":[font],
            })
        };
        let publish = |tracks| {
            std::fs::write(
                root.path().join("map.json"),
                serde_json::to_vec(&serde_json::json!({"version":1,"tracks":tracks})).unwrap(),
            )
            .unwrap()
        };
        publish(vec![track("movie", "op.avi", "font.ttf")]);
        let first = SubtitleCatalog::load(root.path(), "map.json")
            .unwrap()
            .unwrap();
        assert!(first.tracks.contains_key("op.avi"));
        std::fs::write(
            root.path().join("public.ass"),
            b"[Script Info]\nScriptType: v4.00+\n; changed\n",
        )
        .unwrap();
        assert_ne!(
            first.identity(),
            SubtitleCatalog::load(root.path(), "map.json")
                .unwrap()
                .unwrap()
                .identity()
        );
        for tracks in [
            vec![track("bgm", "op.avi", "font.ttf")],
            vec![track("movie", "../op.avi", "font.ttf")],
            vec![track("movie", "op.avi", "../font.ttf")],
            vec![track("movie", "op.avi", "missing.ttf")],
            vec![
                track("movie", "op.avi", "font.ttf"),
                track("movie", "OP.AVI", "font.ttf"),
            ],
        ] {
            publish(tracks);
            assert!(SubtitleCatalog::load(root.path(), "map.json").is_err());
        }
    }
}
