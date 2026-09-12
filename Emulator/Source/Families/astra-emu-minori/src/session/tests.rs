use crate::test_fixture as fixture;
use crate::{
    audio::Audio, mount_minori, scene::core_error, MinoriProvider, MinoriVm, MINORI_PROFILE_FILE,
};
use abi_stable::{std_types::ROption, type_level::downcasting::TD_Opaque};
use astra_emu_family_api::*;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
struct Sink {
    cancelled: Arc<AtomicBool>,
    nonzero: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    block: bool,
    left: Arc<AtomicUsize>,
    right: Arc<AtomicUsize>,
}
impl AudioSink for Sink {
    fn configure(&self, format: PcmFormatSpec) -> FfiFamilyResult<()> {
        assert_eq!(format, crate::audio::FORMAT);
        Ok(()).into()
    }
    fn write(&self, chunk: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        self.writes.fetch_add(1, Ordering::AcqRel);
        while self.block && !self.cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(AudioWriteStatus::Cancelled).into();
        }
        if let PcmChunk::F32(samples) = chunk {
            for frame in samples.chunks_exact(2) {
                if frame[0].abs() > 0.01 {
                    self.left.fetch_add(1, Ordering::Relaxed);
                }
                if frame[1].abs() > 0.01 {
                    self.right.fetch_add(1, Ordering::Relaxed);
                }
            }
            if samples.iter().any(|s| s.abs() > 0.01) {
                self.nonzero.store(true, Ordering::Release);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
        Ok(AudioWriteStatus::Accepted).into()
    }
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn cancel(&self) -> FfiFamilyResult<()> {
        self.cancelled.store(true, Ordering::Release);
        Ok(()).into()
    }
}
#[derive(Clone, Default)]
struct Translation {
    request: Arc<std::sync::Mutex<Option<TextReplacementRequest>>>,
    mode: Arc<AtomicUsize>,
    cancellations: Arc<AtomicUsize>,
    resets: Arc<AtomicUsize>,
}
impl TextReplacementService for Translation {
    fn reset(&self, _: TextResetReason) -> FfiFamilyResult<()> {
        *self.request.lock().unwrap() = None;
        self.resets.fetch_add(1, Ordering::Relaxed);
        Ok(()).into()
    }
    fn submit(&self, request: TextReplacementRequest) -> FfiFamilyResult<()> {
        *self.request.lock().unwrap() = Some(request);
        Ok(()).into()
    }
    fn poll(&self, id: abi_stable::std_types::RString) -> FfiFamilyResult<TextPollResult> {
        match self.mode.load(Ordering::Acquire) {
            1 => Ok(TextPollResult::Ready(TextReplacementResponse {
                request_id: id,
                replacement: "Translated".into(),
            }))
            .into(),
            2 => Ok(TextPollResult::Failed(FamilyError::invalid(
                "TEST_TRANSLATION_FAILED",
                "fixture failure",
            )))
            .into(),
            _ => Ok(TextPollResult::Pending).into(),
        }
    }
    fn cancel(&self, _: abi_stable::std_types::RString) -> FfiFamilyResult<()> {
        self.cancellations.fetch_add(1, Ordering::Relaxed);
        Ok(()).into()
    }
}
fn request(root: &std::path::Path, sink: Sink) -> OpenRequest {
    OpenRequest {
        game_path: root.to_str().unwrap().into(),
        configuration: Default::default(),
        initial_window: WindowState {
            width: 1280,
            height: 720,
            focused: true,
            visible: true,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RSome(AudioSink_TO::from_value(sink, TD_Opaque)),
            text_replacement: ROption::RNone,
        },
    }
}
fn key(code: KeyCode) -> FamilyEvent {
    FamilyEvent::Key {
        code,
        state: KeyState::Pressed,
        modifiers: KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        },
    }
}
struct Capture(Vec<u8>);
impl FrameVisitor for Capture {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()> {
        self.0 = frame.as_slice().to_vec();
        Ok(())
    }
}
fn until(test: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !test() {
        assert!(Instant::now() < deadline, "worker progress deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn native_family_advances_real_archive_scene_audio_input_save_restore_and_close() {
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage * BG.png 0 0\r\n.playBGM tone.ogg[50,-100]\r\n.message 1   Hello\r\n.end\r\n",
    );
    let mut provider = MinoriProvider::default();
    assert!(provider
        .probe(ProbeRequest {
            game_path: root.path().to_str().unwrap().into()
        })
        .unwrap()
        .is_some());
    let sink = Sink::default();
    let mut opened = provider.open(request(root.path(), sink.clone())).unwrap();
    assert_eq!(
        provider
            .open(request(root.path(), Sink::default()))
            .err()
            .unwrap()
            .code(),
        "ASTRA_EMU_MINORI_ACTIVE"
    );
    opened.session.advance(16_666_667, &[]).unwrap();
    let mut frame = Capture(vec![]);
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(&frame.0[..4], &[25, 100, 220, 255]);
    opened.session.advance(16_666_667, &[]).unwrap();
    until(|| sink.nonzero.load(Ordering::Acquire));
    assert!(sink.left.load(Ordering::Relaxed) > 0);
    assert_eq!(sink.right.load(Ordering::Relaxed), 0);
    assert_eq!(
        opened.session.advance(16_666_667, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut frame).unwrap();
    let dialogue = frame.0.clone();
    assert!(dialogue.chunks_exact(4).skip(528 * 1280).any(|p| p[0] > 0));
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[key(KeyCode::Enter)])
            .unwrap()
            .status,
        FamilyStatus::Finished
    );
    assert_eq!(
        opened
            .session
            .advance(0, &[key(KeyCode::F9)])
            .unwrap()
            .status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, dialogue);
    opened.session.close().unwrap();
    assert!(sink.cancelled.load(Ordering::Acquire));
    let reopened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    reopened.session.close().unwrap();
    // A supplied enhancement pauses only the message; input received while pending is discarded.
    fixture::game(root.path(), b".message 1   Hello\r\n.end\r\n");
    let translation = Translation::default();
    let mut req = request(root.path(), Sink::default());
    req.host.text_replacement = ROption::RSome(TextReplacementService_TO::from_value(
        translation.clone(),
        TD_Opaque,
    ));
    let (_, mut translated) = provider.open_session(req).unwrap();
    translated.advance(16_666_667, &[]).unwrap();
    translated.advance(0, &[key(KeyCode::F5)]).unwrap();
    translated
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    assert!(translated.pending.is_some());
    translated.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert_eq!(translation.cancellations.load(Ordering::Relaxed), 1);
    assert_eq!(translation.resets.load(Ordering::Relaxed), 2);
    assert!(translated.pending.is_none());
    assert_eq!(translated.message.as_ref().unwrap().0, "Hello");
    translated.message("Hello".into(), None).unwrap();
    translation.mode.store(1, Ordering::Release);
    translated.advance(0, &[]).unwrap();
    assert_eq!(translated.message.as_ref().unwrap().0, "Translated");
    translated.message("Keep original".into(), None).unwrap();
    translation.mode.store(2, Ordering::Release);
    translated.advance(0, &[]).unwrap();
    assert_eq!(translated.message.as_ref().unwrap().0, "Keep original");
    Box::new(translated).close().unwrap();
}
#[test]
fn audio_close_cancels_blocked_write_and_joins_worker() {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    let archive = Arc::new(
        mount_minori(root.path(), &root.path().join(MINORI_PROFILE_FILE))
            .map_err(core_error)
            .unwrap(),
    );
    let sink = Sink {
        block: true,
        ..Default::default()
    };
    let mut audio =
        Audio::start(archive, AudioSink_TO::from_value(sink.clone(), TD_Opaque)).unwrap();
    until(|| sink.writes.load(Ordering::Acquire) > 0);
    audio.shutdown().unwrap();
    let writes = sink.writes.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(sink.writes.load(Ordering::Acquire), writes);
}
#[test]
fn decoded_audio_rejects_expansion_budget_and_cancellation() {
    let bytes = fixture::wave();
    let stop = AtomicBool::new(false);
    assert!(crate::audio::decode::decode(bytes.clone(), 100, &stop).is_err());
    stop.store(true, Ordering::Release);
    assert!(crate::audio::decode::decode(bytes, 4800, &stop).is_err());
}
#[test]
fn corrupt_or_foreign_slot_is_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".astra-minori/saves");
    std::fs::create_dir_all(&path).unwrap();
    let slot = path.join("slot-000.asav");
    std::fs::write(&slot, b"original commercial save").unwrap();
    let store = crate::storage::Storage::new(root.path()).unwrap();
    let state = crate::storage::Snapshot {
        game: astra_core::Hash256::from_sha256(b"game"),
        vm: vec![],
        message: None,
        wait_ns: 0,
        sounds: vec![],
    };
    assert!(store.read().is_err());
    assert!(store.write(&state).is_err());
    assert_eq!(std::fs::read(slot).unwrap(), b"original commercial save");
}
#[test]
fn non_yielding_variable_loop_is_bounded() {
    let source = b".label loop\r\n.set count = count + 1\r\n.goto loop\r\n";
    let script = crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_minori()).unwrap();
    let mut vm = MinoriVm::new(
        "minori:/scr/test.sc".into(),
        astra_core::Hash256::from_sha256(source),
        script,
        0,
    )
    .unwrap();
    assert!(vm.step(1).is_err());
}
