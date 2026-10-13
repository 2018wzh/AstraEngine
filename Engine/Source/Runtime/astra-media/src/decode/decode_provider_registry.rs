use super::*;

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

pub(super) fn validate_capability(capability: &DecodeCapability) -> Result<(), MediaError> {
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

pub(super) fn validate_request(
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

pub(super) fn validate_output(output: &DecodeOutput) -> Result<(), MediaError> {
    match output {
        DecodeOutput::CpuBuffer { bytes, format } => {
            if bytes.is_empty() || format.is_empty() {
                return Err(decode_error(
                    "ASTRA_DECODE_OUTPUT_INVALID",
                    "decoded CPU buffer is empty or has an invalid format",
                ));
            }
        }
        DecodeOutput::AudioPcmI16 {
            sample_rate,
            channels,
            samples,
        } => validate_audio_samples(*sample_rate, *channels, samples.len(), true)?,
        DecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        } => {
            validate_audio_samples(*sample_rate, *channels, samples.len(), true)?;
            if contains_non_finite_sample(samples) {
                return Err(decode_error(
                    "ASTRA_DECODE_OUTPUT_INVALID",
                    "decoded floating-point PCM contains a non-finite sample",
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

pub(super) fn validate_audio_samples(
    sample_rate: u32,
    channels: u16,
    sample_count: usize,
    require_non_empty: bool,
) -> Result<(), MediaError> {
    if !is_supported_pcm_rate(sample_rate)
        || !is_supported_pcm_channel_count(channels)
        || (require_non_empty && sample_count == 0)
        || !is_aligned_pcm_sample_count(sample_count, channels)
    {
        return Err(decode_error(
            "ASTRA_DECODE_OUTPUT_INVALID",
            "decoded PCM format, length, or frame alignment is invalid",
        ));
    }
    Ok(())
}

pub(super) fn safe_codec(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(super) fn decode_error(code: impl Into<String>, message: impl Into<String>) -> MediaError {
    MediaError::Diagnostics(vec![Diagnostic::blocking(code, message)])
}

#[derive(Debug, Clone, Default)]
pub struct ImageDecodeProvider;

impl DecodeProvider for ImageDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: "astra.decode.image".to_string(),
            priority: ProviderPriority::Platform,
            kinds: vec![DecodeKind::Image],
            codecs: vec![
                "png".to_string(),
                "jpeg".to_string(),
                "jpg".to_string(),
                "bmp".to_string(),
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
                bytes: rgba.into(),
                format: "rgba8".to_string(),
            },
            diagnostics: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SyntheticPlatformDecodeProvider {
    pub(super) provider_id: String,
    pub(super) codecs: Vec<String>,
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
                token_id: "surface.synthetic".to_string(),
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
        let mut pcm = Vec::<i16>::new();
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
            let additional = samples.len();
            if pcm
                .len()
                .checked_add(additional)
                .and_then(|total| total.checked_mul(std::mem::size_of::<i16>()))
                .is_none_or(|total_bytes| total_bytes > MAX_DECODED_AUDIO_BYTES)
            {
                return Err(decode_error(
                    "ASTRA_AUDIO_DECODE_BUDGET",
                    "decoded audio exceeds the bounded CPU buffer budget",
                ));
            }
            pcm.extend(samples);
        }

        if pcm.is_empty() || sample_rate == 0 || channels == 0 {
            return Err(MediaError::message("audio decode produced no PCM samples"));
        }
        Ok(DecodeResult {
            provider_id: self.capability().provider_id,
            kind: request.kind,
            codec: request.codec.clone(),
            output: DecodeOutput::AudioPcmI16 {
                samples: pcm,
                sample_rate,
                channels: u16::try_from(channels).map_err(|_| {
                    decode_error(
                        "ASTRA_AUDIO_DECODE_CHANNELS",
                        "decoded audio channel count exceeds u16",
                    )
                })?,
            },
            diagnostics: Vec::new(),
        })
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Default)]
pub struct WindowsMediaFoundationDecodeProvider;
