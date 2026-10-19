//! Live translation service used by the independent AstraEMU host.
//!
//! The service talks to OpenAI, Anthropic, or Google Gemini endpoints and owns
//! only the network request and an optional in-memory session cache. It does
//! not know about a Family, a game save, or a rendered frame. Dropping the
//! future returned by [`TranslationClient::translate`] cancels the request;
//! callers should do that when a game session exits.

pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub const DEFAULT_CONTEXT_SEGMENTS: u32 = 8;
pub const DEFAULT_CONTEXT_CHARS: u32 = 6_000;
pub const MAX_CONTEXT_SEGMENTS: usize = 16;
pub const MIN_CONTEXT_CHARS: usize = 1_000;
pub const MAX_CONTEXT_CHARS: usize = 12_000;
pub const MAX_CUSTOM_PROMPT_CHARS: usize = 2_000;
pub const MAX_LISTED_MODELS: usize = 200;
pub const DEFAULT_CACHE_ENTRIES: usize = 128;
pub const DEFAULT_CACHE_CHARS: usize = 256_000;
pub(crate) const MAX_RESPONSE_BYTES: usize = 1_048_576;

mod cache;
mod model;
mod prompt;
#[cfg(not(target_os = "android"))]
mod secret;
mod service;
mod transport;

pub use cache::{TranslationSession, TranslationSessionCache};
pub use model::{
    ConnectionTestResult, OpenAiApi, SecretResolver, TranslationError, TranslationProfile,
    TranslationProvider, TranslationRequest, TranslationResult, TranslationSegment,
};
pub use prompt::build_prompt;
#[cfg(not(target_os = "android"))]
pub use secret::PlatformSecretStore;
pub use service::{AsyncTranslationService, TranslationPoll, TranslationServiceError};
pub use transport::TranslationClient;

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
            base_url: "https://example.com/v1".into(),
            model: "model".into(),
            secret_reference: "test.secret".into(),
            ..Default::default()
        }
    }

    #[test]
    fn profile_defaults_to_fifteen_second_timeout_and_validates() {
        let defaults = TranslationProfile::default();
        assert_eq!(defaults.timeout_ms, 15_000);
        assert_eq!(defaults.provider, TranslationProvider::OpenAi);
        assert_eq!(defaults.base_url, "https://api.openai.com/v1");
        assert_eq!(defaults.context_segments, 8);
        assert_eq!(defaults.context_chars, 6_000);
        profile().validate().unwrap();
        assert_eq!(profile().provider_identity(), "openai:test");
    }

    #[test]
    fn provider_serialization_and_default_urls() {
        for (provider, name, url) in [
            (
                TranslationProvider::OpenAi,
                "openai",
                "https://api.openai.com/v1",
            ),
            (
                TranslationProvider::Anthropic,
                "anthropic",
                "https://api.anthropic.com/v1",
            ),
            (
                TranslationProvider::Google,
                "google",
                "https://generativelanguage.googleapis.com/v1beta",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(provider).unwrap(),
                serde_json::json!(name)
            );
            assert_eq!(provider.default_base_url(), url);
            TranslationProfile {
                provider,
                base_url: url.into(),
                ..profile()
            }
            .validate()
            .unwrap();
        }
        assert_eq!(
            serde_json::to_value(OpenAiApi::ChatCompletions).unwrap(),
            serde_json::json!("chat_completions")
        );
    }

    #[test]
    fn base_url_allows_https_and_loopback_http_only() {
        for url in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:8080/v1",
            "http://[::1]:8080/v1",
            "https://example.com/v1/",
        ] {
            TranslationProfile {
                base_url: url.into(),
                ..profile()
            }
            .validate()
            .unwrap_or_else(|error| panic!("{url}: {error}"));
        }
        for url in [
            "http://example.com/v1",
            "http://10.0.0.1/v1",
            "https://user:pass@example.com/v1",
            "https://example.com/v1?key=1",
            "https://example.com/v1#frag",
            "ftp://example.com/v1",
            "not a url",
        ] {
            assert!(
                TranslationProfile {
                    base_url: url.into(),
                    ..profile()
                }
                .validate()
                .is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn profile_bounds_are_enforced() {
        let invalid = [
            TranslationProfile {
                custom_prompt: "x".repeat(MAX_CUSTOM_PROMPT_CHARS + 1),
                ..profile()
            },
            TranslationProfile {
                context_segments: 17,
                ..profile()
            },
            TranslationProfile {
                context_chars: 999,
                ..profile()
            },
            TranslationProfile {
                context_chars: 12_001,
                ..profile()
            },
            TranslationProfile {
                timeout_ms: 999,
                ..profile()
            },
            TranslationProfile {
                timeout_ms: 120_001,
                ..profile()
            },
        ];
        for profile in invalid {
            assert!(profile.validate().is_err(), "{profile:?}");
        }
        TranslationProfile {
            custom_prompt: "x".repeat(MAX_CUSTOM_PROMPT_CHARS),
            context_segments: 0,
            context_chars: 12_000,
            ..profile()
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn request_keeps_newest_segments_within_profile_limit() {
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
        assert!(!prompt.contains("segment-0\n"));
        assert!(!prompt.contains("segment-3\n"));
        assert!(prompt.contains("segment-4\n"));
        assert!(prompt.contains("segment-11\n"));
        assert!(prompt.contains("speaker=Alice, ruby=いま"));

        let narrow = TranslationProfile {
            context_segments: 2,
            ..profile()
        };
        let prompt = build_prompt(&narrow, &request).unwrap();
        assert!(!prompt.contains("segment-9\n"));
        assert!(prompt.contains("segment-10\n"));
        assert!(prompt.contains("segment-11\n"));

        let none = TranslationProfile {
            context_segments: 0,
            ..profile()
        };
        assert!(!build_prompt(&none, &request).unwrap().contains("CONTEXT:"));

        request.current.text = "x".repeat(MAX_CONTEXT_CHARS + 1);
        assert!(matches!(
            request.validate(),
            Err(TranslationError::ContextLimit)
        ));
    }

    #[test]
    fn context_chars_limit_drops_older_segments() {
        let request = TranslationRequest {
            current: TranslationSegment::new("now"),
            recent: vec![
                TranslationSegment::new("a".repeat(600)),
                TranslationSegment::new("b".repeat(600)),
            ],
        };
        let profile = TranslationProfile {
            context_chars: 1_000,
            ..profile()
        };
        let prompt = build_prompt(&profile, &request).unwrap();
        assert!(!prompt.contains(&"a".repeat(600)));
        assert!(prompt.contains(&"b".repeat(600)));
    }

    #[test]
    fn custom_prompt_is_a_style_section_before_context() {
        let request = TranslationRequest::plain("now", ["before".to_owned()]);
        let styled = TranslationProfile {
            custom_prompt: "Use a gentle tone.".into(),
            ..profile()
        };
        let prompt = build_prompt(&styled, &request).unwrap();
        let style = prompt.find("STYLE:\nUse a gentle tone.\n").unwrap();
        let context = prompt.find("CONTEXT:\n").unwrap();
        let current = prompt.find("CURRENT:\n").unwrap();
        assert!(style < context && context < current);
        assert!(!build_prompt(&profile(), &request)
            .unwrap()
            .contains("STYLE:"));
    }

    #[test]
    fn client_constructs_with_secret_resolver() {
        let client = TranslationClient::new(profile(), Arc::new(TestSecrets)).unwrap();
        assert_eq!(client.profile().timeout_ms, DEFAULT_TIMEOUT_MS);
    }
}
