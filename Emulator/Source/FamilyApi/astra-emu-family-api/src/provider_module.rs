//! Shared Rust-provider to Family ABI lifecycle. No engine or SDK dependency.

use crate::*;
use abi_stable::std_types::ROption;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Mutex,
};

struct State<P> {
    provider: P,
    session: Option<(String, Box<dyn FamilySession>)>,
}

/// Optional ABI implementation for a Rust family provider. A single lock keeps
/// open, frame access, advance and complete worker shutdown mutually exclusive.
pub struct ProviderModule<P> {
    state: Mutex<State<P>>,
}

impl<P> ProviderModule<P> {
    pub fn new(provider: P) -> Self {
        Self {
            state: Mutex::new(State {
                provider,
                session: None,
            }),
        }
    }
}

impl<P: Default> Default for ProviderModule<P> {
    fn default() -> Self {
        Self::new(P::default())
    }
}

impl<P: FamilyProvider> FamilyModule for ProviderModule<P> {
    fn initialize_diagnostics(&self, sink: DiagnosticSinkBox) -> FfiFamilyResult<()> {
        boundary(|| diagnostic_bridge::install(sink))
    }

    fn descriptor(&self) -> FfiFamilyResult<FamilyDescriptor> {
        boundary(|| self.state.lock().map_err(lock_error)?.provider.descriptor())
    }

    fn probe(&self, request: ProbeRequest) -> FfiFamilyResult<ROption<ProbeReport>> {
        boundary(|| {
            Ok(self
                .state
                .lock()
                .map_err(lock_error)?
                .provider
                .probe(request)?
                .into())
        })
    }

    fn open(&self, request: OpenRequest) -> FfiFamilyResult<OpenResponse> {
        boundary(|| {
            let mut state = self.state.lock().map_err(lock_error)?;
            if state.session.is_some() {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_SESSION_ACTIVE",
                    "close the active family session before opening another",
                ));
            }
            let opened = state.provider.open(request)?;
            state.session = Some((opened.response.session_id.to_string(), opened.session));
            Ok(opened.response)
        })
    }

    fn advance(&self, request: AdvanceRequest) -> FfiFamilyResult<AdvanceResponse> {
        boundary(|| {
            request.validate()?;
            let mut state = self.state.lock().map_err(lock_error)?;
            session(&mut state, request.session_id.as_str())?
                .advance(request.elapsed_ns, &request.events)
        })
    }

    fn frame(
        &self,
        request: SessionRequest,
        mut consumer: FrameConsumerRef<'_>,
    ) -> FfiFamilyResult<()> {
        boundary(|| {
            request.validate()?;
            let mut state = self.state.lock().map_err(lock_error)?;
            session(&mut state, request.session_id.as_str())?.visit_frame(&mut ConsumerVisitor {
                consumer: &mut consumer,
            })
        })
    }

    fn close(&self, request: SessionRequest) -> FfiFamilyResult<()> {
        boundary(|| {
            request.validate()?;
            // A panic may poison the state. Only shutdown may recover its guard,
            // so workers can still be cancelled and joined; later opens fail.
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            session(&mut state, request.session_id.as_str())?;
            let (_, session) = state.session.take().expect("session checked while locked");
            session.close()
        })
    }
}

impl<P> Drop for ProviderModule<P> {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        if let Some((_, session)) = state.session.take() {
            if let abi_stable::std_types::RResult::RErr(error) = boundary(|| session.close()) {
                tracing::error!(event = "astra.emu.family.module.drop_close_failed", code = %error.code);
            }
        }
    }
}

fn session<'a, P>(
    state: &'a mut State<P>,
    id: &str,
) -> FamilyResult<&'a mut Box<dyn FamilySession>> {
    match state.session.as_mut() {
        Some((active, session)) if active == id => Ok(session),
        _ => Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_SESSION",
            "the requested family session does not exist",
        )),
    }
}

struct ConsumerVisitor<'a, 'b> {
    consumer: &'a mut FrameConsumerRef<'b>,
}
impl FrameVisitor for ConsumerVisitor<'_, '_> {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()> {
        self.consumer.accept(frame).into_result()
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_FAMILY_LOCK",
        "the family module state lock is poisoned",
    )
}

fn boundary<T>(action: impl FnOnce() -> FamilyResult<T>) -> FfiFamilyResult<T> {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result.into(),
        Err(_) => Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_PANIC",
            "family panicked at the dynamic boundary",
        ))
        .into(),
    }
}

#[cfg(test)]
#[path = "provider_module_tests.rs"]
mod tests;
