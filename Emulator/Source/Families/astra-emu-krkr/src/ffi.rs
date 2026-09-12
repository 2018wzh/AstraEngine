//! `abi_stable` dynamic-family boundary, mirroring the FVP plugin shape.

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
    AdvanceRequest, AdvanceResponse, AstraFamilyModule, AstraFamilyModuleRef, FamilyError,
    FamilyModule, FamilyModuleBox, FamilyOpen, FamilyProvider, FamilyResult, FamilySession,
    FfiFamilyResult, FrameConsumerRef, FrameView, OpenRequest, SessionRequest,
};

use crate::provider::KrkrProvider;

#[derive(Default)]
struct KrkrModule {
    provider: Mutex<KrkrProvider>,
    sessions: Mutex<BTreeMap<String, Box<dyn FamilySession>>>,
}

impl FamilyModule for KrkrModule {
    fn descriptor(&self) -> FfiFamilyResult<astra_emu_family_api::FamilyDescriptor> {
        boundary(|| self.provider.lock().map_err(lock_error)?.descriptor())
    }

    fn probe(
        &self,
        request: astra_emu_family_api::ProbeRequest,
    ) -> FfiFamilyResult<ROption<astra_emu_family_api::ProbeReport>> {
        boundary(|| {
            let report = self.provider.lock().map_err(lock_error)?.probe(request)?;
            Ok(report.into())
        })
    }

    fn open(&self, request: OpenRequest) -> FfiFamilyResult<astra_emu_family_api::OpenResponse> {
        boundary(|| {
            let FamilyOpen { response, session } =
                self.provider.lock().map_err(lock_error)?.open(request)?;
            self.sessions
                .lock()
                .map_err(lock_error)?
                .insert(response.session_id.to_string(), session);
            Ok(response)
        })
    }

    fn advance(&self, request: AdvanceRequest) -> FfiFamilyResult<AdvanceResponse> {
        boundary(|| {
            request.validate()?;
            let mut sessions = self.sessions.lock().map_err(lock_error)?;
            let session = sessions
                .get_mut(request.session_id.as_str())
                .ok_or_else(session_missing)?;
            session.advance(request.elapsed_ns, &request.events)
        })
    }

    fn frame(
        &self,
        request: SessionRequest,
        consumer: FrameConsumerRef<'_>,
    ) -> FfiFamilyResult<()> {
        boundary(|| {
            request.validate()?;
            let sessions = self.sessions.lock().map_err(lock_error)?;
            let session = sessions
                .get(request.session_id.as_str())
                .ok_or_else(session_missing)?;
            let mut consumer = consumer;
            let mut visitor = ConsumerVisitor {
                consumer: &mut consumer,
            };
            session.visit_frame(&mut visitor)
        })
    }

    fn close(&self, request: SessionRequest) -> FfiFamilyResult<()> {
        boundary(|| {
            request.validate()?;
            let session = self
                .sessions
                .lock()
                .map_err(lock_error)?
                .remove(request.session_id.as_str())
                .ok_or_else(session_missing)?;
            session.close()
        })
    }
}

struct ConsumerVisitor<'borrow, 'callback> {
    consumer: &'borrow mut FrameConsumerRef<'callback>,
}

impl astra_emu_family_api::FrameVisitor for ConsumerVisitor<'_, '_> {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()> {
        self.consumer.accept(frame).into_result().map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_KRKR_FRAME_CONSUMER",
                "the host frame consumer rejected the frame",
            )
        })
    }
}

fn session_missing() -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_KRKR_SESSION",
        "the requested Kirikiri session does not exist",
    )
}

fn lock_error<T>(_error: std::sync::PoisonError<T>) -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_KRKR_LOCK",
        "the Kirikiri module state lock is poisoned",
    )
}

fn boundary<T>(action: impl FnOnce() -> FamilyResult<T>) -> RResult<T, FamilyError> {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result.into(),
        Err(_) => Err(FamilyError::invalid(
            "ASTRA_EMU_KRKR_PANIC",
            "the Kirikiri family panicked at the dynamic boundary",
        ))
        .into(),
    }
}

extern "C" fn construct_module() -> FamilyModuleBox {
    astra_emu_family_api::FamilyModule_TO::from_value(KrkrModule::default(), TD_Opaque)
}

#[abi_stable::export_root_module]
pub fn astra_krkr_family_root_module() -> AstraFamilyModuleRef {
    AstraFamilyModule {
        service: Constructor(construct_module),
    }
    .leak_into_prefix()
}
