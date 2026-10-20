use super::*;
use crate::test_native::{self, Call, Fixture};
use astra_emu_manager_core::model::*;

fn background(owner: &mut FamilyOwner) -> BackgroundEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = owner.take_background() {
            return event;
        }
        assert!(Instant::now() < deadline, "owner background result missing");
        std::thread::yield_now();
    }
}

struct Harness {
    owner: FamilyOwner,
    mailbox: FrameMailbox,
    calls: Arc<Mutex<Vec<Call>>>,
    entered: std::sync::mpsc::Receiver<()>,
    wake: Receiver<()>,
    candidate: FamilyProbeCandidate,
    _root: tempfile::TempDir,
}
impl Harness {
    fn new(gate: Option<std::sync::mpsc::Receiver<()>>, close_error: bool) -> Self {
        let mailbox = FrameMailbox::new();
        let root = tempfile::tempdir().unwrap();
        let mut owner = FamilyOwner::new(root.path().to_owned(), mailbox.clone());
        owner.start().unwrap();
        loop {
            match background(&mut owner) {
                BackgroundEvent::Ready(result) => {
                    result.unwrap();
                    break;
                }
                BackgroundEvent::Progress(PrepareProgress::Cores { .. }) => {}
                BackgroundEvent::Progress(PrepareProgress::Scan { .. })
                | BackgroundEvent::Scanned(_, _) => panic!("core loading must not scan games"),
                BackgroundEvent::Exited(error) => panic!("owner exited during startup: {error}"),
            }
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sink = calls.clone();
        let (entered, receiver) = std::sync::mpsc::channel();
        owner
            .register_provider(move || {
                let mut fixture = Fixture::new("owner-test", "owner-test", "owner-test");
                fixture.calls = sink;
                fixture.gate = gate;
                fixture.entered = Some(entered);
                fixture.close_error = close_error;
                test_native::install(0, fixture)
            })
            .unwrap();
        loop {
            match background(&mut owner) {
                BackgroundEvent::Ready(result) => {
                    result.unwrap();
                    break;
                }
                BackgroundEvent::Exited(error) => {
                    panic!("owner exited during registration: {error}")
                }
                _ => {}
            }
        }
        owner
            .scan(super::super::probe_worker::ScanRequest {
                kind: super::super::probe_worker::ScanKind::Import,
                paths: vec![PathBuf::from(".")],
                games: Vec::new(),
                excluded: BTreeSet::new(),
                cancel: Arc::new(AtomicBool::new(false)),
            })
            .unwrap();
        let candidate = loop {
            match background(&mut owner) {
                BackgroundEvent::Scanned(_, result) => {
                    break result.unwrap().games.remove(0).candidates.remove(0);
                }
                BackgroundEvent::Exited(error) => panic!("owner exited during scan: {error}"),
                _ => {}
            }
        };
        let (_, wake) = bounded(1);
        Self {
            owner,
            mailbox,
            calls,
            entered: receiver,
            wake,
            candidate,
            _root: root,
        }
    }
    fn request(&mut self) -> OpenSession {
        let epoch = allocate_session_epoch().unwrap();
        let (wake, receiver) = bounded(1);
        self.wake = receiver;
        OpenSession {
            epoch,
            game: GameRecord {
                game_id: "game".into(),
                title: "Owner test".into(),
                user_title: None,
                location: ".".into(),
                family_id: Some("owner-test".into()),
                content_fingerprint: None,
                added_at_unix_ms: 1,
            },
            candidate: self.candidate.clone(),
            configuration: Vec::new(),
            render_extent: None,
            window: WindowState {
                width: 1,
                height: 1,
                scale_factor: 1.0,
                focused: true,
                visible: true,
                suspended: false,
            },
            text: None,
            wake: Arc::new(move || {
                let _ = wake.try_send(());
            }),
            modal: None,
            window_service: None,
            cancellation: astra_emu_manager_core::NativeCancellation::new(epoch),
        }
    }
    fn open(&mut self) -> u64 {
        let request = self.request();
        let epoch = request.epoch;
        self.owner.open(request).unwrap();
        epoch
    }
    fn closed(&self, epoch: u64) -> Update {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(update) = self.owner.take_update(epoch) {
                if update.closed {
                    return update;
                }
            }
            assert!(Instant::now() < deadline, "owner did not close");
            let _ = self.wake.recv_timeout(Duration::from_millis(10));
        }
    }
    fn frame(&self) -> FrameLease {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(frame) = self.mailbox.snapshot().unwrap() {
                return frame;
            }
            assert!(Instant::now() < deadline, "native frame missing");
            let _ = self.wake.recv_timeout(Duration::from_millis(10));
        }
    }
}
#[test]
fn blocked_open_keeps_ui_free_and_cancellation_prevents_publication() {
    let (release, gate) = std::sync::mpsc::channel();
    let mut harness = Harness::new(Some(gate), false);
    let epoch = harness.open();
    harness
        .entered
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    harness.owner.close(epoch, Vec::new()).unwrap();
    release.send(()).unwrap();
    let update = harness.closed(epoch);
    assert!(update.error.unwrap().contains("ASTRA_TEST_OPEN_CANCELLED"));
    assert!(!update.opened);
    assert!(harness.mailbox.snapshot().unwrap().is_none());
    harness.owner.shutdown().unwrap();
    let calls = harness.calls.lock();
    assert_eq!(calls[0].name, "initialize");
    assert_eq!(calls[1].name, "describe");
    assert!(calls.iter().all(|call| call.thread == calls[0].thread));
    assert_ne!(calls[0].thread, std::thread::current().id());
    assert!(!calls.iter().any(|call| call.name == "frame"));
}
#[test]
fn native_producer_runs_without_ui_ticks_and_cpu_lease_survives_close_error() {
    let mut harness = Harness::new(None, true);
    let epoch = harness.open();
    let first = harness.frame();
    let deadline = Instant::now() + Duration::from_secs(5);
    while harness.frame().identity.generation <= first.identity.generation {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mods = KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    };
    harness
        .owner
        .events(
            epoch,
            vec![
                FamilyEvent::Key {
                    logical: None,
                    code: KeyCode::Enter,
                    state: KeyState::Pressed,
                    modifiers: mods,
                },
                FamilyEvent::Key {
                    logical: None,
                    code: KeyCode::Escape,
                    state: KeyState::Released,
                    modifiers: mods,
                },
            ],
        )
        .unwrap();
    harness.owner.close(epoch, Vec::new()).unwrap();
    let update = harness.closed(epoch);
    assert!(update.error.unwrap().contains("ASTRA_TEST_CLOSE"));
    harness.owner.shutdown().unwrap();
    assert_eq!(first.pixels().unwrap(), &[23, 45, 67, 255]);
    let calls = harness.calls.lock();
    let input: Vec<_> = calls
        .iter()
        .filter(|call| call.name == "input")
        .map(|call| call.key.as_str())
        .collect();
    assert_eq!(input, ["Enter", "Escape"]);
    assert_eq!(calls.last().unwrap().name, "close");
}
#[test]
fn reopen_is_admitted_only_after_consuming_closed_session() {
    let mut harness = Harness::new(None, false);
    let first_epoch = harness.open();
    let old = harness.frame();
    let request = harness.request();
    assert!(harness.owner.open(request).is_err());
    harness.owner.close(first_epoch, Vec::new()).unwrap();
    assert!(harness.closed(first_epoch).error.is_none());
    let next = harness.open();
    let current = harness.frame();
    assert_ne!(next, first_epoch);
    assert_eq!(current.identity.session_epoch, next);
    assert_eq!(old.pixels().unwrap(), &[23, 45, 67, 255]);
    harness.owner.close(next, Vec::new()).unwrap();
    assert!(harness.closed(next).error.is_none());
    harness.owner.shutdown().unwrap();
}

#[test]
fn action_error_keeps_the_session_open() {
    let mut harness = Harness::new(None, false);
    let epoch = harness.open();
    let _ = harness.frame();
    harness.owner.action(epoch, 0x1002).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let update = loop {
        if let Some(update) = harness.owner.take_update(epoch) {
            if update.action_error.is_some() || update.closed {
                break update;
            }
        }
        assert!(Instant::now() < deadline, "action error did not arrive");
        let _ = harness.wake.recv_timeout(Duration::from_millis(10));
    };
    assert!(update.action_error.is_some());
    assert!(!update.closed);
    harness.owner.action(epoch, 0x1001).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let controls = harness
            .calls
            .lock()
            .iter()
            .filter(|call| call.name == "control")
            .count();
        if controls >= 2 {
            break;
        }
        if let Some(update) = harness.owner.take_update(epoch) {
            assert!(update.action_error.is_none());
            assert!(!update.closed);
        }
        assert!(Instant::now() < deadline, "successful action did not run");
        let _ = harness.wake.recv_timeout(Duration::from_millis(10));
    }
    if let Some(update) = harness.owner.take_update(epoch) {
        assert!(update.action_error.is_none());
        assert!(!update.closed);
    }
    harness.owner.close(epoch, Vec::new()).unwrap();
    assert!(harness.closed(epoch).error.is_none());
    harness.owner.shutdown().unwrap();
}
