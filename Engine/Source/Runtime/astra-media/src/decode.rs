use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, ErrorKind},
};

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use symphonia::core::{
    codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO},
    errors::Error as SymphoniaError,
    formats::{probe::Hint, FormatOptions, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

use crate::MediaError;

const MAX_DECODED_AUDIO_BYTES: usize = 16 * 1024 * 1024;
#[cfg(any(windows, feature = "ffmpeg-vcpkg"))]
const MAX_DECODED_VIDEO_FRAME_BYTES: usize = 64 * 1024 * 1024;

#[cfg(feature = "ffmpeg-vcpkg")]
mod ffmpeg;
#[cfg(feature = "ffmpeg-vcpkg")]
mod ffmpeg_stream;

#[cfg(feature = "ffmpeg-vcpkg")]
pub use ffmpeg_stream::*;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DecodeKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeRequest {
    pub kind: DecodeKind,
    pub codec: String,
    pub bytes: Vec<u8>,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeResult {
    pub provider_id: String,
    pub kind: DecodeKind,
    pub codec: String,
    pub output: DecodeOutput,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecodeOutput {
    CpuBuffer {
        bytes: Vec<u8>,
        format: String,
        hash: Hash256,
    },
    MediaSurfaceToken(MediaSurfaceToken),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MediaSurfaceToken {
    pub provider_id: String,
    pub token_id: String,
    pub format: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPriority {
    Platform,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeCapability {
    pub provider_id: String,
    pub priority: ProviderPriority,
    pub kinds: Vec<DecodeKind>,
    pub codecs: Vec<String>,
    pub feature_gated: bool,
    pub packaged_eligible: bool,
    pub reference_only: bool,
}

pub trait DecodeProvider: Send + Sync {
    fn capability(&self) -> DecodeCapability;
    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError>;
}

pub const fn ffmpeg_compiled() -> bool {
    cfg!(feature = "ffmpeg-vcpkg")
}

pub fn probe_ffmpeg_provider() -> Result<DecodeCapability, MediaError> {
    #[cfg(feature = "ffmpeg-vcpkg")]
    {
        FfmpegDecodeProvider::probe().map(|provider| provider.capability())
    }
    #[cfg(not(feature = "ffmpeg-vcpkg"))]
    {
        Err(decode_error(
            "ASTRA_FFMPEG_FEATURE_DISABLED",
            "this build does not include the ffmpeg-vcpkg provider",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeBindingContext {
    pub provider_id: String,
    pub target: String,
    pub profile: String,
    pub allow_fallback: bool,
    pub allow_reference_provider: bool,
}

impl DecodeBindingContext {
    pub fn shipping(
        provider_id: impl Into<String>,
        target: impl Into<String>,
        profile: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            target: target.into(),
            profile: profile.into(),
            allow_fallback: false,
            allow_reference_provider: false,
        }
    }

    pub fn with_declared_fallback(mut self) -> Self {
        self.allow_fallback = true;
        self
    }

    pub fn for_reference_tests(mut self) -> Self {
        self.allow_reference_provider = true;
        self
    }
}

#[derive(Default)]
pub struct DecodeProviderRegistry {
    providers: BTreeMap<String, Box<dyn DecodeProvider>>,
}

impl DecodeProviderRegistry {
    pub fn register(&mut self, provider: Box<dyn DecodeProvider>) -> Result<(), MediaError> {
        let capability = provider.capability();
        validate_capability(&capability)?;
        if self.providers.contains_key(&capability.provider_id) {
            return Err(decode_error(
                "ASTRA_DECODE_PROVIDER_DUPLICATE",
                "decode provider id is already registered",
            ));
        }
        tracing::info!(
            event = "media.decode.provider.register",
            provider_id = %capability.provider_id,
            codec_count = capability.codecs.len(),
            "media decode provider registered"
        );
        self.providers
            .insert(capability.provider_id.clone(), provider);
        Ok(())
    }

    pub fn select(
        &self,
        request: &DecodeRequest,
        binding: &DecodeBindingContext,
    ) -> Result<DecodeCapability, MediaError> {
        validate_request(request, binding)?;
        tracing::debug!(
            event = "media.decode.select.start",
            codec = %request.codec,
            profile = %binding.profile,
            target_id = %binding.target,
            provider_id = %binding.provider_id,
            provider_count = self.providers.len(),
            "media decode provider selection started"
        );
        let provider = self.providers.get(&binding.provider_id).ok_or_else(|| {
            decode_error(
                "ASTRA_DECODE_BINDING_MISSING",
                "bound decode provider is not registered",
            )
        })?;
        let capability = provider.capability();
        validate_capability(&capability)?;
        if capability.feature_gated
            || !capability.kinds.contains(&request.kind)
            || !capability
                .codecs
                .iter()
                .any(|codec| codec == &request.codec)
            || (!capability.packaged_eligible && !binding.allow_reference_provider)
            || (capability.priority == ProviderPriority::Fallback && !binding.allow_fallback)
            || (capability.reference_only && !binding.allow_reference_provider)
        {
            return Err(decode_error(
                "ASTRA_DECODE_BINDING_INELIGIBLE",
                "bound decode provider is not eligible for the request and binding context",
            ));
        }
        if capability.priority == ProviderPriority::Fallback {
            tracing::warn!(
                event = "media.decode.select.fallback",
                provider_id = %capability.provider_id,
                codec = %request.codec,
                "explicitly declared media decode fallback selected"
            );
        } else {
            tracing::info!(
                event = "media.decode.select.complete",
                provider_id = %capability.provider_id,
                codec = %request.codec,
                "bound media decode provider selected"
            );
        }
        Ok(capability)
    }

    pub fn decode(
        &self,
        request: &DecodeRequest,
        binding: &DecodeBindingContext,
    ) -> Result<DecodeResult, MediaError> {
        let capability = self.select(request, binding)?;
        let provider = self.providers.get(&capability.provider_id).ok_or_else(|| {
            decode_error(
                "ASTRA_DECODE_REGISTRY_STATE",
                "selected decode provider disappeared from the registry",
            )
        })?;
        let result = provider.decode(request)?;
        if result.provider_id != capability.provider_id
            || result.kind != request.kind
            || result.codec != request.codec
        {
            return Err(decode_error(
                "ASTRA_DECODE_OUTPUT_IDENTITY",
                "decode provider output identity does not match the bound request",
            ));
        }
        validate_output(&result.output)?;
        Ok(result)
    }
}

fn validate_capability(capability: &DecodeCapability) -> Result<(), MediaError> {
    let kinds = capability.kinds.iter().copied().collect::<BTreeSet<_>>();
    let codecs = capability.codecs.iter().collect::<BTreeSet<_>>();
    if !safe_identity(&capability.provider_id)
        || capability.kinds.is_empty()
        || kinds.len() != capability.kinds.len()
        || capability.codecs.is_empty()
        || codecs.len() != capability.codecs.len()
        || capability.codecs.iter().any(|codec| !safe_codec(codec))
        || (capability.reference_only && capability.packaged_eligible)
    {
        return Err(decode_error(
            "ASTRA_DECODE_CAPABILITY_INVALID",
            "decode provider capability descriptor is invalid",
        ));
    }
    Ok(())
}

fn validate_request(
    request: &DecodeRequest,
    binding: &DecodeBindingContext,
) -> Result<(), MediaError> {
    if request.bytes.is_empty()
        || !safe_codec(&request.codec)
        || !safe_identity(&request.profile)
        || !safe_identity(&binding.provider_id)
        || !safe_identity(&binding.target)
        || !safe_identity(&binding.profile)
        || request.profile != binding.profile
    {
        return Err(decode_error(
            "ASTRA_DECODE_REQUEST_INVALID",
            "decode request or binding context is invalid",
        ));
    }
    Ok(())
}

fn validate_output(output: &DecodeOutput) -> Result<(), MediaError> {
    match output {
        DecodeOutput::CpuBuffer {
            bytes,
            format,
            hash,
        } => {
            if bytes.is_empty() || format.is_empty() || Hash256::from_sha256(bytes) != *hash {
                return Err(decode_error(
                    "ASTRA_DECODE_OUTPUT_INVALID",
                    "decoded CPU buffer is empty or has an invalid hash",
                ));
            }
        }
        DecodeOutput::MediaSurfaceToken(token) => {
            if !safe_identity(&token.provider_id)
                || token.token_id.trim().is_empty()
                || token.format.trim().is_empty()
            {
                return Err(decode_error(
                    "ASTRA_DECODE_OUTPUT_INVALID",
                    "decoded media surface token is invalid",
                ));
            }
        }
    }
    Ok(())
}

fn safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_codec(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn decode_error(code: impl Into<String>, message: impl Into<String>) -> MediaError {
    MediaError::Diagnostics(vec![Diagnostic::blocking(code, message)])
}

#[derive(Debug, Clone, Default)]
pub struct ImageDecodeProvider;

impl DecodeProvider for ImageDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: "astra.decode.image".to_string(),
            priority: ProviderPriority::Fallback,
            kinds: vec![DecodeKind::Image],
            codecs: vec![
                "png".to_string(),
                "jpeg".to_string(),
                "jpg".to_string(),
                "webp".to_string(),
            ],
            feature_gated: false,
            packaged_eligible: true,
            reference_only: false,
        }
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        let image = image::load_from_memory(&request.bytes)
            .map_err(|err| MediaError::message(format!("decode image: {err}")))?;
        let rgba = image.to_rgba8().into_raw();
        Ok(DecodeResult {
            provider_id: self.capability().provider_id,
            kind: request.kind,
            codec: request.codec.clone(),
            output: DecodeOutput::CpuBuffer {
                hash: Hash256::from_sha256(&rgba),
                bytes: rgba,
                format: "rgba8".to_string(),
            },
            diagnostics: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SyntheticPlatformDecodeProvider {
    provider_id: String,
    codecs: Vec<String>,
}

impl SyntheticPlatformDecodeProvider {
    pub fn new(provider_id: impl Into<String>, codecs: Vec<&str>) -> Self {
        Self {
            provider_id: provider_id.into(),
            codecs: codecs.into_iter().map(str::to_string).collect(),
        }
    }
}

impl DecodeProvider for SyntheticPlatformDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: self.provider_id.clone(),
            priority: ProviderPriority::Platform,
            kinds: vec![DecodeKind::Image, DecodeKind::Audio, DecodeKind::Video],
            codecs: self.codecs.clone(),
            feature_gated: false,
            packaged_eligible: false,
            reference_only: true,
        }
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        Ok(DecodeResult {
            provider_id: self.provider_id.clone(),
            kind: request.kind,
            codec: request.codec.clone(),
            output: DecodeOutput::MediaSurfaceToken(MediaSurfaceToken {
                provider_id: self.provider_id.clone(),
                token_id: format!("surface:{}", Hash256::from_sha256(&request.bytes).to_hex()),
                format: request.codec.clone(),
            }),
            diagnostics: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct SymphoniaAudioDecodeProvider;

impl SymphoniaAudioDecodeProvider {
    pub fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: "astra.decode.symphonia".to_string(),
            priority: ProviderPriority::Fallback,
            kinds: vec![DecodeKind::Audio],
            codecs: vec![
                "wav".to_string(),
                "ogg".to_string(),
                "flac".to_string(),
                "mp3".to_string(),
            ],
            feature_gated: false,
            packaged_eligible: true,
            reference_only: false,
        }
    }

    pub fn probe_available(&self) -> bool {
        let _ = symphonia::default::get_codecs();
        true
    }
}

impl DecodeProvider for SymphoniaAudioDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        SymphoniaAudioDecodeProvider::capability(self)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        if request.kind != DecodeKind::Audio {
            return Err(MediaError::message(
                "symphonia decode provider only supports audio",
            ));
        }

        let mut hint = Hint::new();
        hint.with_extension(&request.codec);
        let media_source = Cursor::new(request.bytes.clone());
        let media_stream = MediaSourceStream::new(Box::new(media_source), Default::default());
        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                media_stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|err| MediaError::message(format!("probe audio container: {err}")))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| MediaError::message("audio container has no supported track"))?;
        let codec_params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or_else(|| MediaError::message("audio track is missing codec parameters"))?;
        if codec_params.codec == CODEC_ID_NULL_AUDIO {
            return Err(MediaError::message("audio track has no codec id"));
        }
        let track_id = track.id;
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
            .map_err(|err| MediaError::message(format!("create audio decoder: {err}")))?;
        let mut pcm = Vec::new();
        let mut sample_rate = codec_params.sample_rate.unwrap_or_default();
        let mut channels = codec_params
            .channels
            .as_ref()
            .map(|channels| channels.count())
            .unwrap_or_default();
        loop {
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => {
                    return Err(decode_error(
                        "ASTRA_AUDIO_DECODE_TRUNCATED_INPUT",
                        "audio container ended before an explicit end of stream",
                    ));
                }
                Err(err) => return Err(MediaError::message(format!("read audio packet: {err}"))),
            };
            if packet.track_id != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_)) => {
                    return Err(decode_error(
                        "ASTRA_AUDIO_DECODE_CORRUPT_PACKET",
                        "audio decoder rejected a media packet",
                    ));
                }
                Err(err) => return Err(MediaError::message(format!("decode audio packet: {err}"))),
            };
            sample_rate = decoded.spec().rate();
            channels = decoded.spec().channels().count();
            let mut samples = vec![0i16; decoded.samples_interleaved()];
            decoded.copy_to_slice_interleaved(&mut samples);
            let additional = samples
                .len()
                .checked_mul(std::mem::size_of::<i16>())
                .ok_or_else(|| {
                    decode_error(
                        "ASTRA_AUDIO_DECODE_BUDGET",
                        "decoded audio byte count overflowed",
                    )
                })?;
            if pcm
                .len()
                .checked_add(additional)
                .is_none_or(|total| total > MAX_DECODED_AUDIO_BYTES)
            {
                return Err(decode_error(
                    "ASTRA_AUDIO_DECODE_BUDGET",
                    "decoded audio exceeds the bounded CPU buffer budget",
                ));
            }
            for sample in samples {
                pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }

        if pcm.is_empty() || sample_rate == 0 || channels == 0 {
            return Err(MediaError::message("audio decode produced no PCM samples"));
        }
        Ok(DecodeResult {
            provider_id: self.capability().provider_id,
            kind: request.kind,
            codec: request.codec.clone(),
            output: DecodeOutput::CpuBuffer {
                hash: Hash256::from_sha256(&pcm),
                bytes: pcm,
                format: format!("pcm_s16le:{sample_rate}:{channels}"),
            },
            diagnostics: Vec::new(),
        })
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Default)]
pub struct WindowsMediaFoundationDecodeProvider;

#[cfg(windows)]
impl WindowsMediaFoundationDecodeProvider {
    pub fn probe() -> Result<Self, MediaError> {
        let _session = wmf_decode::startup()?;
        Ok(Self)
    }

    pub fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: "astra.decode.wmf".to_string(),
            priority: ProviderPriority::Platform,
            kinds: vec![DecodeKind::Audio, DecodeKind::Video],
            codecs: vec![
                "wav".to_string(),
                "mp3".to_string(),
                "wma".to_string(),
                "aac".to_string(),
                "mp4".to_string(),
                "wmv".to_string(),
                "h264".to_string(),
            ],
            feature_gated: false,
            packaged_eligible: true,
            reference_only: false,
        }
    }
}

#[cfg(windows)]
impl DecodeProvider for WindowsMediaFoundationDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        WindowsMediaFoundationDecodeProvider::capability(self)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        match request.kind {
            DecodeKind::Audio => wmf_decode::decode_audio(self.capability().provider_id, request),
            DecodeKind::Video => wmf_decode::decode_video(self.capability().provider_id, request),
            DecodeKind::Image => Err(wmf_decode::blocking(
                "ASTRA_WMF_UNSUPPORTED_KIND",
                "Windows Media Foundation provider only supports audio and video decode",
            )),
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Default)]
pub struct WebCodecsDecodeProvider;

#[cfg(target_arch = "wasm32")]
impl WebCodecsDecodeProvider {
    pub fn probe() -> Result<Self, MediaError> {
        if webcodecs_available() {
            Ok(Self)
        } else {
            Err(MediaError::Diagnostics(vec![Diagnostic::blocking(
                "ASTRA_WEBCODECS_PROBE",
                "WebCodecs audio/video decoder APIs are unavailable",
            )]))
        }
    }

    pub fn probe_available(&self) -> bool {
        webcodecs_available()
    }

    pub fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: "astra.decode.webcodecs".to_string(),
            priority: ProviderPriority::Platform,
            kinds: vec![DecodeKind::Audio, DecodeKind::Video],
            codecs: vec![
                "mp4".to_string(),
                "webm".to_string(),
                "h264".to_string(),
                "vp8".to_string(),
                "vp9".to_string(),
                "aac".to_string(),
                "opus".to_string(),
            ],
            feature_gated: false,
            packaged_eligible: true,
            reference_only: false,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl DecodeProvider for WebCodecsDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        WebCodecsDecodeProvider::capability(self)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        if !matches!(request.kind, DecodeKind::Audio | DecodeKind::Video) {
            return Err(MediaError::Diagnostics(vec![Diagnostic::blocking(
                "ASTRA_WEBCODECS_UNSUPPORTED_KIND",
                "WebCodecs provider only supports audio and video decode",
            )]));
        }
        let capability = self.capability();
        if !capability
            .codecs
            .iter()
            .any(|codec| codec == &request.codec)
        {
            return Err(MediaError::Diagnostics(vec![Diagnostic::blocking(
                "ASTRA_WEBCODECS_UNSUPPORTED_CODEC",
                format!(
                    "WebCodecs provider does not support codec {}",
                    request.codec
                ),
            )]));
        }
        Ok(DecodeResult {
            provider_id: capability.provider_id.clone(),
            kind: request.kind,
            codec: request.codec.clone(),
            output: DecodeOutput::MediaSurfaceToken(MediaSurfaceToken {
                provider_id: capability.provider_id,
                token_id: format!(
                    "webcodecs:{}",
                    Hash256::from_sha256(&request.bytes).to_hex()
                ),
                format: request.codec.clone(),
            }),
            diagnostics: Vec::new(),
        })
    }
}

#[cfg(target_arch = "wasm32")]
fn webcodecs_available() -> bool {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;

    let global = js_sys::global();
    Reflect::has(&global, &JsValue::from_str("VideoDecoder")).unwrap_or(false)
        && Reflect::has(&global, &JsValue::from_str("AudioDecoder")).unwrap_or(false)
}

#[cfg(windows)]
mod wmf_decode {
    use std::{ptr, slice};

    use astra_core::{Diagnostic, Hash256};
    use windows::{
        core::{Error as WindowsError, Interface, HRESULT},
        Win32::{
            Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK},
            Media::MediaFoundation::{
                IMFAttributes, IMFMediaType, IMFSample, MFAudioFormat_PCM, MFCreateAttributes,
                MFCreateMFByteStreamOnStreamEx, MFCreateMediaType,
                MFCreateSourceReaderFromByteStream, MFMediaType_Audio, MFMediaType_Video,
                MFShutdown, MFStartup, MFVideoFormat_RGB32, MFSTARTUP_FULL,
                MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_FRAME_SIZE,
                MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
                MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
                MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
                MF_VERSION,
            },
            System::{
                Com::{
                    CoInitializeEx, CoUninitialize, StructuredStorage::CreateStreamOnHGlobal,
                    COINIT_MULTITHREADED,
                },
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            },
        },
    };

    use super::{
        DecodeKind, DecodeOutput, DecodeRequest, DecodeResult, MediaError, MAX_DECODED_AUDIO_BYTES,
        MAX_DECODED_VIDEO_FRAME_BYTES,
    };

    pub(super) fn startup() -> Result<WmfSession, MediaError> {
        WmfSession::new().map_err(|err| {
            blocking(
                "ASTRA_WMF_PROBE",
                format!("Media Foundation startup failed: {err}"),
            )
        })
    }

    pub(super) fn decode_audio(
        provider_id: String,
        request: &DecodeRequest,
    ) -> Result<DecodeResult, MediaError> {
        let _session = startup()?;
        let output = decode_audio_inner(&request.bytes).map_err(decode_error)?;
        Ok(DecodeResult {
            provider_id,
            kind: DecodeKind::Audio,
            codec: request.codec.clone(),
            output: DecodeOutput::CpuBuffer {
                hash: Hash256::from_sha256(&output.pcm),
                bytes: output.pcm,
                format: format!("pcm_s16le:{}:{}", output.sample_rate, output.channels),
            },
            diagnostics: output.diagnostics,
        })
    }

    pub(super) fn decode_video(
        provider_id: String,
        request: &DecodeRequest,
    ) -> Result<DecodeResult, MediaError> {
        let _session = startup()?;
        let output = decode_video_inner(&request.bytes).map_err(decode_error)?;
        Ok(DecodeResult {
            provider_id,
            kind: DecodeKind::Video,
            codec: request.codec.clone(),
            output: DecodeOutput::CpuBuffer {
                hash: Hash256::from_sha256(&output.bgra),
                bytes: output.bgra,
                format: format!("bgra8:first_frame:{}x{}", output.width, output.height),
            },
            diagnostics: Vec::new(),
        })
    }

    pub(super) fn blocking(code: &'static str, message: impl Into<String>) -> MediaError {
        MediaError::Diagnostics(vec![Diagnostic::blocking(code, message.into())])
    }

    fn decode_error(err: WindowsError) -> MediaError {
        blocking(
            "ASTRA_WMF_DECODE",
            format!("Media Foundation decode failed: {err}"),
        )
    }

    struct AudioOutput {
        pcm: Vec<u8>,
        sample_rate: u32,
        channels: u32,
        diagnostics: Vec<Diagnostic>,
    }

    struct VideoOutput {
        bgra: Vec<u8>,
        width: u32,
        height: u32,
    }

    pub(super) struct WmfSession {
        com_initialized: bool,
    }

    impl WmfSession {
        fn new() -> windows::core::Result<Self> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                let com_initialized = match hr {
                    S_OK | S_FALSE => true,
                    RPC_E_CHANGED_MODE => false,
                    other => {
                        other.ok()?;
                        false
                    }
                };
                MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
                Ok(Self { com_initialized })
            }
        }
    }

    impl Drop for WmfSession {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
                if self.com_initialized {
                    CoUninitialize();
                }
            }
        }
    }

    fn decode_audio_inner(bytes: &[u8]) -> windows::core::Result<AudioOutput> {
        unsafe {
            let reader = source_reader_from_bytes(bytes)?;
            let stream_index = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
            let media_type = media_type(&MFMediaType_Audio, &MFAudioFormat_PCM)?;
            reader.SetCurrentMediaType(stream_index, None, &media_type)?;
            let current_type = reader.GetCurrentMediaType(stream_index)?;
            let sample_rate = attribute_u32(&current_type, &MF_MT_AUDIO_SAMPLES_PER_SECOND)
                .filter(|value| *value > 0)
                .ok_or_else(|| wmf_error("audio decode reported an invalid sample rate"))?;
            let channels = attribute_u32(&current_type, &MF_MT_AUDIO_NUM_CHANNELS)
                .filter(|value| *value > 0)
                .ok_or_else(|| wmf_error("audio decode reported an invalid channel count"))?;
            let mut pcm = Vec::new();

            loop {
                let mut flags = 0;
                let mut sample = None;
                reader.ReadSample(
                    stream_index,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
                let Some(sample) = sample else {
                    continue;
                };
                let chunk = sample_bytes(&sample)?;
                if chunk.is_empty() {
                    continue;
                }
                let remaining = MAX_DECODED_AUDIO_BYTES.saturating_sub(pcm.len());
                if chunk.len() > remaining {
                    return Err(wmf_error(
                        "decoded audio exceeds the bounded CPU buffer budget",
                    ));
                }
                pcm.extend_from_slice(&chunk);
            }

            if pcm.is_empty() {
                return Err(wmf_error("audio decode produced no PCM samples"));
            }
            Ok(AudioOutput {
                pcm,
                sample_rate,
                channels,
                diagnostics: Vec::new(),
            })
        }
    }

    fn decode_video_inner(bytes: &[u8]) -> windows::core::Result<VideoOutput> {
        unsafe {
            let reader = source_reader_from_bytes(bytes)?;
            let stream_index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            let media_type = media_type(&MFMediaType_Video, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(stream_index, None, &media_type)?;
            let current_type = reader.GetCurrentMediaType(stream_index)?;
            let Some((width, height)) = attribute_frame_size(&current_type) else {
                return Err(wmf_error("video decode did not report a frame size"));
            };
            let expected_frame_bytes = width as usize * height as usize * 4;
            if expected_frame_bytes == 0 {
                return Err(wmf_error("video decode reported an empty frame size"));
            }
            if expected_frame_bytes > MAX_DECODED_VIDEO_FRAME_BYTES {
                return Err(wmf_error(
                    "video frame exceeds the bounded CPU buffer limit",
                ));
            }

            loop {
                let mut flags = 0;
                let mut sample = None;
                reader.ReadSample(
                    stream_index,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
                let Some(sample) = sample else {
                    continue;
                };
                let bytes = sample_bytes(&sample)?;
                if !bytes.is_empty() {
                    if bytes.len() < expected_frame_bytes {
                        return Err(wmf_error("video decode produced a partial BGRA frame"));
                    }
                    return Ok(VideoOutput {
                        bgra: bytes[..expected_frame_bytes].to_vec(),
                        width,
                        height,
                    });
                }
            }
            Err(wmf_error("video decode produced no BGRA frame"))
        }
    }

    unsafe fn source_reader_from_bytes(
        bytes: &[u8],
    ) -> windows::core::Result<windows::Win32::Media::MediaFoundation::IMFSourceReader> {
        if bytes.is_empty() {
            return Err(wmf_error("decode input is empty"));
        }
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
        let locked = GlobalLock(hglobal);
        if locked.is_null() {
            return Err(WindowsError::from_thread());
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(hglobal);
        let stream = CreateStreamOnHGlobal(hglobal, true)?;
        let byte_stream = MFCreateMFByteStreamOnStreamEx(&stream)?;
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 2)?;
        let attributes =
            attributes.ok_or_else(|| wmf_error("source reader attributes unavailable"))?;
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        MFCreateSourceReaderFromByteStream(&byte_stream, Some(&attributes))
    }

    unsafe fn media_type(
        major_type: &windows::core::GUID,
        subtype: &windows::core::GUID,
    ) -> windows::core::Result<IMFMediaType> {
        let media_type = MFCreateMediaType()?;
        let attrs: IMFAttributes = media_type.cast()?;
        attrs.SetGUID(&MF_MT_MAJOR_TYPE, major_type)?;
        attrs.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        Ok(media_type)
    }

    unsafe fn attribute_u32(media_type: &IMFMediaType, key: &windows::core::GUID) -> Option<u32> {
        let attrs: IMFAttributes = media_type.cast().ok()?;
        attrs.GetUINT32(key).ok()
    }

    unsafe fn attribute_frame_size(media_type: &IMFMediaType) -> Option<(u32, u32)> {
        let attrs: IMFAttributes = media_type.cast().ok()?;
        let packed = attrs.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let width = (packed >> 32) as u32;
        let height = (packed & 0xffff_ffff) as u32;
        (width > 0 && height > 0).then_some((width, height))
    }

    fn wmf_error(message: &'static str) -> WindowsError {
        WindowsError::new(HRESULT(0x80004005_u32 as i32), message)
    }

    unsafe fn sample_bytes(sample: &IMFSample) -> windows::core::Result<Vec<u8>> {
        let buffer = sample.ConvertToContiguousBuffer()?;
        let len = buffer.GetCurrentLength()? as usize;
        let mut data = ptr::null_mut();
        let mut current_len = 0;
        buffer.Lock(&mut data, None, Some(&mut current_len))?;
        let copy_len = (current_len as usize).min(len);
        let bytes = if copy_len == 0 || data.is_null() {
            Vec::new()
        } else {
            slice::from_raw_parts(data, copy_len).to_vec()
        };
        buffer.Unlock()?;
        Ok(bytes)
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[derive(Debug, Clone)]
pub struct FfmpegDecodeProvider {
    probed: bool,
}

#[cfg(feature = "ffmpeg-vcpkg")]
impl FfmpegDecodeProvider {
    pub fn new_unprobed() -> Self {
        Self { probed: false }
    }

    pub fn probe() -> Result<Self, MediaError> {
        ffmpeg::probe()?;
        Ok(Self { probed: true })
    }

    pub fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: "astra.decode.ffmpeg".to_string(),
            priority: ProviderPriority::Fallback,
            kinds: vec![DecodeKind::Audio, DecodeKind::Video],
            codecs: vec![
                "mp4".to_string(),
                "webm".to_string(),
                "wav".to_string(),
                "ogg".to_string(),
                "flac".to_string(),
                "mp3".to_string(),
            ],
            feature_gated: !self.probed,
            packaged_eligible: true,
            reference_only: false,
        }
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
impl DecodeProvider for FfmpegDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        FfmpegDecodeProvider::capability(self)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        if !self.probed {
            return Err(MediaError::message(
                "ffmpeg decode provider must be explicitly probed before use",
            ));
        }
        ffmpeg::decode(self.capability().provider_id, request)
    }
}
