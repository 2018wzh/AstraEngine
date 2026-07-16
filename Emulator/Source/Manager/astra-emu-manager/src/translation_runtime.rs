use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{mpsc, Arc},
    thread,
};

use astra_core::Hash256;
use astra_emu_manager_core::{TranslationCacheRecord, TranslationProfileRecord};
use astra_emu_translation_openai_compatible::{
    OpenAiCompatibleTranslationProvider, SecretResolver, TranslationEndpointKind,
    TranslationProfile, TranslationProtocol, TranslationRequest,
};

const MAX_SESSION_CACHE_ENTRIES: usize = 4_096;
const MAX_PENDING_REQUESTS: usize = 64;
const MAX_RECENT_SENTENCES: usize = 32;

pub(crate) struct TranslationLaunchConfig {
    pub(crate) case_identity: String,
    pub(crate) profile: Option<TranslationProfileRecord>,
    pub(crate) consent_present: bool,
    pub(crate) persistent_cache_enabled: bool,
    pub(crate) cached: Vec<TranslationCacheRecord>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TranslationOverlayState {
    pub(crate) source: String,
    pub(crate) translated: String,
    pub(crate) status: String,
    pub(crate) endpoint: String,
    pub(crate) model: String,
    pub(crate) sent_scope: String,
}

pub(crate) struct TranslationRuntime {
    case_identity: String,
    provider_identity: String,
    profile: Option<TranslationProfile>,
    background: Option<String>,
    glossary: Vec<(String, String)>,
    sender: Option<mpsc::SyncSender<WorkerCommand>>,
    receiver: Option<mpsc::Receiver<WorkerResult>>,
    worker: Option<thread::JoinHandle<()>>,
    recent: VecDeque<String>,
    pending: BTreeSet<String>,
    session_cache: BTreeMap<(String, String), CachedTranslation>,
    persistent_cache_enabled: bool,
    pending_writes: Vec<TranslationCacheRecord>,
    overlay: TranslationOverlayState,
}

struct CachedTranslation {
    source: String,
    translated: String,
}

struct WorkerRequest {
    source_hash: String,
    request: TranslationRequest,
}

enum WorkerCommand {
    Translate(WorkerRequest),
    ResetCircuit,
}

struct WorkerResult {
    source_hash: String,
    source: String,
    result: Result<WorkerSuccess, String>,
}

struct WorkerSuccess {
    translated: String,
    provider_identity: String,
    sent_sentence_count: usize,
}

impl TranslationRuntime {
    pub(crate) fn open(
        config: TranslationLaunchConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, String> {
        let mut session_cache = BTreeMap::new();
        for record in config.cached {
            if record.case_identity != config.case_identity {
                return Err("ASTRA_EMU_TRANSLATION_CACHE_CASE_MISMATCH".into());
            }
            if session_cache
                .insert(
                    (record.source_hash, record.provider_identity),
                    CachedTranslation {
                        source: record.source_text,
                        translated: record.translated_text,
                    },
                )
                .is_some()
            {
                return Err("ASTRA_EMU_TRANSLATION_CACHE_DUPLICATE".into());
            }
        }
        let mut runtime = Self {
            case_identity: config.case_identity,
            provider_identity: String::new(),
            profile: None,
            background: None,
            glossary: Vec::new(),
            sender: None,
            receiver: None,
            worker: None,
            recent: VecDeque::new(),
            pending: BTreeSet::new(),
            session_cache,
            persistent_cache_enabled: config.persistent_cache_enabled,
            pending_writes: Vec::new(),
            overlay: TranslationOverlayState {
                status: if config.consent_present {
                    "Translation profile is not configured".into()
                } else {
                    "Translation requires global consent".into()
                },
                ..Default::default()
            },
        };
        let Some(record) = config.profile else {
            return Ok(runtime);
        };
        let profile = translation_profile_from_record(&record)?;
        runtime.overlay.endpoint = profile.endpoint.clone();
        runtime.overlay.model = profile.model.clone();
        runtime.overlay.sent_scope = format!(
            "current sentence + up to {} recent sentences; max {} bytes",
            profile.context_sentences, profile.body_limit_bytes
        );
        runtime.background = record.background;
        runtime.glossary = record.glossary;
        runtime.provider_identity = provider_identity(&profile);
        runtime.profile = Some(profile.clone());
        if !config.consent_present {
            return Ok(runtime);
        }
        let (request_tx, request_rx) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
        let (result_tx, result_rx) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
        let worker = thread::Builder::new()
            .name("astra-emu-translation".into())
            .spawn(move || worker_main(profile, secrets, request_rx, result_tx))
            .map_err(|_| "ASTRA_EMU_TRANSLATION_WORKER_CREATE".to_owned())?;
        runtime.sender = Some(request_tx);
        runtime.receiver = Some(result_rx);
        runtime.worker = Some(worker);
        runtime.overlay.status = "Ready".into();
        Ok(runtime)
    }

    pub(crate) fn capture(&mut self, source: String) -> Result<(), String> {
        self.poll()?;
        let source_hash = Hash256::from_sha256(source.as_bytes()).to_string();
        self.overlay.source = source.clone();
        let cache_key = (source_hash.clone(), self.provider_identity.clone());
        if let Some(cached) = self.session_cache.get(&cache_key) {
            if cached.source != source {
                return Err("ASTRA_EMU_TRANSLATION_CACHE_HASH_COLLISION".into());
            }
            self.overlay.translated = cached.translated.clone();
            self.overlay.status = "Cached".into();
            self.remember(source);
            return Ok(());
        }
        let Some(sender) = &self.sender else {
            self.overlay.translated.clear();
            self.remember(source);
            return Ok(());
        };
        if self.pending.contains(&source_hash) {
            self.overlay.status = "Translating".into();
            return Ok(());
        }
        let request = TranslationRequest {
            current: source.clone(),
            recent: self.recent.clone(),
            background: self.background.clone(),
            glossary: self.glossary.clone(),
        };
        sender
            .try_send(WorkerCommand::Translate(WorkerRequest {
                source_hash: source_hash.clone(),
                request,
            }))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "ASTRA_EMU_TRANSLATION_QUEUE_FULL".to_owned(),
                mpsc::TrySendError::Disconnected(_) => {
                    "ASTRA_EMU_TRANSLATION_WORKER_DISCONNECTED".to_owned()
                }
            })?;
        self.pending.insert(source_hash);
        self.overlay.translated.clear();
        self.overlay.status = "Translating".into();
        self.remember(source);
        Ok(())
    }

    pub(crate) fn poll(&mut self) -> Result<(), String> {
        let Some(receiver) = &self.receiver else {
            return Ok(());
        };
        while let Ok(result) = receiver.try_recv() {
            if !self.pending.remove(&result.source_hash) {
                return Err("ASTRA_EMU_TRANSLATION_RESULT_NOT_PENDING".into());
            }
            match result.result {
                Ok(success) => {
                    if success.provider_identity != self.provider_identity {
                        return Err("ASTRA_EMU_TRANSLATION_PROVIDER_IDENTITY".into());
                    }
                    if self.session_cache.len() >= MAX_SESSION_CACHE_ENTRIES
                        && !self.session_cache.contains_key(&(
                            result.source_hash.clone(),
                            success.provider_identity.clone(),
                        ))
                    {
                        let oldest = self
                            .session_cache
                            .keys()
                            .next()
                            .cloned()
                            .ok_or_else(|| "ASTRA_EMU_TRANSLATION_CACHE_STATE".to_owned())?;
                        self.session_cache.remove(&oldest);
                    }
                    self.session_cache.insert(
                        (
                            result.source_hash.clone(),
                            success.provider_identity.clone(),
                        ),
                        CachedTranslation {
                            source: result.source.clone(),
                            translated: success.translated.clone(),
                        },
                    );
                    if self.persistent_cache_enabled {
                        self.pending_writes.push(TranslationCacheRecord {
                            case_identity: self.case_identity.clone(),
                            source_hash: result.source_hash.clone(),
                            source_text: result.source.clone(),
                            translated_text: success.translated.clone(),
                            provider_identity: success.provider_identity,
                        });
                    }
                    if self.overlay.source == result.source {
                        self.overlay.translated = success.translated;
                        self.overlay.status = format!(
                            "Translated with {} context sentences",
                            success.sent_sentence_count
                        );
                    }
                }
                Err(code) => {
                    if self.overlay.source == result.source {
                        self.overlay.translated.clear();
                        self.overlay.status = code;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn overlay(&self) -> &TranslationOverlayState {
        &self.overlay
    }

    pub(crate) fn take_pending_writes(&mut self) -> Vec<TranslationCacheRecord> {
        std::mem::take(&mut self.pending_writes)
    }

    pub(crate) fn reset_circuit(&mut self) -> Result<(), String> {
        let Some(sender) = &self.sender else {
            return Err("ASTRA_EMU_TRANSLATION_PROVIDER_NOT_ACTIVE".into());
        };
        sender
            .try_send(WorkerCommand::ResetCircuit)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "ASTRA_EMU_TRANSLATION_QUEUE_FULL".to_owned(),
                mpsc::TrySendError::Disconnected(_) => {
                    "ASTRA_EMU_TRANSLATION_WORKER_DISCONNECTED".to_owned()
                }
            })?;
        self.overlay.status = "Manual recovery requested".into();
        Ok(())
    }

    fn remember(&mut self, source: String) {
        if self.recent.back() != Some(&source) {
            self.recent.push_back(source);
        }
        while self.recent.len() > MAX_RECENT_SENTENCES {
            self.recent.pop_front();
        }
    }
}

impl Drop for TranslationRuntime {
    fn drop(&mut self) {
        self.sender.take();
        // Dropping the handle detaches a request that is already in flight.
        // The HTTP timeout is bounded by the validated profile and the closed
        // result channel makes the worker terminate immediately afterwards.
        self.worker.take();
    }
}

fn worker_main(
    profile: TranslationProfile,
    secrets: Arc<dyn SecretResolver>,
    receiver: mpsc::Receiver<WorkerCommand>,
    sender: mpsc::SyncSender<WorkerResult>,
) {
    let result = (|| -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "ASTRA_EMU_TRANSLATION_RUNTIME_CREATE".to_owned())?;
        let provider = OpenAiCompatibleTranslationProvider::new(profile, secrets)
            .map_err(|error| error.to_string())?;
        for command in receiver {
            match command {
                WorkerCommand::ResetCircuit => runtime.block_on(provider.reset_circuit_by_user()),
                WorkerCommand::Translate(request) => {
                    let source = request.request.current.clone();
                    let result = runtime
                        .block_on(provider.translate(&request.request))
                        .map(|value| WorkerSuccess {
                            translated: value.translated,
                            provider_identity: value.provider_identity,
                            sent_sentence_count: value.sent_sentence_count,
                        })
                        .map_err(|error| error.to_string());
                    if sender
                        .send(WorkerResult {
                            source_hash: request.source_hash,
                            source,
                            result,
                        })
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    })();
    if let Err(code) = result {
        tracing::error!(
            event = "astra.emu.translation.worker_failed",
            diagnostic_code = %code
        );
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
    let profile = TranslationProfile {
        profile_id: record.profile_id.clone(),
        endpoint_kind,
        endpoint: record.endpoint.clone(),
        protocol,
        model: record.model.clone(),
        target_language: record.target_language.clone(),
        context_sentences: record.context_sentences,
        body_limit_bytes: record.body_limit_bytes,
        timeout_ms: record.timeout_ms,
        secret_reference: record.secret_reference.clone(),
    };
    profile.validate().map_err(|error| error.to_string())?;
    Ok(profile)
}

fn provider_identity(profile: &TranslationProfile) -> String {
    profile.provider_identity()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UnavailableSecrets;

    impl SecretResolver for UnavailableSecrets {
        fn resolve(
            &self,
            _reference: &str,
        ) -> Result<String, astra_emu_translation_openai_compatible::TranslationError> {
            Err(astra_emu_translation_openai_compatible::TranslationError::SecretUnavailable)
        }
    }

    fn unavailable_secrets() -> Arc<dyn SecretResolver> {
        Arc::new(UnavailableSecrets)
    }

    #[test]
    fn missing_consent_preserves_source_without_starting_network_worker() {
        let mut runtime = TranslationRuntime::open(
            TranslationLaunchConfig {
                case_identity: "case.test".into(),
                profile: Some(profile()),
                consent_present: false,
                persistent_cache_enabled: false,
                cached: Vec::new(),
            },
            unavailable_secrets(),
        )
        .unwrap();
        runtime.capture("original".into()).unwrap();
        assert_eq!(runtime.overlay().source, "original");
        assert!(runtime.overlay().translated.is_empty());
        assert!(runtime.sender.is_none());
    }

    #[test]
    fn persistent_cache_is_hash_bound_and_used_without_network() {
        let source = "original";
        let hash = Hash256::from_sha256(source.as_bytes()).to_string();
        let mut runtime = TranslationRuntime::open(
            TranslationLaunchConfig {
                case_identity: "case.test".into(),
                profile: Some(profile()),
                consent_present: false,
                persistent_cache_enabled: true,
                cached: vec![TranslationCacheRecord {
                    case_identity: "case.test".into(),
                    source_hash: hash,
                    source_text: source.into(),
                    translated_text: "translated".into(),
                    provider_identity: "ecnu-openai-compatible:test".into(),
                }],
            },
            unavailable_secrets(),
        )
        .unwrap();
        runtime.capture(source.into()).unwrap();
        assert_eq!(runtime.overlay().translated, "translated");
        assert_eq!(runtime.overlay().status, "Cached");
    }

    fn profile() -> TranslationProfileRecord {
        TranslationProfileRecord {
            profile_id: "test".into(),
            endpoint_kind: "ecnu".into(),
            endpoint: "https://chat.ecnu.edu.cn/open/api/v1".into(),
            protocol: "responses".into(),
            model: "test-model".into(),
            target_language: "zh-CN".into(),
            context_sentences: 10,
            body_limit_bytes: 16 * 1024,
            timeout_ms: 30_000,
            secret_reference: "ecnu.test".into(),
            background: None,
            glossary: Vec::new(),
        }
    }
}
