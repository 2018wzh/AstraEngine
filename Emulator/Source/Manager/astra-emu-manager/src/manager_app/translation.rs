use super::*;

impl AstraEmuManagerController {
    pub(super) fn test_translation_connection(&mut self) -> Result<ManagerViewModel, String> {
        self.connection_test.take();
        let profile = self
            .profile()?
            .ok_or("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED")?;
        let wake = self
            .host_wake
            .clone()
            .ok_or("ASTRA_EMU_HOST_WAKE_REQUIRED")?;
        self.connection_test = Some(connection_test::ConnectionTest::start(profile, wake)?);
        self.diagnostic = "正在测试翻译服务连接…".into();
        self.model()
    }

    pub(super) fn poll_connection_test(&mut self) -> bool {
        let result = self.connection_test.as_ref().and_then(|test| test.poll());
        let Some(result) = result else {
            return false;
        };
        self.connection_test.take();
        self.diagnostic = match result {
            Ok(ms) => format!("连接成功（{ms} ms）"),
            Err(error) => format!("连接失败：{error}"),
        };
        true
    }
    pub(super) fn profile(&self) -> Result<Option<TranslationProfile>, String> {
        self.library
            .translation_profile()
            .map_err(|error| error.to_string())
    }

    pub(super) fn profile_from_form(
        &self,
        endpoint_kind: &str,
        endpoint: &str,
        protocol: &str,
        model: &str,
        target_language: &str,
        timeout_ms: i32,
    ) -> Result<TranslationProfile, String> {
        let endpoint_kind = match endpoint_kind {
            "openai-compatible" | "openai_compatible" => TranslationEndpointKind::OpenAiCompatible,
            "openai" => TranslationEndpointKind::OpenAi,
            "ecnu" => TranslationEndpointKind::Ecnu,
            "third_party" | "third-party" => TranslationEndpointKind::ThirdParty,
            _ => return Err("ASTRA_EMU_TRANSLATION_ENDPOINT_KIND_INVALID".into()),
        };
        let protocol = match protocol {
            "responses" => TranslationProtocol::Responses,
            "chat_completions" => TranslationProtocol::ChatCompletions,
            _ => return Err("ASTRA_EMU_TRANSLATION_PROTOCOL_INVALID".into()),
        };
        let profile = TranslationProfile {
            profile_id: "translation.default".into(),
            endpoint_kind,
            endpoint: endpoint.trim_end_matches('/').into(),
            protocol,
            model: model.trim().into(),
            target_language: target_language.trim().into(),
            timeout_ms: u64::try_from(timeout_ms)
                .map_err(|_| "ASTRA_EMU_TRANSLATION_TIMEOUT_INVALID")?,
            secret_reference: "translation.default".into(),
        };
        profile.validate().map_err(|error| error.to_string())?;
        Ok(profile)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn save_translation_profile(
        &mut self,
        endpoint_kind: &str,
        endpoint: &str,
        protocol: &str,
        model: &str,
        target_language: &str,

        timeout_ms: i32,

        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_TRANSLATION_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        self.connection_test.take();
        let previous = self.profile()?;
        let mut profile = self.profile_from_form(
            endpoint_kind,
            endpoint,
            protocol,
            model,
            target_language,
            timeout_ms,
        )?;
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
        self.translation_consent = false;
        if !secret.is_empty() {
            if let Some(previous) = previous {
                store
                    .delete(&previous.secret_reference)
                    .map_err(|_| "ASTRA_EMU_TRANSLATION_SAVED_OLD_SECRET_CLEANUP_FAILED")?;
            }
        }
        self.diagnostic.clear();
        self.model()
    }

    pub(super) fn grant_translation_consent(&mut self) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_TRANSLATION_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        let profile = self
            .profile()?
            .ok_or_else(|| "ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED".to_owned())?;
        ManagerSecretStore::open()
            .map_err(|error| error.to_string())?
            .resolve(&profile.secret_reference)
            .map_err(|_| "ASTRA_EMU_TRANSLATION_SECRET_REQUIRED".to_owned())?;
        self.translation_consent = true;
        self.model()
    }
}
