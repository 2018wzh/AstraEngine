use super::*;
use std::{cell::Cell, rc::Rc};

struct Provider {
    closed: Rc<Cell<usize>>,
    panic_on_advance: bool,
    fail_close: bool,
}
struct Session {
    closed: Rc<Cell<usize>>,
    panic_on_advance: bool,
    fail_close: bool,
}

fn frame_info() -> FrameInfo {
    FrameInfo {
        width: 1,
        height: 1,
        stride: 4,
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Opaque,
        },
    }
}

impl FamilyProvider for Provider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        Ok(FamilyDescriptor {
            family_id: "test".into(),
            plugin_id: "astra.emu.test".into(),
            abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
            version: "0.1.0".into(),
            configuration: Default::default(),
            capabilities: vec![FamilyCapability::CpuFrame].into(),
            supported_formats: vec!["test".into()].into(),
        })
    }
    fn probe(&self, _: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        Ok(None)
    }
    fn open(&mut self, _: OpenRequest) -> FamilyResult<FamilyOpen> {
        Ok(FamilyOpen {
            response: OpenResponse {
                session_id: "test-session".into(),
                frame: frame_info(),
                audio_format: ROption::RNone,
            },
            session: Box::new(Session {
                closed: self.closed.clone(),
                panic_on_advance: self.panic_on_advance,
                fail_close: self.fail_close,
            }),
        })
    }
}

impl FamilySession for Session {
    fn advance(&mut self, _: u64, _: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
        assert!(!self.panic_on_advance, "test panic");
        Ok(AdvanceResponse {
            window_command: ROption::RSome(FamilyWindowCommand::SetFullscreen(true)),
            ..AdvanceResponse::running()
        })
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        visitor.accept(FrameView::from_slice(&[0, 0, 0, 255], frame_info())?)
    }
    fn close(self: Box<Self>) -> FamilyResult<()> {
        self.closed.set(self.closed.get() + 1);
        if self.fail_close {
            return Err(FamilyError::invalid("TEST_CLOSE", "test close error"));
        }
        Ok(())
    }
}

fn open_request() -> OpenRequest {
    OpenRequest {
        game_path: "test-game".into(),
        configuration: Default::default(),
        initial_window: WindowState {
            width: 1,
            height: 1,
            focused: true,
            visible: true,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RNone,
            text_replacement: ROption::RNone,
        },
    }
}
fn request() -> SessionRequest {
    SessionRequest {
        session_id: "test-session".into(),
    }
}
fn module(panic_on_advance: bool, fail_close: bool) -> (ProviderModule<Provider>, Rc<Cell<usize>>) {
    let closed = Rc::new(Cell::new(0));
    (
        ProviderModule::new(Provider {
            closed: closed.clone(),
            panic_on_advance,
            fail_close,
        }),
        closed,
    )
}

#[test]
fn active_session_rejects_replacement_and_close_allows_reopen() {
    let (module, closed) = module(false, false);
    module.open(open_request()).into_result().unwrap();
    assert_eq!(
        module
            .open(open_request())
            .into_result()
            .unwrap_err()
            .code(),
        "ASTRA_EMU_FAMILY_SESSION_ACTIVE"
    );
    assert!(module
        .close(SessionRequest {
            session_id: "other-session".into()
        })
        .is_err());
    assert_eq!(closed.get(), 0);
    module.close(request()).into_result().unwrap();
    module.open(open_request()).into_result().unwrap();
    drop(module);
    assert_eq!(closed.get(), 2);
}

#[test]
fn typed_window_command_crosses_the_family_vtable() {
    let (module, closed) = module(false, false);
    let module =
        FamilyModule_TO::from_value(module, abi_stable::type_level::downcasting::TD_Opaque);
    module.open(open_request()).into_result().unwrap();
    let response = module
        .advance(AdvanceRequest {
            session_id: "test-session".into(),
            elapsed_ns: 1,
            events: Default::default(),
        })
        .into_result()
        .unwrap();
    assert_eq!(
        response.window_command,
        ROption::RSome(FamilyWindowCommand::SetFullscreen(true))
    );
    module.close(request()).into_result().unwrap();
    assert_eq!(closed.get(), 1);
}

#[test]
fn panic_blocks_further_work_but_still_allows_worker_shutdown() {
    let (module, closed) = module(true, false);
    module.open(open_request()).into_result().unwrap();
    let result = module.advance(AdvanceRequest {
        session_id: "test-session".into(),
        elapsed_ns: 1,
        events: Default::default(),
    });
    assert_eq!(
        result.into_result().unwrap_err().code(),
        "ASTRA_EMU_FAMILY_PANIC"
    );
    assert_eq!(
        module.descriptor().into_result().unwrap_err().code(),
        "ASTRA_EMU_FAMILY_LOCK"
    );
    module.close(request()).into_result().unwrap();
    assert_eq!(closed.get(), 1);
    assert_eq!(
        module
            .open(open_request())
            .into_result()
            .unwrap_err()
            .code(),
        "ASTRA_EMU_FAMILY_LOCK"
    );
    drop(module);
    assert_eq!(closed.get(), 1);
}

#[test]
fn close_failure_is_returned_without_retaining_the_session() {
    let (module, closed) = module(false, true);
    module.open(open_request()).into_result().unwrap();
    assert_eq!(
        module.close(request()).into_result().unwrap_err().code(),
        "TEST_CLOSE"
    );
    assert!(module.close(request()).is_err());
    drop(module);
    assert_eq!(closed.get(), 1);
}
