use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Mutex,
};

use abi_stable::{
    prefix_type::PrefixTypeTrait,
    sabi_types::Constructor,
    std_types::{ROption, RResult},
    type_level::downcasting::TD_Opaque,
};
use astra_emu_family_api::{
    AstraFamilyModule, AstraFamilyModuleRef, FamilyError, FamilyModule, FamilyModuleBox,
    FamilyProvider, FamilyResult, FamilySession, FrameConsumerRef, FrameVisitor, OpenRequest,
    SessionRequest,
};

use crate::provider::{FvpProvider, FvpSession};

#[derive(Default)]
struct FvpModule {
    provider: Mutex<FvpProvider>,
    sessions: Mutex<BTreeMap<String, FvpSession>>,
}

impl FamilyModule for FvpModule {
    fn descriptor(
        &self,
    ) -> astra_emu_family_api::FfiFamilyResult<astra_emu_family_api::FamilyDescriptor> {
        boundary(|| self.provider.lock().map_err(lock_error)?.descriptor())
    }

    fn probe(
        &self,
        request: astra_emu_family_api::ProbeRequest,
    ) -> astra_emu_family_api::FfiFamilyResult<ROption<astra_emu_family_api::ProbeReport>> {
        boundary(|| {
            let report = self.provider.lock().map_err(lock_error)?.probe(request)?;
            Ok(report.into())
        })
    }

    fn open(
        &self,
        request: OpenRequest,
    ) -> astra_emu_family_api::FfiFamilyResult<astra_emu_family_api::OpenResponse> {
        boundary(|| {
            let (response, session) = self
                .provider
                .lock()
                .map_err(lock_error)?
                .open_session(request)?;
            self.sessions
                .lock()
                .map_err(lock_error)?
                .insert(response.session_id.to_string(), session);
            Ok(response)
        })
    }

    fn advance(
        &self,
        request: astra_emu_family_api::AdvanceRequest,
    ) -> astra_emu_family_api::FfiFamilyResult<astra_emu_family_api::AdvanceResponse> {
        boundary(|| {
            request.validate()?;
            let mut sessions = self.sessions.lock().map_err(lock_error)?;
            let session = sessions
                .get_mut(request.session_id.as_str())
                .ok_or_else(|| {
                    FamilyError::invalid(
                        "ASTRA_EMU_FVP_SESSION",
                        "the requested FVP session does not exist",
                    )
                })?;
            session.advance(request.elapsed_ns, &request.events)
        })
    }

    fn frame(
        &self,
        request: SessionRequest,
        consumer: FrameConsumerRef<'_>,
    ) -> astra_emu_family_api::FfiFamilyResult<()> {
        boundary(|| {
            request.validate()?;
            let sessions = self.sessions.lock().map_err(lock_error)?;
            let session = sessions
                .get(&request.session_id.to_string())
                .ok_or_else(|| {
                    FamilyError::invalid(
                        "ASTRA_EMU_FVP_SESSION",
                        "the requested FVP session does not exist",
                    )
                })?;
            let mut consumer = consumer;
            let mut visitor = ConsumerVisitor {
                consumer: &mut consumer,
            };
            session.visit_frame(&mut visitor)
        })
    }

    fn close(&self, request: SessionRequest) -> astra_emu_family_api::FfiFamilyResult<()> {
        boundary(|| {
            request.validate()?;
            let session = self
                .sessions
                .lock()
                .map_err(lock_error)?
                .remove(request.session_id.as_str())
                .ok_or_else(|| {
                    FamilyError::invalid(
                        "ASTRA_EMU_FVP_SESSION",
                        "the requested FVP session does not exist",
                    )
                })?;
            Box::new(session).close()
        })
    }
}

struct ConsumerVisitor<'borrow, 'callback> {
    consumer: &'borrow mut FrameConsumerRef<'callback>,
}

impl FrameVisitor for ConsumerVisitor<'_, '_> {
    fn accept(&mut self, frame: astra_emu_family_api::FrameView<'_>) -> FamilyResult<()> {
        self.consumer.accept(frame).into_result().map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_FVP_FRAME_CONSUMER",
                "the host frame consumer rejected the frame",
            )
        })
    }
}

fn lock_error<T>(_error: std::sync::PoisonError<T>) -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_FVP_LOCK",
        "the FVP module state lock is poisoned",
    )
}

fn boundary<T>(action: impl FnOnce() -> FamilyResult<T>) -> RResult<T, FamilyError> {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result.into(),
        Err(_) => Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_PANIC",
            "FVP panicked at the dynamic family boundary",
        ))
        .into(),
    }
}

extern "C" fn construct_module() -> FamilyModuleBox {
    astra_emu_family_api::FamilyModule_TO::from_value(FvpModule::default(), TD_Opaque)
}

#[abi_stable::export_root_module]
pub fn astra_fvp_family_root_module() -> AstraFamilyModuleRef {
    AstraFamilyModule {
        service: Constructor(construct_module),
    }
    .leak_into_prefix()
}
