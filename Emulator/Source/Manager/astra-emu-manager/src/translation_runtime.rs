use std::{collections::VecDeque, sync::Arc, time::Duration};

use astra_byte_source::OwnedByteBuffer;
use astra_emu_extension_api::TRANSLATION_TEXT_HOOK_ID;
use astra_emu_family_api::{
    LegacyDiagnostic, LegacyHookInvocationV1, LegacyHookResultV1, LegacyHookStatusV1,
    LegacyProviderError,
};
use astra_emu_manager_core::{SynchronousFamilyHookProvider, TranslationProfileRecord};
use astra_emu_translation_openai_compatible::{
    OpenAiCompatibleTranslationProvider, SecretResolver, TranslationEndpointKind, TranslationError,
    TranslationProfile, TranslationProtocol, TranslationRequest,
};

pub(crate) struct TranslationLaunchConfig {
    pub(crate) case_identity: String,
    pub(crate) profile: Option<TranslationProfileRecord>,
    pub(crate) consent_present: bool,
}

pub(crate) struct TranslationRuntime {
    provider: Option<Arc<TranslationHookProvider>>,
}

impl TranslationRuntime {
    pub(crate) fn open(
        config: TranslationLaunchConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, String> {
        if config.case_identity.is_empty() {
            return Err("ASTRA_EMU_TRANSLATION_CASE_IDENTITY".into());
        }
        let provider = match (config.profile, config.consent_present) {
            (Some(record), true) => {
                let background = record.background.clone();
                let glossary = record.glossary.clone();
                let profile = translation_profile_from_record(&record)?;
                let timeout_ms = u32::try_from(profile.timeout_ms)
                    .map_err(|_| "ASTRA_EMU_TRANSLATION_TIMEOUT_REPRESENTATION")?;
                let provider = OpenAiCompatibleTranslationProvider::new(profile, secrets)
                    .map_err(|error| error.to_string())?;
                Some(Arc::new(TranslationHookProvider {
                    provider,
                    timeout_ms,
                    background,
                    glossary,
                }))
            }
            _ => None,
        };
        Ok(Self { provider })
    }

    pub(crate) fn hook_provider(&self) -> Option<(u32, Arc<dyn SynchronousFamilyHookProvider>)> {
        self.provider.as_ref().map(|provider| {
            (
                provider.timeout_ms,
                provider.clone() as Arc<dyn SynchronousFamilyHookProvider>,
            )
        })
    }
}

struct TranslationHookProvider {
    provider: OpenAiCompatibleTranslationProvider<dyn SecretResolver>,
    timeout_ms: u32,
    background: Option<String>,
    glossary: Vec<(String, String)>,
}

impl SynchronousFamilyHookProvider for TranslationHookProvider {
    fn provider_id(&self) -> &str {
        "astra.emu.translation.openai_compatible"
    }

    fn invoke(
        &self,
        invocation: &LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        if invocation.hook_id != TRANSLATION_TEXT_HOOK_ID {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_TRANSLATION_HOOK_ID",
                "translation provider received an unsupported hook identity",
            ));
        }
        if invocation.timeout_ms != self.timeout_ms {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_TRANSLATION_TIMEOUT_BINDING",
                "translation timeout differs from the explicit provider binding",
            ));
        }
        if invocation.timeout_ms == 0 {
            return Ok(failed_hook(
                LegacyHookStatusV1::TimedOut,
                "ASTRA_EMU_TRANSLATION_TIMEOUT",
            ));
        }
        let source = std::str::from_utf8(invocation.payload.as_slice()).map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_TRANSLATION_REQUEST_UTF8",
                "translation request is not valid UTF-8",
            )
        })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_TRANSLATION_RUNTIME_CREATE",
                    "translation runtime could not be created",
                )
            })?;
        let request = TranslationRequest {
            current: source.to_owned(),
            recent: VecDeque::new(),
            background: self.background.clone(),
            glossary: self.glossary.clone(),
        };
        match runtime.block_on(tokio::time::timeout(
            Duration::from_millis(u64::from(invocation.timeout_ms)),
            self.provider.translate(&request),
        )) {
            Err(_) => Ok(failed_hook(
                LegacyHookStatusV1::TimedOut,
                "ASTRA_EMU_TRANSLATION_TIMEOUT",
            )),
            Ok(Ok(result)) if !result.translated.is_empty() => Ok(LegacyHookResultV1 {
                status: LegacyHookStatusV1::Completed,
                payload: OwnedByteBuffer::from_vec(result.translated.into_bytes()),
                diagnostics: Vec::new(),
            }),
            Ok(Ok(_)) => Ok(failed_hook(
                LegacyHookStatusV1::Failed,
                "ASTRA_EMU_TRANSLATION_PROTOCOL",
            )),
            Ok(Err(error)) => Ok(translation_failure(error)),
        }
    }
}

fn translation_failure(error: TranslationError) -> LegacyHookResultV1 {
    let (status, code) = match error {
        TranslationError::Timeout => (
            LegacyHookStatusV1::TimedOut,
            "ASTRA_EMU_TRANSLATION_TIMEOUT",
        ),
        TranslationError::SecretUnavailable
        | TranslationError::Http {
            status: 401 | 403, ..
        } => (LegacyHookStatusV1::Failed, "ASTRA_EMU_TRANSLATION_AUTH"),
        TranslationError::RateLimited | TranslationError::Http { status: 429, .. } => (
            LegacyHookStatusV1::Failed,
            "ASTRA_EMU_TRANSLATION_RATE_LIMITED",
        ),
        TranslationError::Protocol(_) => {
            (LegacyHookStatusV1::Failed, "ASTRA_EMU_TRANSLATION_PROTOCOL")
        }
        TranslationError::Profile(_) => {
            (LegacyHookStatusV1::Failed, "ASTRA_EMU_TRANSLATION_PROFILE")
        }
        TranslationError::CircuitOpen
        | TranslationError::Transport(_)
        | TranslationError::Http { .. } => {
            (LegacyHookStatusV1::Failed, "ASTRA_EMU_TRANSLATION_NETWORK")
        }
    };
    failed_hook(status, code)
}

fn failed_hook(status: LegacyHookStatusV1, code: &str) -> LegacyHookResultV1 {
    LegacyHookResultV1 {
        status,
        payload: OwnedByteBuffer::from_vec(Vec::new()),
        diagnostics: vec![LegacyDiagnostic {
            code: code.into(),
            severity: "warn".into(),
            message: "translation hook failed; the family core must retain the original text"
                .into(),
            subject: "translation".into(),
        }],
    }
}

pub(crate) fn translation_profile_from_record(
    record: &TranslationProfileRecord,
) -> Result<TranslationProfile, String> {
    let endpoint_kind = match record.endpoint_kind.as_str() {
        "ecnu" => TranslationEndpointKind::Ecnu,
        "openai" => TranslationEndpointKind::OpenAi,
        "third_party" => TranslationEndpointKind::ThirdParty,
        _ => return Err("ASTRA_EMU_TRANSLATION_ENDPOINT_KIND".into()),
    };
    let protocol = match record.protocol.as_str() {
        "responses" => TranslationProtocol::Responses,
        "chat_completions" => TranslationProtocol::ChatCompletions,
        _ => return Err("ASTRA_EMU_TRANSLATION_PROTOCOL".into()),
    };
    let timeout_ms = u32::try_from(record.timeout_ms)
        .map_err(|_| "ASTRA_EMU_TRANSLATION_TIMEOUT_REPRESENTATION")?;
    Ok(TranslationProfile {
        profile_id: record.profile_id.clone(),
        endpoint_kind,
        endpoint: record.endpoint.clone(),
        protocol,
        model: record.model.clone(),
        target_language: record.target_language.clone(),
        context_sentences: record.context_sentences,
        body_limit_bytes: record.body_limit_bytes,
        timeout_ms: u64::from(timeout_ms),
        secret_reference: record.secret_reference.clone(),
    })
}
