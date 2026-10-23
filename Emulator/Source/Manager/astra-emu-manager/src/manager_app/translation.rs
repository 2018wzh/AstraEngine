use super::*;
use astra_emu_manager_ui_slint::TranslationFormViewModel;
use astra_emu_translation::TranslationError;
use std::sync::Arc;

const TEST_SECRET_REFERENCE: &str = "translation.test";

struct FormSecret(String);

impl SecretResolver for FormSecret {
    fn resolve(&self, _reference: &str) -> Result<String, TranslationError> {
        Ok(self.0.clone())
    }
}

impl AstraEmuManagerController {
    pub(super) fn test_translation_connection(
        &mut self,
        form: &TranslationFormViewModel,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        self.require_network()?;
        self.connection_test.take();
        let mut profile = profile_from_form(form)?;
        let secrets: Arc<dyn SecretResolver> = if secret.is_empty() {
            profile.secret_reference = self
                .profile()?
                .ok_or("ASTRA_EMU_TRANSLATION_SECRET_REQUIRED")?
                .secret_reference;
            Arc::new(ManagerSecretStore::open().map_err(|error| error.to_string())?)
        } else {
            profile.secret_reference = TEST_SECRET_REFERENCE.into();
            Arc::new(FormSecret(secret.to_owned()))
        };
        let wake = self
            .host_wake
            .clone()
            .ok_or("ASTRA_EMU_HOST_WAKE_REQUIRED")?;
        self.connection_test = Some(connection_test::ConnectionTest::start(
            profile, secrets, wake,
        )?);
        self.translation_status = "正在测试翻译服务连接…".into();
        self.model()
    }

    pub(super) fn poll_connection_test(&mut self) -> bool {
        let result = self.connection_test.as_ref().and_then(|test| test.poll());
        let Some(result) = result else {
            return false;
        };
        self.connection_test.take();
        match result {
            Ok(result) => {
                self.translation_status = format!(
                    "连接成功（{} ms，{} 个可用模型）",
                    result.latency_ms,
                    result.models.len()
                );
                self.translation_models = result.models;
            }
            Err(error) => {
                self.translation_status = format!("连接失败：{error}");
            }
        }
        true
    }

    pub(super) fn profile(&self) -> Result<Option<TranslationProfile>, String> {
        self.library
            .translation_profile()
            .map_err(|error| error.to_string())
    }

    pub(super) fn save_translation_profile(
        &mut self,
        form: &TranslationFormViewModel,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        if self.session_running() {
            return Err("ASTRA_EMU_TRANSLATION_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        self.connection_test.take();
        let previous = self.profile()?;
        let mut profile = profile_from_form(form)?;
        let store = ManagerSecretStore::open().map_err(|error| error.to_string())?;
        if secret.is_empty() {
            profile.secret_reference = previous
                .as_ref()
                .ok_or("ASTRA_EMU_TRANSLATION_SECRET_REQUIRED")?
                .secret_reference
                .clone();
            store
                .resolve(&profile.secret_reference)
                .map_err(|_| "ASTRA_EMU_TRANSLATION_SECRET_REQUIRED".to_owned())?;
        } else {
            profile.secret_reference = if previous
                .as_ref()
                .is_some_and(|p| p.secret_reference == "translation.a")
            {
                "translation.b".into()
            } else {
                "translation.a".into()
            };
            store
                .store(&profile.secret_reference, secret)
                .map_err(|error| error.to_string())?;
        }
        if let Err(error) = self.library.set_translation_profile(&profile) {
            if !secret.is_empty() {
                store
                    .delete(&profile.secret_reference)
                    .map_err(|cleanup| format!("{error}; {cleanup}"))?;
            }
            return Err(error.to_string());
        }
        if !secret.is_empty() {
            if let Some(previous) = previous {
                store
                    .delete(&previous.secret_reference)
                    .map_err(|_| "ASTRA_EMU_TRANSLATION_SAVED_OLD_SECRET_CLEANUP_FAILED")?;
            }
        }
        self.translation_status = "翻译配置已保存".into();
        self.model()
    }

    pub(super) fn set_translation_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        if enabled {
            self.require_network()?;
            self.profile()?
                .ok_or("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED")?;
        }
        self.library
            .set_app_state(TRANSLATION_ENABLED_KEY, &enabled)
            .map_err(|error| error.to_string())?;
        self.translation_enabled = enabled;
        self.model()
    }

    /// The live translation profile for a new session, or `None` when the
    /// network permission, the translation switch or the profile is missing.
    pub(super) fn launch_translation_profile(&self) -> Result<Option<TranslationProfile>, String> {
        if !self.network_allowed || !self.translation_enabled {
            return Ok(None);
        }
        self.profile()
    }
}

pub(super) fn profile_from_form(
    form: &TranslationFormViewModel,
) -> Result<TranslationProfile, String> {
    let provider = match form.provider.as_str() {
        "openai" => TranslationProvider::OpenAi,
        "anthropic" => TranslationProvider::Anthropic,
        "google" => TranslationProvider::Google,
        _ => return Err("ASTRA_EMU_TRANSLATION_PROVIDER_INVALID".into()),
    };
    let openai_api = match form.openai_api.as_str() {
        "responses" => OpenAiApi::Responses,
        "chat_completions" => OpenAiApi::ChatCompletions,
        _ => return Err("ASTRA_EMU_TRANSLATION_PROTOCOL_INVALID".into()),
    };
    let profile = TranslationProfile {
        profile_id: "translation.default".into(),
        provider,
        base_url: form.base_url.trim().trim_end_matches('/').into(),
        openai_api,
        model: form.model.trim().into(),
        target_language: form.language.trim().into(),
        timeout_ms: u64::try_from(form.timeout_s)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or("ASTRA_EMU_TRANSLATION_TIMEOUT_INVALID")?,
        secret_reference: "translation.default".into(),
        custom_prompt: form.custom_prompt.clone(),
        context_segments: u32::try_from(form.context_segments)
            .map_err(|_| "ASTRA_EMU_TRANSLATION_CONTEXT_INVALID")?,
        context_chars: u32::try_from(form.context_chars)
            .map_err(|_| "ASTRA_EMU_TRANSLATION_CONTEXT_INVALID")?,
    };
    profile.validate().map_err(|error| error.to_string())?;
    Ok(profile)
}

pub(super) fn profile_form(profile: Option<&TranslationProfile>) -> TranslationFormViewModel {
    let defaults = TranslationProfile::default();
    let profile = profile.unwrap_or(&defaults);
    TranslationFormViewModel {
        provider: profile.provider.as_str().into(),
        base_url: profile.base_url.clone(),
        openai_api: match profile.openai_api {
            OpenAiApi::Responses => "responses".into(),
            OpenAiApi::ChatCompletions => "chat_completions".into(),
        },
        model: profile.model.clone(),
        language: profile.target_language.clone(),
        timeout_s: i32::try_from(profile.timeout_ms / 1000).unwrap_or(i32::MAX),
        custom_prompt: profile.custom_prompt.clone(),
        context_segments: i32::try_from(profile.context_segments).unwrap_or(i32::MAX),
        context_chars: i32::try_from(profile.context_chars).unwrap_or(i32::MAX),
    }
}
