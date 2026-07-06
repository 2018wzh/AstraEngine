use std::io::{Cursor, ErrorKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
}

pub trait DecodeProvider: Send + Sync {
    fn capability(&self) -> DecodeCapability;
    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodePolicy {
    pub profile: String,
    pub fallback_enabled: bool,
    pub prefer_fallback: bool,
}

impl DecodePolicy {
    pub fn desktop_release() -> Self {
        Self {
            profile: "desktop-release".to_string(),
            fallback_enabled: true,
            prefer_fallback: false,
        }
    }

    pub fn without_fallback(mut self) -> Self {
        self.fallback_enabled = false;
        self
    }

    pub fn prefer_fallback_for_tests(mut self) -> Self {
        self.prefer_fallback = true;
        self
    }
}

#[derive(Default)]
pub struct DecodeProviderRegistry {
    providers: Vec<Box<dyn DecodeProvider>>,
}

impl DecodeProviderRegistry {
    pub fn register(&mut self, provider: Box<dyn DecodeProvider>) {
        self.providers.push(provider);
    }

    pub fn select(
        &self,
        request: &DecodeRequest,
        policy: &DecodePolicy,
    ) -> Result<DecodeCapability, MediaError> {
        let mut candidates: Vec<_> = self
            .providers
            .iter()
            .map(|provider| provider.capability())
            .filter(|capability| {
                capability.kinds.contains(&request.kind)
                    && capability
                        .codecs
                        .iter()
                        .any(|codec| codec == &request.codec)
                    && capability.packaged_eligible
                    && (policy.fallback_enabled
                        || capability.priority != ProviderPriority::Fallback)
            })
            .collect();
        candidates.sort_by_key(
            |capability| match (policy.prefer_fallback, capability.priority) {
                (true, ProviderPriority::Fallback) => 0,
                (true, ProviderPriority::Platform) => 1,
                (false, ProviderPriority::Platform) => 0,
                (false, ProviderPriority::Fallback) => 1,
            },
        );
        candidates
            .into_iter()
            .next()
            .ok_or_else(|| MediaError::message("no eligible decode provider"))
    }
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
            packaged_eligible: true,
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
        let mut diagnostics = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(err) => return Err(MediaError::message(format!("read audio packet: {err}"))),
            };
            if packet.track_id != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(err) => return Err(MediaError::message(format!("decode audio packet: {err}"))),
            };
            sample_rate = decoded.spec().rate();
            channels = decoded.spec().channels().count();
            let mut samples = vec![0i16; decoded.samples_interleaved()];
            decoded.copy_to_slice_interleaved(&mut samples);
            let additional = samples.len() * std::mem::size_of::<i16>();
            if pcm.len().saturating_add(additional) > MAX_DECODED_AUDIO_BYTES {
                diagnostics.push(Diagnostic::warning(
                    "ASTRA_AUDIO_DECODE_TRUNCATED",
                    "decoded audio exceeded the bounded CPU buffer limit",
                ));
                break;
            }
            for sample in samples {
                pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }

        if pcm.is_empty() {
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
            diagnostics,
        })
    }
}

#[cfg(feature = "ffmpeg")]
#[derive(Debug, Clone)]
pub struct FfmpegDecodeProvider {
    probed: bool,
}

#[cfg(feature = "ffmpeg")]
impl FfmpegDecodeProvider {
    pub fn new_unprobed() -> Self {
        Self { probed: false }
    }

    pub fn probe() -> Result<Self, MediaError> {
        #[cfg(feature = "ffmpeg-system")]
        {
            ffmpeg_next::init()
                .map_err(|err| MediaError::message(format!("ffmpeg init: {err}")))?;
            return Ok(Self { probed: true });
        }
        #[cfg(not(feature = "ffmpeg-system"))]
        {
            Err(MediaError::message(
                "ffmpeg feature is enabled without ffmpeg-system native bindings",
            ))
        }
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
        }
    }
}

#[cfg(feature = "ffmpeg")]
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
        Ok(DecodeResult {
            provider_id: self.capability().provider_id,
            kind: request.kind,
            codec: request.codec.clone(),
            output: DecodeOutput::MediaSurfaceToken(MediaSurfaceToken {
                provider_id: self.capability().provider_id,
                token_id: format!("ffmpeg:{}", Hash256::from_sha256(&request.bytes).to_hex()),
                format: request.codec.clone(),
            }),
            diagnostics: Vec::new(),
        })
    }
}
