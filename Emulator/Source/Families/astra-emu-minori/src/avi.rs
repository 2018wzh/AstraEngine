//! Minori AVI integration for the shared AstraMedia incremental decoder.
//!
//! Minori does not own a container parser or codec implementation. The family
//! adapter only validates the RIFF/AVI identity and binds the explicit FFmpeg
//! provider. Packet validation, timestamp scheduling, and PCM conversion stay
//! in AstraMedia.

use std::{
    io::{Read, Seek, SeekFrom},
    marker::PhantomData,
};

use astra_media::{
    DecodeCapability, DecodeKind, DecodeProvider, DecodeRequest, DecodeResult, DecodedMediaPacket,
    IncrementalMediaDecoder, MediaError, MediaPlaybackConfig,
};

#[cfg(feature = "ffmpeg-vcpkg")]
use astra_media::{
    DecodeOutput, FfmpegDecodeProvider, FfmpegIncrementalDecodeProvider, IncrementalDecodeBudget,
    IncrementalDecodeProviderRegistry, IncrementalDecodeRequest,
};

pub const MINORI_AVI_DECODE_PROVIDER_ID: &str = "astra.decode.minori.avi";
pub const MINORI_AVI_STREAM_PROVIDER_ID: &str = "astra.decode.ffmpeg.incremental";

const MAX_PREVIEW_INPUT_BYTES: usize = 512 * 1024 * 1024;
#[cfg(any(feature = "ffmpeg-vcpkg", test))]
const MAX_PREVIEW_FRAME_BYTES: usize = 64 * 1024 * 1024;
#[cfg(any(feature = "ffmpeg-vcpkg", test))]
const MAX_VIDEO_DIMENSION: u32 = 16_384;

#[cfg(feature = "ffmpeg-vcpkg")]
enum MinoriAviBackend {
    Ffmpeg(Box<dyn IncrementalMediaDecoder>),
}

/// Bounded family-owned handle around AstraMedia's incremental decoder.
///
/// `R` remains in the public type so bounded VFS readers can be passed without
/// exposing paths or unbounded byte sources. The reader is consumed into
/// AstraMedia's private, bounded FFmpeg spool during construction.
pub struct MinoriAviDecoder<R> {
    #[cfg(feature = "ffmpeg-vcpkg")]
    backend: MinoriAviBackend,
    _reader: PhantomData<fn() -> R>,
}

/// Explicit first-frame binding for Minori AVI previews.
///
/// This provider is an AstraMedia FFmpeg binding, not a second decoder. It is
/// eligible only when the `ffmpeg-vcpkg` feature is compiled into the selected
/// host; there is deliberately no native or handwritten fallback.
#[derive(Debug, Clone, Default)]
pub struct MinoriAviDecodeProvider;

impl MinoriAviDecodeProvider {
    pub fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: MINORI_AVI_DECODE_PROVIDER_ID.to_owned(),
            priority: astra_media::ProviderPriority::Platform,
            kinds: vec![DecodeKind::Video],
            codecs: vec!["avi".to_owned()],
            feature_gated: !astra_media::ffmpeg_compiled(),
            packaged_eligible: true,
            reference_only: false,
        }
    }
}

impl DecodeProvider for MinoriAviDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        MinoriAviDecodeProvider::capability(self)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        if request.kind != DecodeKind::Video {
            return Err(avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_KIND"));
        }
        if !request.codec.eq_ignore_ascii_case("avi") {
            return Err(avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_CODEC"));
        }
        validate_preview_input_len(request.bytes.len())?;
        if !is_avi_container_header(&request.bytes) {
            return Err(avi_preview_diagnostic(
                "ASTRA_EMU_MINORI_AVI_PREVIEW_HEADER",
            ));
        }
        if !astra_media::ffmpeg_compiled() {
            return Err(avi_preview_diagnostic(
                "ASTRA_EMU_MINORI_AVI_FFMPEG_UNAVAILABLE",
            ));
        }
        #[cfg(feature = "ffmpeg-vcpkg")]
        {
            let provider = FfmpegDecodeProvider::probe()
                .map_err(|error| avi_media_error("ASTRA_EMU_MINORI_AVI_FFMPEG_PROBE", error))?;
            let result = provider
                .decode(request)
                .map_err(|error| avi_media_error("ASTRA_EMU_MINORI_AVI_PREVIEW_DECODE", error))?;
            let DecodeOutput::CpuBuffer { bytes, format } = result.output else {
                return Err(avi_preview_diagnostic(
                    "ASTRA_EMU_MINORI_AVI_PREVIEW_OUTPUT",
                ));
            };
            let (width, height) = parse_frame_format(&format)?;
            let expected_bytes = usize::try_from(width)
                .ok()
                .and_then(|width| {
                    usize::try_from(height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_FRAME"))?;
            if bytes.len() != expected_bytes || bytes.len() > MAX_PREVIEW_FRAME_BYTES {
                return Err(avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_FRAME"));
            }
            let mut rgba = bytes.as_slice().to_vec();
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            Ok(DecodeResult {
                provider_id: MINORI_AVI_DECODE_PROVIDER_ID.to_owned(),
                kind: request.kind,
                codec: request.codec.to_ascii_lowercase(),
                output: DecodeOutput::CpuBuffer {
                    bytes: rgba.into(),
                    format: format!("rgba8:first_frame:{width}x{height}"),
                },
                diagnostics: Vec::new(),
            })
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        unreachable!("ffmpeg_compiled is false without the feature")
    }
}

fn avi_preview_diagnostic(code: &'static str) -> MediaError {
    MediaError::Diagnostics(vec![astra_core::Diagnostic::blocking(
        code,
        "Minori AVI requires the explicitly bound AstraMedia FFmpeg provider",
    )])
}

fn validate_preview_input_len(len: usize) -> Result<(), MediaError> {
    if len == 0 || len > MAX_PREVIEW_INPUT_BYTES {
        return Err(avi_preview_diagnostic(
            "ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT",
        ));
    }
    Ok(())
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn parse_frame_format(format: &str) -> Result<(u32, u32), MediaError> {
    let dimensions = format
        .strip_prefix("bgra8:first_frame:")
        .ok_or_else(|| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_OUTPUT"))?;
    let (width, height) = dimensions
        .split_once('x')
        .ok_or_else(|| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_OUTPUT"))?;
    let width = width
        .parse::<u32>()
        .map_err(|_| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_OUTPUT"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_OUTPUT"))?;
    validate_video_dimensions(width, height).map_err(avi_preview_diagnostic)?;
    Ok((width, height))
}

#[cfg(any(feature = "ffmpeg-vcpkg", test))]
fn validate_video_dimensions(width: u32, height: u32) -> Result<(), &'static str> {
    if width == 0 || height == 0 || width > MAX_VIDEO_DIMENSION || height > MAX_VIDEO_DIMENSION {
        return Err("ASTRA_EMU_MINORI_AVI_VIDEO_DIMENSIONS");
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS")?;
    if pixels > MAX_PREVIEW_FRAME_BYTES as u64 {
        return Err("ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS");
    }
    Ok(())
}

impl<R: Read + Seek + 'static> MinoriAviDecoder<R> {
    pub fn new(mut reader: R) -> Result<Self, String> {
        let mut header = [0_u8; 12];
        reader
            .read_exact(&mut header)
            .map_err(|_| "ASTRA_EMU_MINORI_AVI_HEADER".to_owned())?;
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| "ASTRA_EMU_MINORI_AVI_READER".to_owned())?;
        if !is_avi_container_header(&header) {
            return Err("ASTRA_EMU_MINORI_AVI_HEADER".to_owned());
        }

        #[cfg(feature = "ffmpeg-vcpkg")]
        {
            let mut registry = IncrementalDecodeProviderRegistry::default();
            registry
                .register(Box::new(FfmpegIncrementalDecodeProvider::probe().map_err(
                    |error| minori_avi_error("ASTRA_EMU_MINORI_AVI_FFMPEG_PROBE", error),
                )?))
                .map_err(|error| minori_avi_error("ASTRA_EMU_MINORI_AVI_FFMPEG_PROVIDER", error))?;
            let decoder = registry
                .open(
                    MINORI_AVI_STREAM_PROVIDER_ID,
                    IncrementalDecodeRequest::new("avi", Box::new(reader)).with_budget(
                        IncrementalDecodeBudget {
                            max_encoded_bytes: MAX_PREVIEW_INPUT_BYTES,
                            max_video_frame_bytes: MAX_PREVIEW_FRAME_BYTES,
                            max_pending_packets: 64,
                            max_video_frames: 64,
                            max_audio_packets: 64,
                        },
                    ),
                )
                .map_err(|error| minori_avi_error("ASTRA_EMU_MINORI_AVI_FFMPEG_OPEN", error))?;
            Ok(Self {
                backend: MinoriAviBackend::Ffmpeg(decoder),
                _reader: PhantomData,
            })
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        {
            let _ = reader;
            Err("ASTRA_EMU_MINORI_AVI_FFMPEG_UNAVAILABLE".to_owned())
        }
    }
}

impl<R> IncrementalMediaDecoder for MinoriAviDecoder<R> {
    fn provider_id(&self) -> &'static str {
        MINORI_AVI_STREAM_PROVIDER_ID
    }

    fn playback_config(&self) -> MediaPlaybackConfig {
        #[cfg(feature = "ffmpeg-vcpkg")]
        match &self.backend {
            MinoriAviBackend::Ffmpeg(decoder) => decoder.playback_config(),
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        {
            MediaPlaybackConfig::default()
        }
    }

    fn read_next(&mut self) -> Result<Option<DecodedMediaPacket>, MediaError> {
        #[cfg(feature = "ffmpeg-vcpkg")]
        {
            match &mut self.backend {
                MinoriAviBackend::Ffmpeg(decoder) => decoder.read_next(),
            }
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        Err(MediaError::message(
            "ASTRA_EMU_MINORI_AVI_FFMPEG_UNAVAILABLE",
        ))
    }

    fn seek(&mut self, position_us: u64) -> Result<u64, MediaError> {
        #[cfg(feature = "ffmpeg-vcpkg")]
        match &mut self.backend {
            MinoriAviBackend::Ffmpeg(decoder) => decoder.seek(position_us),
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        {
            let _ = position_us;
            Err(MediaError::message(
                "ASTRA_EMU_MINORI_AVI_FFMPEG_UNAVAILABLE",
            ))
        }
    }

    fn cancel(&mut self) -> Result<(), MediaError> {
        #[cfg(feature = "ffmpeg-vcpkg")]
        match &mut self.backend {
            MinoriAviBackend::Ffmpeg(decoder) => decoder.cancel(),
        }
        #[cfg(not(feature = "ffmpeg-vcpkg"))]
        Err(MediaError::message(
            "ASTRA_EMU_MINORI_AVI_FFMPEG_UNAVAILABLE",
        ))
    }
}

fn is_avi_container_header(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"AVI "
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn minori_avi_error(prefix: &'static str, error: MediaError) -> String {
    let codes = match error {
        MediaError::Diagnostics(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(","),
        MediaError::Message(_) => String::new(),
    };
    if codes.is_empty() {
        tracing::error!(
            event = "astra_emu_minori_avi_provider_error",
            diagnostic_prefix = prefix,
            "Minori AVI provider failed without a structured diagnostic code"
        );
        prefix.to_owned()
    } else {
        tracing::error!(
            event = "astra_emu_minori_avi_provider_error",
            diagnostic_prefix = prefix,
            diagnostic_codes = %codes,
            "Minori AVI provider returned a structured diagnostic"
        );
        format!("{prefix}:{codes}")
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn avi_media_error(prefix: &'static str, error: MediaError) -> MediaError {
    let diagnostic = minori_avi_error(prefix, error);
    MediaError::Diagnostics(vec![astra_core::Diagnostic::blocking(
        "ASTRA_EMU_MINORI_AVI_PROVIDER",
        diagnostic,
    )])
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use astra_media::{DecodeKind, DecodeProvider, DecodeRequest, MediaError};

    use super::{
        is_avi_container_header, validate_preview_input_len, validate_video_dimensions,
        MinoriAviDecodeProvider, MinoriAviDecoder, MAX_PREVIEW_INPUT_BYTES,
        MINORI_AVI_DECODE_PROVIDER_ID,
    };

    #[test]
    fn container_identity_is_checked_before_provider_open() {
        let result = MinoriAviDecoder::new(Cursor::new(b"not an AVI".to_vec()));
        assert!(matches!(
            result,
            Err(error) if error == "ASTRA_EMU_MINORI_AVI_HEADER"
        ));
        assert!(is_avi_container_header(b"RIFF\x10\0\0\0AVI "));
        assert!(!is_avi_container_header(b"RIFF\x10\0\0\0WAVE"));
    }

    #[test]
    fn preview_provider_is_explicit_and_bounded() {
        let provider = MinoriAviDecodeProvider;
        let capability = provider.capability();
        assert_eq!(capability.provider_id, MINORI_AVI_DECODE_PROVIDER_ID);
        assert_eq!(capability.kinds, vec![DecodeKind::Video]);
        assert_eq!(capability.codecs, vec!["avi"]);
        let error = provider
            .decode(&DecodeRequest {
                kind: DecodeKind::Video,
                codec: "avi".into(),
                bytes: Vec::<u8>::new().into(),
                profile: "astra.manager.preview.v1".into(),
            })
            .expect_err("empty movie must not produce a preview");
        let MediaError::Diagnostics(diagnostics) = error else {
            panic!("preview failure must remain a blocking diagnostic");
        };
        assert_eq!(
            diagnostics[0].code,
            "ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT"
        );
    }

    #[test]
    fn preview_input_budget_is_checked_before_container_parse() {
        assert!(validate_preview_input_len(1).is_ok());
        let error = validate_preview_input_len(MAX_PREVIEW_INPUT_BYTES + 1).unwrap_err();
        let MediaError::Diagnostics(diagnostics) = error else {
            panic!("preview failure must remain a blocking diagnostic");
        };
        assert_eq!(
            diagnostics[0].code,
            "ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT"
        );
    }

    #[test]
    fn video_dimension_budget_rejects_invalid_frames() {
        assert!(validate_video_dimensions(320, 180).is_ok());
        assert_eq!(
            validate_video_dimensions(0, 180).unwrap_err(),
            "ASTRA_EMU_MINORI_AVI_VIDEO_DIMENSIONS"
        );
        assert_eq!(
            validate_video_dimensions(16_384, 16_384).unwrap_err(),
            "ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS"
        );
    }
}
