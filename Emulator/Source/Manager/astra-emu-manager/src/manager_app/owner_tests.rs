use super::*;
use astra_emu_family_api::*;
use std::{cell::Cell, rc::Rc, thread::ThreadId};

#[derive(Debug)]
struct Call { thread: ThreadId, kind: &'static str, elapsed: u64, events: Vec<FamilyEvent> }

struct Provider {
    calls: Sender<Call>,
    gate: Option<Receiver<()>>,
    entered: Sender<()>,
    marker: Rc<Cell<u64>>,
}
struct Session { calls: Sender<Call>, marker: Rc<Cell<u64>>, frames: FramePublisher }

fn record(calls: &Sender<Call>, kind: &'static str, elapsed: u64, events: &[FamilyEvent]) {
    calls.send(Call { thread: std::thread::current().id(), kind, elapsed, events: events.to_vec() }).unwrap();
}

impl FamilyProvider for Provider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        record(&self.calls, "descriptor", 0, &[]);
        Ok(FamilyDescriptor { family_id: "owner-test".into(), plugin_id: "owner-test".into(),
            abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(), version: "1.0.0".into(),
            configuration: Default::default(), capabilities: vec![FamilyCapability::CpuFrame].into(),
            supported_formats: vec!["owner-test".into()].into() })
    }
    fn probe(&self, _: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        record(&self.calls, "probe", 0, &[]);
        Ok(Some(ProbeReport { family_id: "owner-test".into(), game_id: "game".into(),
            format: "owner-test".into(), confidence_permyriad: 10_000 }))
    }
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        record(&self.calls, "open", 0, &[]);
        self.entered.send(()).unwrap();
        if let Some(gate) = self.gate.take() { gate.recv().unwrap(); }
        let info = FrameInfo { width: 1, height: 1, logical_width: 1, logical_height: 1, stride: 4,
            format: FrameFormat::Rgba8Srgb { alpha: FrameAlpha::Opaque }, rows: FrameRowOrder::TopToBottom };
        let mut frames = FramePublisher::new(request.session_epoch)?;
        frames.publish(vec![23, 45, 67, 255], Vec::as_slice, info)?;
        Ok(FamilyOpen { response: OpenResponse { session_id: "owner-test".into(), frame: info, audio_format: ROption::RNone },
            session: Box::new(Session { calls: self.calls.clone(), marker: self.marker.clone(), frames }) })
    }
}
impl Drop for Provider {
    fn drop(&mut self) { record(&self.calls, "provider_drop", 0, &[]); }
}
impl FamilySession for Session {
    fn advance(&mut self, elapsed: u64, events: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
        record(&self.calls, "advance", elapsed, events);
        self.marker.set(self.marker.get() + 1);
        if events.iter().any(|event| matches!(event, FamilyEvent::WindowCloseRequested)) {
            return Err(FamilyError::new("ASTRA_TEST_ADVANCE", "advance failed"));
        }
        Ok(AdvanceResponse { window_command: ROption::RSome(FamilyWindowCommand::SetTitle("Owner title".into())),
            ..AdvanceResponse::running() })
    }
    fn acquire_frame(&mut self, after: Option<FrameIdentity>) -> FamilyResult<FramePoll> {
        record(&self.calls, "frame", 0, &[]);
        Ok(self.frames.poll(after))
    }
    fn close(self: Box<Self>) -> FamilyResult<()> {
        record(&self.calls, "close", 0, &[]);
        if self.marker.get() == 0 { Ok(()) }
        else { Err(FamilyError::new("ASTRA_TEST_CLOSE", "close failed")) }
    }
}

struct Harness {
    owner: FamilyOwner, mailbox: FrameMailbox, calls: Receiver<Call>, entered: Receiver<()>,
    wake: Receiver<()>, candidate: FamilyProbeCandidate,
}
impl Harness {
    fn new(gate: Option<Receiver<()>>) -> Self {
        let mailbox = FrameMailbox::new();
        let (mut owner, _) = FamilyOwner::start(Vec::new(), mailbox.clone()).unwrap();
        let (calls, receiver) = bounded(128);
        let (entered, entered_receiver) = bounded(2);
        owner.register_provider(move || Provider { calls, entered, gate, marker: Rc::new(Cell::new(0)) }).unwrap();
        let candidate = match owner.probe(&ProbeRequest { game_path: ".".into() }, None, None).unwrap() {
            FamilyProbeSelection::Selected(candidate) => *candidate,
            other => panic!("unexpected selection: {other:?}"),
        };
        let (_, wake) = bounded(1);
        Self { owner, mailbox, calls: receiver, entered: entered_receiver, wake, candidate }
    }
    fn open(&mut self) -> u64 {
        let epoch = allocate_session_epoch().unwrap();
        let (wake, receiver) = bounded(1);
        self.wake = receiver;
        self.mailbox.activate(epoch).unwrap();
        self.owner.open(OpenSession { epoch,
            game: GameRecord { game_id: "game".into(), title: "Owner test".into(), user_title: None,
                location: ".".into(), family_id: Some("owner-test".into()), content_fingerprint: None, added_at_unix_ms: 1 },
            candidate: self.candidate.clone(), configuration: Vec::new(),
            window: WindowState { width: 1, height: 1, focused: true, visible: true }, text: None,
            audio_device: audio_executor::AudioDeviceKind::Native,
            wake: Arc::new(move || { let _ = wake.try_send(()); }),
            modal: None,
        }).unwrap();
        epoch
    }
    fn closed(&self, epoch: u64) -> Update {
        loop {
            if let Some(update) = self.owner.take_update(epoch) {
                if update.closed { return update; }
            }
            self.wake.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
}

#[test]
fn blocked_open_keeps_ui_free_and_preserves_input_close_error_order() {
    let (release, gate) = bounded(1);
    let mut harness = Harness::new(Some(gate));
    let epoch = harness.open();
    harness.entered.recv_timeout(Duration::from_secs(5)).unwrap();
    // Open and close both return while the native entry is still blocked.
    let input = FamilyEvent::PointerMove { x: 12.0, y: 34.0 };
    harness.owner.events(epoch, vec![input.clone()]).unwrap();
    harness.owner.close(epoch, vec![FamilyEvent::WindowCloseRequested]).unwrap();
    release.send(()).unwrap();
    let closed = harness.closed(epoch);
    assert_eq!(closed.error.as_deref(), Some("ASTRA_TEST_ADVANCE: advance failed; ASTRA_TEST_CLOSE: close failed"));
    harness.owner.shutdown().unwrap();
    let calls: Vec<_> = harness.calls.try_iter().collect();
    let thread = calls[0].thread;
    assert_ne!(thread, std::thread::current().id());
    assert!(calls.iter().all(|call| call.thread == thread));
    let advances: Vec<_> = calls.iter().filter(|call| call.kind == "advance").collect();
    assert_eq!(advances[0].events, vec![input]);
    assert_eq!(advances[1].events, vec![FamilyEvent::WindowCloseRequested]);
    assert_eq!(calls.last().unwrap().kind, "provider_drop");
}

#[test]
fn clock_runs_without_ui_ticks_and_cpu_lease_survives_owner_shutdown() {
    let mut harness = Harness::new(None);
    let epoch = harness.open();
    let mut timed = 0;
    while timed < 2 {
        let call = harness.calls.recv_timeout(Duration::from_secs(5)).unwrap();
        if call.kind == "advance" && call.elapsed > 0 { timed += 1; }
    }
    let frame = harness.mailbox.snapshot().unwrap().unwrap();
    harness.owner.close(epoch, Vec::new()).unwrap();
    let closed = harness.closed(epoch);
    assert_eq!(closed.error.as_deref(), Some("ASTRA_TEST_CLOSE: close failed"));
    harness.owner.shutdown().unwrap();
    assert_eq!(frame.pixels(), &[23, 45, 67, 255]);
    assert_eq!(frame.identity.session_epoch, epoch);
}

#[test]
fn closing_owner_does_not_restore_game_surface_or_title_before_join() {
    use astra_emu_manager::ManagerController;
    let harness = Harness::new(None);
    let directory = tempfile::tempdir().unwrap();
    let mut controller = AstraEmuManagerController::open_library(
        directory.path().to_owned(), harness.mailbox.clone(), Vec::new(),
    ).unwrap();
    controller.registry = harness.owner;
    let game = GameRecord { game_id: "game".into(), title: "Closing owner".into(), user_title: None,
        location: ".".into(), family_id: Some("owner-test".into()), content_fingerprint: None, added_at_unix_ms: 1 };
    controller.library.add_game(&game).unwrap();
    controller.candidates.insert(game.game_id.clone(), harness.candidate);
    let (wake, updates) = bounded(1);
    controller.set_host_wake(Arc::new(move || { let _ = wake.try_send(()); }));
    controller.update_window_state(WindowState { width: 1, height: 1, visible: true, focused: true }).unwrap();
    controller.launch("game").unwrap();
    while controller.game_title() != Some("Owner title") {
        updates.recv_timeout(Duration::from_secs(5)).unwrap();
        controller.advance_runtime().unwrap();
    }
    assert!(controller.is_game_active());
    assert_eq!(controller.game_title(), Some("Owner title"));
    controller.leave_game().unwrap();
    // The worker may still be cleaning up; subsequent UI servicing must stay in library mode.
    assert!(!controller.is_game_active());
    assert_eq!(controller.game_title(), None);
    assert!(!controller.game_fullscreen());
    let mut closed = false;
    while !closed {
        let call = harness.calls.recv_timeout(Duration::from_secs(5)).unwrap();
        closed = call.kind == "close";
    }
    assert_eq!(controller.registry.shutdown().unwrap_err(), "ASTRA_TEST_CLOSE: close failed");
}
