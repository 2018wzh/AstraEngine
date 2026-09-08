//! OpenAI-compatible translation service used by the independent AstraEMU host.
//!
//! The service owns only the network request and an optional in-memory session
//! cache. It does not know about a Family, a game save, or a rendered frame.
//! Dropping the future returned by [`OpenAiCompatibleTranslationProvider::translate`]
//! cancels the request; callers should do that when a game session exits.

pub const ECNU_BASE_URL: &str = "https://chat.ecnu.edu.cn/open/api/v1";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub const MAX_CONTEXT_SEGMENTS: usize = 8;
pub const MAX_CONTEXT_CHARS: usize = 6_000;
pub const DEFAULT_CACHE_ENTRIES: usize = 128;
pub const DEFAULT_CACHE_CHARS: usize = 256_000;
pub(crate) const MAX_RESPONSE_BYTES: usize = 1_048_576;

mod cache;
mod model;
mod prompt;
#[cfg(not(target_os = "android"))]
mod secret;
mod transport;

pub use cache::{TranslationSession, TranslationSessionCache};
pub use model::{
    ConnectionTestResult, SecretResolver, TranslationEndpointKind, TranslationError,
    TranslationProfile, TranslationProtocol, TranslationRequest, TranslationResult,
    TranslationSegment,
};
pub use prompt::build_prompt;
#[cfg(not(target_os = "android"))]
pub use secret::PlatformSecretStore;
pub use transport::OpenAiCompatibleTranslationProvider;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[derive(Debug)]
    struct TestSecrets;

    impl SecretResolver for TestSecrets {
        fn resolve(&self, _reference: &str) -> Result<String, TranslationError> {
            Ok("secret".into())
        }
    }

    fn profile() -> TranslationProfile {
        TranslationProfile {
            profile_id: "test".into(),
            endpoint_kind: TranslationEndpointKind::OpenAiCompatible,
            endpoint: "https://example.com/v1".into(),
            protocol: TranslationProtocol::Responses,
            model: "model".into(),
            target_language: "zh-CN".into(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            secret_reference: "test.secret".into(),
        }
    }

    #[test]
    fn profile_defaults_to_fifteen_second_timeout_and_validates() {
        assert_eq!(TranslationProfile::default().timeout_ms, 15_000);
        profile().validate().unwrap();
    }

    #[test]
    fn request_keeps_newest_eight_segments_and_optional_metadata() {
        let mut request = TranslationRequest {
            current: TranslationSegment {
                text: "现在".into(),
                speaker: Some("Alice".into()),
                ruby: Some("いま".into()),
            },
            recent: (0..12)
                .map(|index| TranslationSegment::new(format!("segment-{index}")))
                .collect(),
        };
        let prompt = build_prompt(&profile(), &request).unwrap();
        assert!(!prompt.contains("segment-0"));
        assert!(!prompt.contains("segment-3"));
        assert!(prompt.contains("segment-4"));
        assert!(prompt.contains("segment-11"));
        assert!(prompt.contains("speaker=Alice, ruby=いま"));
        assert!(prompt.chars().count() <= MAX_CONTEXT_CHARS + 256);

        request.current.text = "x".repeat(MAX_CONTEXT_CHARS + 1);
        assert!(matches!(
            request.validate(),
            Err(TranslationError::ContextLimit)
        ));
    }

    #[test]
    fn provider_constructs_with_secret_resolver() {
        let provider =
            OpenAiCompatibleTranslationProvider::new(profile(), Arc::new(TestSecrets)).unwrap();
        assert_eq!(provider.profile().timeout_ms, DEFAULT_TIMEOUT_MS);
    }
}
