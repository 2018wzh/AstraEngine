use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use thiserror::Error;
use tokio::task::AbortHandle;

use crate::{
    model::{TranslationError, TranslationRequest, TranslationResult, TranslationSegment},
    TranslationSession, MAX_CONTEXT_CHARS, MAX_CONTEXT_SEGMENTS,
};

const MAX_INFLIGHT_REQUESTS: usize = 8;

/// Result of polling one asynchronous translation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslationPoll {
    Pending,
    Ready(TranslationResult),
    Cancelled,
    Failed(TranslationError),
}

/// Errors produced by the host-side asynchronous service before a provider
/// request is started or after a request has already been consumed.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TranslationServiceError {
    #[error("ASTRA_EMU_TRANSLATION_SERVICE_REQUEST_ID")]
    InvalidRequestId,
    #[error("ASTRA_EMU_TRANSLATION_SERVICE_REQUEST: {0}")]
    InvalidRequest(TranslationError),
    #[error("ASTRA_EMU_TRANSLATION_SERVICE_DUPLICATE_REQUEST")]
    DuplicateRequest,
    #[error("ASTRA_EMU_TRANSLATION_SERVICE_INFLIGHT_LIMIT")]
    InflightLimit,
    #[error("ASTRA_EMU_TRANSLATION_SERVICE_UNKNOWN_REQUEST")]
    UnknownRequest,
}

struct RequestEntry {
    generation: u64,
    state: TranslationPoll,
    abort: Option<AbortHandle>,
}

struct ServiceState {
    generation: u64,
    requests: HashMap<String, RequestEntry>,
    context: VecDeque<TranslationSegment>,
    context_chars: usize,
}

/// Host-side asynchronous translation service for one live game session.
///
/// It owns the bounded source context and the non-persistent translation
/// cache. Every spawned request captures the service generation; a reset or
/// configuration change marks all older requests cancelled and prevents a
/// late response from being published into the new session.
pub struct AsyncTranslationService<R: ?Sized> {
    session: Arc<TranslationSession<R>>,
    handle: tokio::runtime::Handle,
    state: Arc<Mutex<ServiceState>>,
}

impl<R: crate::SecretResolver + ?Sized + 'static> AsyncTranslationService<R> {
    pub fn new(session: Arc<TranslationSession<R>>, handle: tokio::runtime::Handle) -> Self {
        Self {
            session,
            handle,
            state: Arc::new(Mutex::new(ServiceState {
                generation: 0,
                requests: HashMap::new(),
                context: VecDeque::new(),
                context_chars: 0,
            })),
        }
    }

    /// Submit one current segment. The service supplies the bounded recent
    /// context and records the current source before starting the request.
    pub fn submit(
        &self,
        request_id: impl Into<String>,
        current: TranslationSegment,
    ) -> Result<(), TranslationServiceError> {
        let request_id = request_id.into();
        validate_request_id(&request_id)?;
        let mut state = self
            .state
            .lock()
            .expect("translation service mutex poisoned");
        if state.requests.contains_key(&request_id) {
            return Err(TranslationServiceError::DuplicateRequest);
        }
        if state.requests.len() >= MAX_INFLIGHT_REQUESTS {
            return Err(TranslationServiceError::InflightLimit);
        }
        let request = TranslationRequest {
            current: current.clone(),
            recent: state.context.iter().cloned().collect(),
        };
        request
            .validate()
            .map_err(TranslationServiceError::InvalidRequest)?;
        append_context(&mut state, current);
        let generation = state.generation;
        state.requests.insert(
            request_id.clone(),
            RequestEntry {
                generation,
                state: TranslationPoll::Pending,
                abort: None,
            },
        );
        drop(state);

        let session = Arc::clone(&self.session);
        let shared = Arc::clone(&self.state);
        let task_id = request_id.clone();
        let abort = self.handle.spawn(async move {
            let result = session.translate(&request).await;
            let mut state = shared.lock().expect("translation service mutex poisoned");
            let Some(entry) = state.requests.get_mut(&task_id) else {
                return;
            };
            if entry.generation != generation || !matches!(entry.state, TranslationPoll::Pending) {
                return;
            }
            entry.state = match result {
                Ok(result) => TranslationPoll::Ready(result),
                Err(TranslationError::SessionReset) => TranslationPoll::Cancelled,
                Err(error) => TranslationPoll::Failed(error),
            };
        });
        let abort_handle = abort.abort_handle();
        let mut state = self
            .state
            .lock()
            .expect("translation service mutex poisoned");
        if let Some(entry) = state.requests.get_mut(&request_id) {
            if entry.generation == generation && matches!(entry.state, TranslationPoll::Pending) {
                entry.abort = Some(abort_handle);
                return Ok(());
            }
        }
        abort.abort();
        Ok(())
    }

    /// Poll without waiting or entering the provider runtime.
    pub fn poll(&self, request_id: &str) -> Result<TranslationPoll, TranslationServiceError> {
        validate_request_id(request_id)?;
        let mut state = self
            .state
            .lock()
            .expect("translation service mutex poisoned");
        let Some(entry) = state.requests.get(request_id) else {
            return Err(TranslationServiceError::UnknownRequest);
        };
        if matches!(entry.state, TranslationPoll::Pending) {
            return Ok(TranslationPoll::Pending);
        }
        let entry = state
            .requests
            .remove(request_id)
            .expect("translation service request disappeared while locked");
        Ok(entry.state)
    }

    /// Cancel a request. A cancelled request remains pollable until its
    /// terminal state is consumed, which makes reset and shutdown observable.
    pub fn cancel(&self, request_id: &str) -> Result<(), TranslationServiceError> {
        validate_request_id(request_id)?;
        let mut state = self
            .state
            .lock()
            .expect("translation service mutex poisoned");
        let Some(entry) = state.requests.get_mut(request_id) else {
            return Err(TranslationServiceError::UnknownRequest);
        };
        if matches!(entry.state, TranslationPoll::Pending) {
            if let Some(abort) = entry.abort.take() {
                abort.abort();
            }
            entry.state = TranslationPoll::Cancelled;
        }
        Ok(())
    }

    /// Invalidate all in-flight requests and clear both source context and
    /// translated results for the current game session.
    pub fn reset(&self) {
        let mut state = self
            .state
            .lock()
            .expect("translation service mutex poisoned");
        state.generation = state
            .generation
            .checked_add(1)
            .expect("translation service generation exhausted");
        for entry in state.requests.values_mut() {
            if let Some(abort) = entry.abort.take() {
                abort.abort();
            }
            if matches!(entry.state, TranslationPoll::Pending) {
                entry.state = TranslationPoll::Cancelled;
            }
        }
        state.context.clear();
        state.context_chars = 0;
        self.session.clear();
    }

    pub fn cache_len(&self) -> usize {
        self.session.cache_len()
    }
}

fn validate_request_id(request_id: &str) -> Result<(), TranslationServiceError> {
    if request_id.is_empty()
        || request_id.chars().count() > 256
        || request_id.chars().any(char::is_control)
    {
        return Err(TranslationServiceError::InvalidRequestId);
    }
    Ok(())
}

fn append_context(state: &mut ServiceState, segment: TranslationSegment) {
    let chars = segment.rendered().chars().count();
    state.context.push_back(segment);
    state.context_chars = state.context_chars.saturating_add(chars);
    while state.context.len() > MAX_CONTEXT_SEGMENTS || state.context_chars > MAX_CONTEXT_CHARS {
        let Some(old) = state.context.pop_front() else {
            break;
        };
        state.context_chars = state
            .context_chars
            .saturating_sub(old.rendered().chars().count());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        OpenAiCompatibleTranslationProvider, SecretResolver, TranslationEndpointKind,
        TranslationProfile, TranslationProtocol,
    };

    struct TestSecrets;

    impl SecretResolver for TestSecrets {
        fn resolve(&self, _reference: &str) -> Result<String, TranslationError> {
            Ok("secret".into())
        }
    }

    fn service() -> (
        tokio::runtime::Runtime,
        AsyncTranslationService<TestSecrets>,
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let profile = TranslationProfile {
            profile_id: "test".into(),
            endpoint_kind: TranslationEndpointKind::OpenAiCompatible,
            endpoint: "https://example.com/v1".into(),
            protocol: TranslationProtocol::Responses,
            model: "model".into(),
            target_language: "zh-CN".into(),
            timeout_ms: 1_000,
            secret_reference: "test.secret".into(),
        };
        let provider = Arc::new(
            OpenAiCompatibleTranslationProvider::new(profile, Arc::new(TestSecrets)).unwrap(),
        );
        let session = Arc::new(TranslationSession::new(provider));
        let service = AsyncTranslationService::new(session, runtime.handle().clone());
        (runtime, service)
    }

    #[test]
    fn service_rejects_duplicate_and_invalid_request_ids() {
        let (_runtime, service) = service();
        assert_eq!(
            service.submit("", TranslationSegment::new("x")),
            Err(TranslationServiceError::InvalidRequestId)
        );
        service.submit("one", TranslationSegment::new("x")).unwrap();
        assert_eq!(
            service.submit("one", TranslationSegment::new("y")),
            Err(TranslationServiceError::DuplicateRequest)
        );
        service.cancel("one").unwrap();
    }

    #[test]
    fn reset_marks_pending_requests_cancelled_and_clears_cache() {
        let (_runtime, service) = service();
        service.submit("one", TranslationSegment::new("x")).unwrap();
        service.reset();
        assert_eq!(service.poll("one").unwrap(), TranslationPoll::Cancelled);
        assert_eq!(service.cache_len(), 0);
    }
}
