use super::{error, frame::Frame, input, transport_error, Lease};
use crate::{parent::SessionTransport, wire};
use abi_stable::std_types::ROption;
use astra_emu_family_api::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    process::Command,
    sync::Arc,
};

pub(super) fn open(
    executable: PathBuf,
    game: String,
    window: WindowState,
    sink: Arc<AudioSinkBox>,
    diagnostics: Arc<DiagnosticSinkBox>,
    id: u64,
    lease: Lease,
) -> FamilyResult<FamilyOpen> {
    let mut transport = SessionTransport::launch(Command::new(executable), sink, diagnostics, id)
        .map_err(|_| transport_error("ASTRA_EMU_KRKR_BOOT"))?;
    let boot = wire::Command::Boot {
        game_path: wire::Text::new(game)
            .map_err(|_| error("ASTRA_EMU_KRKR_GAME_PATH", "game path exceeds native bound"))?,
        window: wire::WindowState {
            width: window.width,
            height: window.height,
            focused: window.focused,
            visible: window.visible,
        },
        progress: transport.initial_progress(),
    };
    let opened = (|| {
        let expected = transport
            .boot(boot)
            .map_err(|_| transport_error("ASTRA_EMU_KRKR_BOOT"))?;
        let source = transport
            .capture()
            .map_err(|_| transport_error("ASTRA_EMU_KRKR_FRAME"))?;
        if source.info != expected {
            return Err(error(
                "ASTRA_EMU_KRKR_FRAME",
                "native boot frame changed before initial capture",
            ));
        }
        Frame::from_native(source)
    })();
    let frame = match opened {
        Ok(frame) => frame,
        Err(first) => {
            let _ = transport.close();
            return Err(first);
        }
    };
    let response = OpenResponse {
        session_id: format!("krkr-{id}").into(),
        frame: frame.info(),
        audio_format: ROption::RSome(PcmFormatSpec {
            sample_rate: 48000,
            channels: 2,
            format: PcmFormat::I16,
        }),
    };
    if let Err(first) = response.validate_for_descriptor(&super::krkr_descriptor()) {
        let _ = transport.close();
        return Err(first);
    }
    Ok(FamilyOpen {
        response,
        session: Box::new(KrkrSession {
            transport: RefCell::new(Some(transport)),
            frame: RefCell::new(frame),
            dirty: Cell::new(false),
            first: RefCell::new(None),
            lease: RefCell::new(Some(lease)),
        }),
    })
}

struct KrkrSession {
    transport: RefCell<Option<SessionTransport>>,
    frame: RefCell<Frame>,
    dirty: Cell<bool>,
    first: RefCell<Option<FamilyError>>,
    // Released only after child.wait and every transport/diagnostic worker join.
    lease: RefCell<Option<Lease>>,
}
impl KrkrSession {
    fn fail<T>(&self, first: FamilyError) -> FamilyResult<T> {
        if self.first.borrow().is_none() {
            *self.first.borrow_mut() = Some(first);
        }
        self.finish();
        Err(self.first.borrow().as_ref().unwrap().clone())
    }
    fn check(&self) -> FamilyResult<()> {
        if let Some(first) = self.first.borrow().as_ref() {
            return Err(first.clone());
        }
        if self.transport.borrow().is_none() {
            return Err(error("ASTRA_EMU_KRKR_SESSION", "KrKr session is closed"));
        }
        Ok(())
    }
    fn finish(&self) {
        let transport = self.transport.borrow_mut().take();
        if let Some(transport) = transport {
            if transport.close().is_err() && self.first.borrow().is_none() {
                *self.first.borrow_mut() = Some(transport_error("ASTRA_EMU_KRKR_SHUTDOWN"));
            }
        }
        self.lease.borrow_mut().take();
    }
}
impl FamilySession for KrkrSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        self.check()?;
        let events = match input::events(events) {
            Ok(events) => events,
            Err(first) => return self.fail(first),
        };
        let reply = self
            .transport
            .borrow_mut()
            .as_mut()
            .unwrap()
            .advance(elapsed_ns, events);
        let reply = match reply {
            Ok(reply) => reply,
            Err(_) => return self.fail(transport_error("ASTRA_EMU_KRKR_ADVANCE")),
        };
        let response = match input::response(reply) {
            Ok(response) => response,
            Err(first) => return self.fail(first),
        };
        self.dirty.set(true);
        Ok(response)
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        self.check()?;
        if self.dirty.get() {
            let source = self.transport.borrow_mut().as_mut().unwrap().capture();
            let source = match source {
                Ok(source) => source,
                Err(_) => return self.fail(transport_error("ASTRA_EMU_KRKR_FRAME")),
            };
            let frame = match Frame::from_native(source) {
                Ok(frame) => frame,
                Err(first) => return self.fail(first),
            };
            *self.frame.borrow_mut() = frame;
            self.dirty.set(false);
        }
        let result = {
            let frame = self.frame.borrow();
            frame.view().and_then(|view| visitor.accept(view))
        };
        match result {
            Ok(()) => Ok(()),
            Err(first) => self.fail(first),
        }
    }
    fn close(self: Box<Self>) -> FamilyResult<()> {
        self.finish();
        self.first
            .borrow()
            .as_ref()
            .map_or(Ok(()), |error| Err(error.clone()))
    }
}
impl Drop for KrkrSession {
    fn drop(&mut self) {
        self.finish();
    }
}
