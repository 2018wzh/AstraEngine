use std::{collections::BTreeMap, fmt, io::Read};

use super::{playback_error, IncrementalMediaDecoder};
use crate::MediaError;

/// Provider identity and codec eligibility for an incremental decoder.
///
/// The composition root must register and select a provider explicitly.  A
/// provider with a missing, mismatched, or feature-gated capability is never
/// opened and is never treated as an implicit fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalDecodeCapability {
    pub provider_id: String,
    pub codecs: Vec<String>,
    pub feature_gated: bool,
}

/// Provider-independent bounds applied before a streaming decoder is opened.
/// Codec-specific timing limits remain owned by the selected provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncrementalDecodeBudget {
    pub max_encoded_bytes: usize,
    pub max_video_frame_bytes: usize,
    pub max_pending_packets: usize,
    pub max_video_frames: usize,
    pub max_audio_packets: usize,
}

impl Default for IncrementalDecodeBudget {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 256 * 1024 * 1024,
            max_video_frame_bytes: 64 * 1024 * 1024,
            max_pending_packets: 64,
            max_video_frames: 64,
            max_audio_packets: 64,
        }
    }
}

/// Owned request passed to exactly one incrementally bound decode provider.
/// The reader is intentionally opaque to the registry, so platform and family
/// adapters can provide VFS-backed sources without exposing paths or payloads.
pub struct IncrementalDecodeRequest {
    pub codec: String,
    pub reader: Box<dyn Read>,
    pub budget: IncrementalDecodeBudget,
}

impl fmt::Debug for IncrementalDecodeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IncrementalDecodeRequest")
            .field("codec", &self.codec)
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl IncrementalDecodeRequest {
    pub fn new(codec: impl Into<String>, reader: Box<dyn Read>) -> Self {
        Self {
            codec: codec.into().to_ascii_lowercase(),
            reader,
            budget: IncrementalDecodeBudget::default(),
        }
    }

    pub fn with_budget(mut self, budget: IncrementalDecodeBudget) -> Self {
        self.budget = budget;
        self
    }

    fn validate(&self) -> Result<(), MediaError> {
        if self.codec.is_empty()
            || !self
                .codec
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || self.budget.max_encoded_bytes == 0
            || self.budget.max_video_frame_bytes == 0
            || self.budget.max_pending_packets == 0
            || self.budget.max_video_frames == 0
            || self.budget.max_audio_packets == 0
        {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_REQUEST",
                "incremental decoder request identity or budget is invalid",
            ));
        }
        Ok(())
    }
}

/// Factory owned by a decode backend.  A provider may be FFmpeg, a platform
/// stream, or a family-specific mature decoder, but selection is always by
/// the explicit provider id supplied by the composition root.
pub trait IncrementalDecodeProvider {
    fn provider_id(&self) -> &'static str;
    fn capability(&self) -> IncrementalDecodeCapability;
    fn open_reader(
        &self,
        request: IncrementalDecodeRequest,
    ) -> Result<Box<dyn IncrementalMediaDecoder>, MediaError>;
}

/// Session-scoped registry for incremental providers.  It deliberately does
/// not choose by registration order and does not expose a fallback chain.
#[derive(Default)]
pub struct IncrementalDecodeProviderRegistry {
    providers: BTreeMap<String, Box<dyn IncrementalDecodeProvider>>,
}

impl IncrementalDecodeProviderRegistry {
    pub fn register(
        &mut self,
        provider: Box<dyn IncrementalDecodeProvider>,
    ) -> Result<(), MediaError> {
        let provider_id = provider.provider_id();
        let capability = provider.capability();
        if !safe_identity(provider_id)
            || capability.provider_id != provider_id
            || capability.codecs.is_empty()
            || capability.codecs.iter().any(|codec| !safe_codec(codec))
            || capability.codecs.len()
                != capability
                    .codecs
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
        {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_PROVIDER_CAPABILITY",
                "incremental provider capability identity or codec set is invalid",
            ));
        }
        if self.providers.contains_key(provider_id) {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_PROVIDER_DUPLICATE",
                "incremental provider id was registered more than once",
            ));
        }
        self.providers.insert(provider_id.to_owned(), provider);
        Ok(())
    }

    pub fn open(
        &self,
        provider_id: &str,
        request: IncrementalDecodeRequest,
    ) -> Result<Box<dyn IncrementalMediaDecoder>, MediaError> {
        request.validate()?;
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_INCREMENTAL_PROVIDER_MISSING",
                "requested incremental provider is not registered",
            )
        })?;
        if provider.provider_id() != provider_id {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_PROVIDER_IDENTITY",
                "incremental provider registry identity changed",
            ));
        }
        let capability = provider.capability();
        if capability.provider_id != provider_id
            || capability.feature_gated
            || !capability
                .codecs
                .iter()
                .any(|codec| codec == &request.codec)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_PROVIDER_INELIGIBLE",
                "incremental provider is not eligible for the bound codec",
            ));
        }
        let decoder = provider.open_reader(request)?;
        if decoder.provider_id() != provider_id {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_DECODER_IDENTITY",
                "incremental decoder identity does not match its selected provider",
            ));
        }
        Ok(decoder)
    }

    pub fn contains(&self, provider_id: &str) -> bool {
        self.providers.contains_key(provider_id)
    }
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{DecodedMediaPacket, MediaPlaybackConfig};

    struct EmptyDecoder;

    impl IncrementalMediaDecoder for EmptyDecoder {
        fn provider_id(&self) -> &'static str {
            "test.incremental"
        }

        fn playback_config(&self) -> MediaPlaybackConfig {
            MediaPlaybackConfig {
                duration_us: 1,
                has_video: true,
                has_audio: false,
                ..MediaPlaybackConfig::default()
            }
        }

        fn read_next(&mut self) -> Result<Option<DecodedMediaPacket>, MediaError> {
            Ok(None)
        }

        fn seek(&mut self, _position_us: u64) -> Result<u64, MediaError> {
            Ok(1)
        }

        fn cancel(&mut self) -> Result<(), MediaError> {
            Ok(())
        }
    }

    struct TestProvider;

    impl IncrementalDecodeProvider for TestProvider {
        fn provider_id(&self) -> &'static str {
            "test.incremental"
        }

        fn capability(&self) -> IncrementalDecodeCapability {
            IncrementalDecodeCapability {
                provider_id: "test.incremental".to_owned(),
                codecs: vec!["avi".to_owned()],
                feature_gated: false,
            }
        }

        fn open_reader(
            &self,
            _request: IncrementalDecodeRequest,
        ) -> Result<Box<dyn IncrementalMediaDecoder>, MediaError> {
            Ok(Box::new(EmptyDecoder))
        }
    }

    struct MismatchedProvider;

    impl IncrementalDecodeProvider for MismatchedProvider {
        fn provider_id(&self) -> &'static str {
            "test.mismatch"
        }

        fn capability(&self) -> IncrementalDecodeCapability {
            IncrementalDecodeCapability {
                provider_id: "test.mismatch".to_owned(),
                codecs: vec!["avi".to_owned()],
                feature_gated: false,
            }
        }

        fn open_reader(
            &self,
            _request: IncrementalDecodeRequest,
        ) -> Result<Box<dyn IncrementalMediaDecoder>, MediaError> {
            Ok(Box::new(EmptyDecoder))
        }
    }

    #[test]
    fn registry_requires_explicit_identity_and_rejects_duplicates() {
        fn code(error: MediaError) -> String {
            match error {
                MediaError::Diagnostics(diagnostics) => diagnostics
                    .first()
                    .map(|diagnostic| diagnostic.code.clone())
                    .unwrap_or_default(),
                MediaError::Message(message) => message,
            }
        }

        let mut registry = IncrementalDecodeProviderRegistry::default();
        registry.register(Box::new(TestProvider)).unwrap();
        assert!(registry.contains("test.incremental"));
        let duplicate = match registry.register(Box::new(TestProvider)) {
            Ok(()) => panic!("duplicate provider registration unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(
            code(duplicate),
            "ASTRA_MEDIA_INCREMENTAL_PROVIDER_DUPLICATE"
        );
        let request = IncrementalDecodeRequest::new("avi", Box::new(Cursor::new([1_u8])));
        let missing = match registry.open("missing", request) {
            Ok(_) => panic!("missing provider unexpectedly opened"),
            Err(error) => error,
        };
        assert_eq!(code(missing), "ASTRA_MEDIA_INCREMENTAL_PROVIDER_MISSING");

        registry.register(Box::new(MismatchedProvider)).unwrap();
        let identity = match registry.open(
            "test.mismatch",
            IncrementalDecodeRequest::new("avi", Box::new(Cursor::new([1_u8]))),
        ) {
            Ok(_) => panic!("mismatched decoder unexpectedly opened"),
            Err(error) => error,
        };
        assert_eq!(code(identity), "ASTRA_MEDIA_INCREMENTAL_DECODER_IDENTITY");

        let ineligible = match registry.open(
            "test.incremental",
            IncrementalDecodeRequest::new("mp4", Box::new(Cursor::new([1_u8]))),
        ) {
            Ok(_) => panic!("provider unexpectedly accepted an undeclared codec"),
            Err(error) => error,
        };
        assert_eq!(
            code(ineligible),
            "ASTRA_MEDIA_INCREMENTAL_PROVIDER_INELIGIBLE"
        );
    }
}
