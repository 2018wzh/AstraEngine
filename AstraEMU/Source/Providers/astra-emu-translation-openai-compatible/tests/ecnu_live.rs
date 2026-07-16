use std::{collections::VecDeque, sync::Arc};

use astra_emu_translation_openai_compatible::{
    OpenAiCompatibleTranslationProvider, SecretResolver, TranslationEndpointKind, TranslationError,
    TranslationProfile, TranslationProtocol, TranslationRequest, ECNU_BASE_URL,
};

struct EnvironmentSecretResolver;

impl SecretResolver for EnvironmentSecretResolver {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        std::env::var(reference).map_err(|_| TranslationError::SecretUnavailable)
    }
}

#[tokio::test]
#[ignore = "requires an explicitly supplied ECNU_API_KEY and performs one live provider request"]
async fn ecnu_responses_live_does_not_expose_credentials() {
    let provider = OpenAiCompatibleTranslationProvider::new(
        TranslationProfile {
            profile_id: "ecnu-live".into(),
            endpoint_kind: TranslationEndpointKind::Ecnu,
            endpoint: ECNU_BASE_URL.into(),
            protocol: TranslationProtocol::Responses,
            model: "ecnu-plus".into(),
            target_language: "zh-CN".into(),
            context_sentences: 2,
            body_limit_bytes: 16 * 1024,
            timeout_ms: 60_000,
            secret_reference: "ECNU_API_KEY".into(),
        },
        Arc::new(EnvironmentSecretResolver),
    )
    .unwrap();
    let result = provider
        .translate(&TranslationRequest {
            current: "The sky is blue.".into(),
            recent: VecDeque::from(["A calm morning.".into()]),
            background: None,
            glossary: vec![],
        })
        .await
        .unwrap();
    assert!(!result.translated.trim().is_empty());
    assert!(result.provider_identity.contains("ecnu-plus"));
}
