use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    FamilyError, TextPollResult, TextReplacementRequest, TextReplacementResponse,
    TextReplacementService, TextReplacementServiceBox, TextReplacementService_TO, TextResetReason,
};
use astra_emu_translation_openai_compatible::{
    AsyncTranslationService, OpenAiCompatibleTranslationProvider, SecretResolver, TranslationError,
    TranslationPoll, TranslationProfile, TranslationSegment, TranslationServiceError,
    TranslationSession,
};
use tokio::runtime::{Builder, Runtime};

/// Host-owned translation bridge for one family session.
///
/// The bridge owns the Tokio runtime and the bounded translation service. The
/// ABI sink only holds an `Arc` to the bridge state, so cloning the sink does
/// not clone a runtime or a provider session.
pub(crate) struct TextReplacementBridge {
    state: Arc<TextBridgeState>,
}

struct TextBridgeState {
    closed: AtomicBool,
    inner: Mutex<Option<BridgeRuntime>>,
}

/// Keep `service` before `_runtime`: struct fields are dropped in declaration
/// order, so the service drops before its runtime handle is released.
struct BridgeRuntime {
    service: AsyncTranslationService<dyn SecretResolver>,
    _runtime: Runtime,
}

struct TextReplacementSink {
    state: Arc<TextBridgeState>,
}

impl TextReplacementBridge {
    /// Construct a session-owned bridge from an already selected profile and
    /// host secret resolver. Profile validation happens before any runtime or
    /// provider state is created.
    pub(crate) fn new(
        profile: TranslationProfile,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, FamilyError> {
        profile.validate().map_err(profile_error)?;
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|_| error("ASTRA_EMU_TEXT_RUNTIME_CREATE"))?;
        Self::from_runtime(profile, secrets, runtime)
    }

    #[cfg(test)]
    fn inert_for_test(
        profile: TranslationProfile,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, FamilyError> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| error("ASTRA_EMU_TEXT_RUNTIME_CREATE"))?;
        Self::from_runtime(profile, secrets, runtime)
    }

    fn from_runtime(
        profile: TranslationProfile,
        secrets: Arc<dyn SecretResolver>,
        runtime: Runtime,
    ) -> Result<Self, FamilyError> {
        let provider = OpenAiCompatibleTranslationProvider::new(profile, secrets)
            .map_err(translation_error)?;
        let session = Arc::new(TranslationSession::new(Arc::new(provider)));
        let service = AsyncTranslationService::new(session, runtime.handle().clone());
        Ok(Self {
            state: Arc::new(TextBridgeState {
                closed: AtomicBool::new(false),
                inner: Mutex::new(Some(BridgeRuntime {
                    service,
                    _runtime: runtime,
                })),
            }),
        })
    }

    /// Return a cloneable ABI sink. Each call creates a thin ABI wrapper over
    /// the same host-owned session state.
    pub(crate) fn sink(&self) -> TextReplacementServiceBox {
        TextReplacementService_TO::from_value(
            TextReplacementSink {
                state: Arc::clone(&self.state),
            },
            TD_Opaque,
        )
    }

    /// Cancel requests, clear session context/cache, and release the service
    /// before the owned Tokio runtime. This must be called before plugin unload.
    pub(crate) fn close(&self) -> Result<(), FamilyError> {
        close_state(&self.state)
    }
}

impl Drop for TextReplacementBridge {
    fn drop(&mut self) {
        if close_state(&self.state).is_err() {
            tracing::error!(
                event = "astra.emu.text.shutdown_failed",
                diagnostic_code = "ASTRA_EMU_TEXT_SHUTDOWN_FAILED"
            );
        }
    }
}

impl TextReplacementService for TextReplacementSink {
    fn reset(&self, _reason: TextResetReason) -> astra_emu_family_api::FfiFamilyResult<()> {
        let result = with_runtime(&self.state, |runtime| {
            runtime.service.reset();
            Ok(())
        });
        result.into()
    }

    fn submit(&self, request: TextReplacementRequest) -> astra_emu_family_api::FfiFamilyResult<()> {
        submit_request(&self.state, request).into()
    }

    fn poll(
        &self,
        request_id: abi_stable::std_types::RString,
    ) -> astra_emu_family_api::FfiFamilyResult<TextPollResult> {
        let result = with_runtime(&self.state, |runtime| {
            let request_id = owned_request_id(request_id.as_str())?;
            match runtime.service.poll(&request_id).map_err(service_error)? {
                TranslationPoll::Pending => Ok(TextPollResult::Pending),
                TranslationPoll::Ready(result) => {
                    if result.translated.is_empty() {
                        return Ok(TextPollResult::Failed(error(
                            "ASTRA_EMU_TEXT_TRANSLATION_EMPTY",
                        )));
                    }
                    let response = TextReplacementResponse {
                        request_id: request_id.into(),
                        replacement: result.translated.into(),
                    };
                    response.validate().map_err(family_error)?;
                    Ok(TextPollResult::Ready(response))
                }
                TranslationPoll::Cancelled => Ok(TextPollResult::Cancelled),
                TranslationPoll::Failed(error_value) => {
                    Ok(TextPollResult::Failed(translation_error(error_value)))
                }
            }
        });
        result.into()
    }

    fn cancel(
        &self,
        request_id: abi_stable::std_types::RString,
    ) -> astra_emu_family_api::FfiFamilyResult<()> {
        let result = with_runtime(&self.state, |runtime| {
            let request_id = owned_request_id(request_id.as_str())?;
            runtime.service.cancel(&request_id).map_err(service_error)
        });
        result.into()
    }
}

fn submit_request(
    state: &TextBridgeState,
    request: TextReplacementRequest,
) -> Result<(), FamilyError> {
    request.validate().map_err(family_error)?;
    let request_id = owned_request_id(request.request_id.as_str())?;
    let segment = TranslationSegment {
        text: request.source.as_str().to_owned(),
        speaker: optional_text(request.speaker.as_str()),
        ruby: optional_text(request.ruby.as_str()),
    };
    with_runtime(state, |runtime| {
        runtime
            .service
            .submit(request_id, segment)
            .map_err(service_error)
    })
}

fn optional_text(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn owned_request_id(request_id: &str) -> Result<String, FamilyError> {
    if request_id.is_empty()
        || request_id.chars().count() > 256
        || request_id.chars().any(char::is_control)
    {
        return Err(error("ASTRA_EMU_TEXT_REQUEST_ID"));
    }
    Ok(request_id.to_owned())
}

fn with_runtime<T>(
    state: &TextBridgeState,
    action: impl FnOnce(&BridgeRuntime) -> Result<T, FamilyError>,
) -> Result<T, FamilyError> {
    if state.closed.load(Ordering::Acquire) {
        return Err(error("ASTRA_EMU_TEXT_SERVICE_CLOSED"));
    }
    let guard = state
        .inner
        .lock()
        .map_err(|_| error("ASTRA_EMU_TEXT_STATE_LOCK"))?;
    let runtime = guard
        .as_ref()
        .ok_or_else(|| error("ASTRA_EMU_TEXT_SERVICE_CLOSED"))?;
    action(runtime)
}

fn close_state(state: &TextBridgeState) -> Result<(), FamilyError> {
    if state.closed.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| error("ASTRA_EMU_TEXT_STATE_LOCK"))?;
    if let Some(runtime) = guard.as_ref() {
        runtime.service.reset();
    }
    // BridgeRuntime drops service before runtime, ensuring all spawned request
    // handles are invalidated before the executor is torn down.
    guard.take();
    Ok(())
}

fn family_error(error_value: FamilyError) -> FamilyError {
    FamilyError::new(error_value.code(), "translation request was rejected")
}

fn profile_error(error_value: TranslationError) -> FamilyError {
    let code = match error_value {
        TranslationError::InvalidProfile(_) => "ASTRA_EMU_TEXT_PROFILE",
        _ => "ASTRA_EMU_TEXT_PROVIDER",
    };
    error(code)
}

fn translation_error(error_value: TranslationError) -> FamilyError {
    let code = match error_value {
        TranslationError::InvalidProfile(_) => "ASTRA_EMU_TEXT_PROFILE",
        TranslationError::InvalidRequest(_) => "ASTRA_EMU_TEXT_REQUEST",
        TranslationError::ContextLimit => "ASTRA_EMU_TEXT_CONTEXT_LIMIT",
        TranslationError::SecretUnavailable => "ASTRA_EMU_TEXT_AUTH",
        TranslationError::Timeout => "ASTRA_EMU_TEXT_TIMEOUT",
        TranslationError::HttpStatus(401 | 403) => "ASTRA_EMU_TEXT_AUTH",
        TranslationError::HttpStatus(429) => "ASTRA_EMU_TEXT_RATE_LIMITED",
        TranslationError::HttpStatus(_) | TranslationError::Transport => "ASTRA_EMU_TEXT_NETWORK",
        TranslationError::Protocol(_) => "ASTRA_EMU_TEXT_PROTOCOL",
        TranslationError::SessionReset => "ASTRA_EMU_TEXT_CANCELLED",
    };
    error(code)
}

fn service_error(error_value: TranslationServiceError) -> FamilyError {
    let code = match error_value {
        TranslationServiceError::InvalidRequestId => "ASTRA_EMU_TEXT_REQUEST_ID",
        TranslationServiceError::InvalidRequest(_) => "ASTRA_EMU_TEXT_REQUEST",
        TranslationServiceError::DuplicateRequest => "ASTRA_EMU_TEXT_DUPLICATE_REQUEST",
        TranslationServiceError::InflightLimit => "ASTRA_EMU_TEXT_INFLIGHT_LIMIT",
        TranslationServiceError::UnknownRequest => "ASTRA_EMU_TEXT_UNKNOWN_REQUEST",
    };
    error(code)
}

fn error(code: &'static str) -> FamilyError {
    FamilyError::new(code, "translation service operation failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSecrets;

    impl SecretResolver for TestSecrets {
        fn resolve(&self, _reference: &str) -> Result<String, TranslationError> {
            Ok("unused-in-tests".into())
        }
    }

    fn profile() -> TranslationProfile {
        TranslationProfile {
            profile_id: "test.profile".into(),
            endpoint_kind:
                astra_emu_translation_openai_compatible::TranslationEndpointKind::OpenAiCompatible,
            endpoint: "https://example.com/v1".into(),
            protocol: astra_emu_translation_openai_compatible::TranslationProtocol::Responses,
            model: "test-model".into(),
            target_language: "zh-CN".into(),
            timeout_ms: 1_000,
            secret_reference: "test.secret".into(),
        }
    }

    #[test]
    fn constructor_rejects_invalid_profile_before_runtime_creation() {
        let mut value = profile();
        value.endpoint = "http://example.com".into();
        let error = match TextReplacementBridge::new(value, Arc::new(TestSecrets)) {
            Ok(_) => panic!("invalid profile was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "ASTRA_EMU_TEXT_PROFILE");
    }

    #[test]
    fn close_is_idempotent_and_rejects_late_operations() {
        let bridge = TextReplacementBridge::new(profile(), Arc::new(TestSecrets)).unwrap();
        bridge.close().unwrap();
        bridge.close().unwrap();
        let sink = TextReplacementSink {
            state: Arc::clone(&bridge.state),
        };
        let result = sink
            .reset(TextResetReason::NewGame)
            .into_result()
            .unwrap_err();
        assert_eq!(result.code(), "ASTRA_EMU_TEXT_SERVICE_CLOSED");
    }

    #[test]
    fn request_id_is_owned_and_rejected_when_invalid() {
        let bridge = TextReplacementBridge::new(profile(), Arc::new(TestSecrets)).unwrap();
        let sink = TextReplacementSink {
            state: Arc::clone(&bridge.state),
        };
        let result = sink.poll("bad\nrequest".into()).into_result().unwrap_err();
        assert_eq!(result.code(), "ASTRA_EMU_TEXT_REQUEST_ID");
    }

    #[test]
    fn cancel_maps_pending_request_to_cancelled() {
        let bridge =
            TextReplacementBridge::inert_for_test(profile(), Arc::new(TestSecrets)).unwrap();
        let sink = TextReplacementSink {
            state: Arc::clone(&bridge.state),
        };
        sink.submit(TextReplacementRequest {
            request_id: "cancel-me".into(),
            source: "source".into(),
            speaker: "".into(),
            ruby: "".into(),
        })
        .into_result()
        .unwrap();
        sink.cancel("cancel-me".into()).into_result().unwrap();
        assert!(matches!(
            sink.poll("cancel-me".into()).into_result().unwrap(),
            TextPollResult::Cancelled
        ));
    }

    #[test]
    fn reset_discards_pending_request_and_session_state() {
        let bridge =
            TextReplacementBridge::inert_for_test(profile(), Arc::new(TestSecrets)).unwrap();
        let sink = TextReplacementSink {
            state: Arc::clone(&bridge.state),
        };
        sink.submit(TextReplacementRequest {
            request_id: "reset-me".into(),
            source: "source".into(),
            speaker: "".into(),
            ruby: "".into(),
        })
        .into_result()
        .unwrap();
        sink.reset(TextResetReason::Load).into_result().unwrap();
        let error = sink.poll("reset-me".into()).into_result().unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_TEXT_UNKNOWN_REQUEST");
    }
}
