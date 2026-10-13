use super::*;

pub(super) const MAX_DECODED_AUDIO_BYTES: usize = 16 * 1024 * 1024;

#[cfg(any(windows, feature = "ffmpeg-vcpkg"))]
pub(super) const MAX_DECODED_VIDEO_FRAME_BYTES: usize = 64 * 1024 * 1024;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DecodeKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeRequest {
    pub kind: DecodeKind,
    pub codec: String,
    pub bytes: astra_byte_source::OwnedByteBuffer,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodeResult {
    pub provider_id: String,
    pub kind: DecodeKind,
    pub codec: String,
    pub output: DecodeOutput,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodeOutput {
    CpuBuffer {
        bytes: astra_byte_source::OwnedByteBuffer,
        format: String,
    },
    AudioPcmI16 {
        sample_rate: u32,
        channels: u16,
        samples: Vec<i16>,
    },
    AudioPcmF32 {
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
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

#[cfg(windows)]
pub fn decode_windows_video_stream(
    bytes: &[u8],
    max_frames: u64,
    max_bytes: u64,
) -> Result<crate::DecodedVideoStream, MediaError> {
    wmf_decode::decode_video_stream(bytes, max_frames, max_bytes)
}

/// Incremental Media Foundation video decoder. The encoded source stays owned
/// by the MF byte stream while decoded frames are released one at a time, so a
/// caller can enforce a small presentation ring instead of retaining the full
/// decoded movie.
pub struct WindowsVideoStreamDecoder {
    #[cfg(windows)]
    pub(super) inner: wmf_decode::IncrementalVideoDecoder,
}

impl WindowsVideoStreamDecoder {
    pub fn duration_us(&self) -> u64 {
        #[cfg(windows)]
        {
            self.inner.duration_us
        }
        #[cfg(not(windows))]
        {
            0
        }
    }

    pub fn next_frame(&mut self) -> Result<Option<crate::DecodedVideoFrame>, MediaError> {
        #[cfg(windows)]
        {
            self.inner.next_frame().map_err(wmf_decode::decode_error)
        }
        #[cfg(not(windows))]
        {
            Err(decode_error(
                "ASTRA_WMF_PLATFORM_UNAVAILABLE",
                "Media Foundation video stream decode is only available on Windows",
            ))
        }
    }
}

pub fn open_windows_video_stream(
    bytes: &[u8],
    max_frames: u64,
    max_bytes: u64,
) -> Result<WindowsVideoStreamDecoder, MediaError> {
    #[cfg(windows)]
    {
        Ok(WindowsVideoStreamDecoder {
            inner: wmf_decode::IncrementalVideoDecoder::open(bytes, max_frames, max_bytes)
                .map_err(wmf_decode::decode_error)?,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, max_frames, max_bytes);
        Err(decode_error(
            "ASTRA_WMF_PLATFORM_UNAVAILABLE",
            "Media Foundation video stream decode is only available on Windows",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsDecodedAudioChunk {
    pub pts_us: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymphoniaDecodedAudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

pub struct SymphoniaAudioStreamDecoder {
    pub(super) format: Box<dyn FormatReader>,
    pub(super) decoder: Box<dyn AudioDecoder>,
    pub(super) track_id: u32,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) decoded_bytes: u64,
    pub(super) max_decoded_bytes: u64,
}

impl SymphoniaAudioStreamDecoder {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn next_chunk(&mut self) -> Result<Option<SymphoniaDecodedAudioChunk>, MediaError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),
                Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                    return Err(decode_error(
                        "ASTRA_AUDIO_STREAM_TRUNCATED_INPUT",
                        "streaming audio container ended before an explicit end of stream",
                    ));
                }
                Err(error) => {
                    return Err(MediaError::message(format!(
                        "read streaming audio packet: {error}"
                    )))
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let decoded = self.decoder.decode(&packet).map_err(|error| match error {
                SymphoniaError::DecodeError(_) => decode_error(
                    "ASTRA_AUDIO_STREAM_CORRUPT_PACKET",
                    "streaming audio decoder rejected a media packet",
                ),
                error => MediaError::message(format!("decode streaming audio packet: {error}")),
            })?;
            self.sample_rate = decoded.spec().rate();
            self.channels = u16::try_from(decoded.spec().channels().count()).map_err(|_| {
                decode_error(
                    "ASTRA_AUDIO_STREAM_CHANNELS",
                    "streaming audio channel count exceeds the supported range",
                )
            })?;
            let mut samples = vec![0i16; decoded.samples_interleaved()];
            decoded.copy_to_slice_interleaved(&mut samples);
            let chunk_bytes = u64::try_from(samples.len())
                .ok()
                .and_then(|samples| samples.checked_mul(2))
                .ok_or_else(|| {
                    decode_error(
                        "ASTRA_AUDIO_STREAM_BUDGET",
                        "streaming audio sample byte count overflowed",
                    )
                })?;
            self.decoded_bytes = self.decoded_bytes.checked_add(chunk_bytes).ok_or_else(|| {
                decode_error(
                    "ASTRA_AUDIO_STREAM_BUDGET",
                    "streaming audio decoded byte count overflowed",
                )
            })?;
            if self.decoded_bytes > self.max_decoded_bytes {
                return Err(decode_error(
                    "ASTRA_AUDIO_STREAM_BUDGET",
                    "streaming audio exceeds the declared session decode budget",
                ));
            }
            return Ok(Some(SymphoniaDecodedAudioChunk {
                sample_rate: self.sample_rate,
                channels: self.channels,
                samples,
            }));
        }
    }
}

pub fn open_symphonia_audio_stream(
    codec: &str,
    bytes: astra_byte_source::OwnedByteBuffer,
    max_decoded_bytes: u64,
) -> Result<SymphoniaAudioStreamDecoder, MediaError> {
    if !matches!(codec, "wav" | "ogg" | "flac" | "mp3")
        || bytes.is_empty()
        || max_decoded_bytes == 0
    {
        return Err(decode_error(
            "ASTRA_AUDIO_STREAM_DESCRIPTOR",
            "streaming audio descriptor is invalid",
        ));
    }
    let mut hint = Hint::new();
    hint.with_extension(codec);
    let media_stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            media_stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| MediaError::message(format!("probe streaming audio: {error}")))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| MediaError::message("streaming audio has no supported track"))?;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| MediaError::message("streaming audio is missing codec parameters"))?;
    if codec_params.codec == CODEC_ID_NULL_AUDIO {
        return Err(MediaError::message("streaming audio has no codec id"));
    }
    let track_id = track.id;
    let sample_rate = codec_params.sample_rate.unwrap_or_default();
    let channels = codec_params
        .channels
        .as_ref()
        .map(|channels| channels.count())
        .unwrap_or_default();
    let channels = u16::try_from(channels).map_err(|_| {
        decode_error(
            "ASTRA_AUDIO_STREAM_CHANNELS",
            "streaming audio channel count exceeds the supported range",
        )
    })?;
    if sample_rate == 0 || channels == 0 {
        return Err(decode_error(
            "ASTRA_AUDIO_STREAM_FORMAT",
            "streaming audio format is incomplete",
        ));
    }
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
        .map_err(|error| MediaError::message(format!("create streaming audio decoder: {error}")))?;
    Ok(SymphoniaAudioStreamDecoder {
        format,
        decoder,
        track_id,
        sample_rate,
        channels,
        decoded_bytes: 0,
        max_decoded_bytes,
    })
}

pub struct WindowsAudioStreamDecoder {
    #[cfg(windows)]
    pub(super) inner: wmf_decode::IncrementalAudioDecoder,
}

impl WindowsAudioStreamDecoder {
    pub fn next_chunk(&mut self) -> Result<Option<WindowsDecodedAudioChunk>, MediaError> {
        #[cfg(windows)]
        {
            self.inner.next_chunk().map_err(wmf_decode::decode_error)
        }
        #[cfg(not(windows))]
        {
            Err(decode_error(
                "ASTRA_WMF_PLATFORM_UNAVAILABLE",
                "Media Foundation audio stream decode is only available on Windows",
            ))
        }
    }
}

pub fn open_windows_audio_stream(
    bytes: &[u8],
    max_samples: u64,
) -> Result<WindowsAudioStreamDecoder, MediaError> {
    #[cfg(windows)]
    {
        Ok(WindowsAudioStreamDecoder {
            inner: wmf_decode::IncrementalAudioDecoder::open(bytes, max_samples)
                .map_err(wmf_decode::decode_error)?,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, max_samples);
        Err(decode_error(
            "ASTRA_WMF_PLATFORM_UNAVAILABLE",
            "Media Foundation audio stream decode is only available on Windows",
        ))
    }
}

#[cfg(not(windows))]
pub fn decode_windows_video_stream(
    _bytes: &[u8],
    _max_frames: u64,
    _max_bytes: u64,
) -> Result<crate::DecodedVideoStream, MediaError> {
    Err(decode_error(
        "ASTRA_WMF_PLATFORM_UNAVAILABLE",
        "Media Foundation video stream decode is only available on Windows",
    ))
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
    pub(super) providers: BTreeMap<String, Box<dyn DecodeProvider>>,
}
