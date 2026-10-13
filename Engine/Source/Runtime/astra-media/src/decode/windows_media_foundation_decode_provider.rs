use super::*;

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
                "m4v".to_string(),
                "wmv".to_string(),
                "asf".to_string(),
                "mpg".to_string(),
                "mpeg".to_string(),
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
                token_id: next_webcodecs_token()?,
                format: request.codec.clone(),
            }),
            diagnostics: Vec::new(),
        })
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn next_webcodecs_token() -> Result<String, MediaError> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
    let token = NEXT_TOKEN
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| {
            decode_error(
                "ASTRA_WEBCODECS_TOKEN_EXHAUSTED",
                "WebCodecs token sequence is exhausted",
            )
        })?;
    Ok(format!("webcodecs:{token}"))
}

#[cfg(target_arch = "wasm32")]
pub(super) fn webcodecs_available() -> bool {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;

    let global = js_sys::global();
    Reflect::has(&global, &JsValue::from_str("VideoDecoder")).unwrap_or(false)
        && Reflect::has(&global, &JsValue::from_str("AudioDecoder")).unwrap_or(false)
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[derive(Debug, Clone)]
pub struct FfmpegDecodeProvider {
    pub(super) probed: bool,
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
