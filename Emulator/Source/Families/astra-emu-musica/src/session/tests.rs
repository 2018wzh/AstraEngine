use crate::test_fixture as fixture;
use crate::{
    audio::Audio, mount_musica, scene::core_error, MusicaProvider, MusicaVm, MUSICA_PROFILE_FILE,
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
// The provider intentionally permits one live session per process.
static PROVIDER_SESSION: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(feature = "ffmpeg-vcpkg")]
#[path = "tests/movie.rs"]
mod movie;

#[cfg(not(feature = "ffmpeg-vcpkg"))]
#[test]
fn movie_without_ffmpeg_is_an_explicit_feature_error() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".movie 1 sample.mp4 1280 720 t\r\n.end\r\n");
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let error = opened.session.advance(16_666_667, &[]).err().unwrap();
    assert_eq!(error.code.as_str(), "ASTRA_EMU_MUSICA_MOVIE_UNAVAILABLE");
    opened.session.close().unwrap();
}

#[test]
fn unsupported_command_diagnostic_keeps_ordinal_without_source_text() {
    let error = super::vm_error(crate::MusicaRuntimeError::UnsupportedOpcode {
        opcode: "private script text: hidden".into(),
        ordinal: 42,
    });
    assert_eq!(error.code.as_str(), "ASTRA_EMU_MUSICA_RUNTIME_OPCODE");
    assert_eq!(
        error.message.as_str(),
        "script command at ordinal 42 is not implemented"
    );
}

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
            for frame in samples.as_chunks::<2>().0 {
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
fn native_gpu_choice_focus_save_restore_and_confirm_follow_the_selected_branch() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".select First:left Second:right\r\n.label left\r\n.message 1   Wrong\r\n.end\r\n.label right\r\n.message 2   Selected\r\n.end\r\n");
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    assert_eq!(
        opened.session.advance(16_666_667, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    let mut frame = Capture(Vec::new());
    opened.session.visit_frame(&mut frame).unwrap();
    let first = frame.0.clone();
    assert!(first.as_chunks::<4>().0.iter().any(|p| p[0] != 0));
    opened
        .session
        .advance(0, &[key(KeyCode::ArrowDown)])
        .unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    let second = frame.0.clone();
    assert_ne!(first, second);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened
        .session
        .advance(0, &[key(KeyCode::ArrowDown)])
        .unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, first);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, second);
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_ne!(frame.0, second);
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_choice_pointer_ignores_blank_clicks_and_selects_hovered_branch() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".select First:left Second:right\r\n.label left\r\n.message 1   Wrong\r\n.end\r\n.label right\r\n.end\r\n");
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let click = || FamilyEvent::PointerButton {
        button: PointerButton::Primary,
        state: KeyState::Pressed,
    };
    opened.session.advance(16_666_667, &[]).unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.visit_frame(&mut frame).unwrap();
    let first = frame.0.clone();
    // No known pointer position must not confirm the keyboard focus.
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[click()])
            .unwrap()
            .status,
        FamilyStatus::Waiting
    );
    for (x, y) in [(20.0, 20.0), (250.0, 300.0), (1040.0, 310.0)] {
        assert_eq!(
            opened
                .session
                .advance(16_666_667, &[FamilyEvent::PointerMove { x, y }, click()])
                .unwrap()
                .status,
            FamilyStatus::Waiting
        );
        opened.session.visit_frame(&mut frame).unwrap();
        assert_eq!(frame.0, first);
    }
    opened
        .session
        .advance(0, &[FamilyEvent::PointerMove { x: 250.0, y: 310.0 }])
        .unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_ne!(frame.0, first);
    // Only the second branch ends immediately; the first waits for dialogue input.
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[click()])
            .unwrap()
            .status,
        FamilyStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_crossfade_clear_removes_the_previous_effect_frame() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".effect CrossFade BG.png:* 32 100\r\n.effect CrossFade\r\n.end\r\n",
    );
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(&frame.0[..4], &[25, 100, 220, 255]);
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert!(frame
        .0
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [0, 0, 0, 255]));
    opened.session.close().unwrap();
}
#[test]
fn native_family_advances_real_archive_scene_audio_input_save_restore_and_close() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage * BG.png 0 0\r\n.playBGM tone.ogg[50,-100]\r\n.message 1   Hello\r\n.end\r\n",
    );
    let mut provider = MusicaProvider::default();
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
        "ASTRA_EMU_MUSICA_ACTIVE"
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
    assert!(dialogue
        .as_chunks::<4>()
        .0
        .iter()
        .skip(528 * 1280)
        .any(|p| p[0] > 0));
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
        mount_musica(root.path(), &root.path().join(MUSICA_PROFILE_FILE))
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
    let duration = audio.duration(4).unwrap();
    assert!(matches!(
        duration.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    audio.shutdown().unwrap();
    assert!(matches!(
        duration.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    let writes = sink.writes.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(sink.writes.load(Ordering::Acquire), writes);
}
#[test]
fn decoded_audio_rejects_expansion_budget_and_cancellation() {
    let bytes = fixture::wave();
    let stop = AtomicBool::new(false);
    assert!(astra_emu_sdk::decode_audio(&bytes, 100, &stop).is_err());
    stop.store(true, Ordering::Release);
    assert!(astra_emu_sdk::decode_audio(&bytes, 4800, &stop).is_err());
}

#[test]
fn audio_snapshot_timeout_cancels_blocked_worker_and_rejects_late_commands() {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    let archive =
        Arc::new(mount_musica(root.path(), &root.path().join(MUSICA_PROFILE_FILE)).unwrap());
    let sink = Sink {
        block: true,
        ..Default::default()
    };
    let mut audio =
        Audio::start(archive, AudioSink_TO::from_value(sink.clone(), TD_Opaque)).unwrap();
    until(|| sink.writes.load(Ordering::Acquire) > 0);
    assert_eq!(
        audio.snapshot().err().unwrap().code.as_str(),
        "ASTRA_EMU_MUSICA_AUDIO_SNAPSHOT"
    );
    assert!(sink.cancelled.load(Ordering::Acquire));
    assert_eq!(
        audio.apply(Vec::new()).unwrap_err().code.as_str(),
        "ASTRA_EMU_MUSICA_AUDIO_SNAPSHOT"
    );
    assert!(audio.shutdown().is_err());
    let writes = sink.writes.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(sink.writes.load(Ordering::Acquire), writes);
}
#[test]
fn corrupt_or_foreign_slot_is_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".astra-musica/saves");
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
    let script = crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        astra_core::Hash256::from_sha256(source),
        script,
        0,
    )
    .unwrap();
    assert!(vm.step(1).is_err());
}

#[test]
fn native_gpu_control_keys_respect_release_focus_and_choice_boundaries() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".pragma enable_control\r\n.message 1   First\r\n.message 2   Second\r\n.select Confirm:done\r\n.label done\r\n.end\r\n");
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    let mut capture = Capture(Vec::new());
    opened.session.visit_frame(&mut capture).unwrap();
    let first = capture.0.clone();
    opened
        .session
        .advance(0, &[key(KeyCode::ControlLeft), key(KeyCode::ControlRight)])
        .unwrap();
    let mut release_left = key(KeyCode::ControlLeft);
    if let FamilyEvent::Key { state, .. } = &mut release_left {
        *state = KeyState::Released;
    }
    opened.session.advance(16_666_667, &[release_left]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let second = capture.0.clone();
    assert_ne!(first, second);
    opened
        .session
        .advance(
            100_000_000,
            &[FamilyEvent::WindowFocused { focused: false }],
        )
        .unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, second);
    opened
        .session
        .advance(
            0,
            &[
                key(KeyCode::ControlRight),
                FamilyEvent::WindowSuspended { suspended: true },
            ],
        )
        .unwrap();
    opened
        .session
        .advance(
            100_000_000,
            &[FamilyEvent::WindowSuspended { suspended: false }],
        )
        .unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, second);

    opened
        .session
        .advance(
            16_666_667,
            &[
                FamilyEvent::WindowFocused { focused: true },
                key(KeyCode::ControlRight),
            ],
        )
        .unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let choice = capture.0.clone();
    assert_ne!(choice, second);
    assert_eq!(
        opened.session.advance(100_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, choice);
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[key(KeyCode::Enter)])
            .unwrap()
            .status,
        FamilyStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_screen_shake_advances_during_wait_and_restores_through_family_input() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage BG.png 0 0 * 0 0\r\n.shakescreen V 4 30\r\n.wait 1000\r\n.end\r\n",
    );
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    let original = frame.0.clone();
    opened.session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        opened.session.advance(30_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut frame).unwrap();
    let negative = frame.0.clone();
    assert_ne!(negative, original);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.advance(30_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    let positive = frame.0.clone();
    assert_ne!(positive, negative);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, negative);
    opened.session.advance(30_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, positive);
    opened
        .session
        .advance(0, &[FamilyEvent::WindowSuspended { suspended: true }])
        .unwrap();
    opened.session.advance(30_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, positive);
    opened
        .session
        .advance(0, &[FamilyEvent::WindowSuspended { suspended: false }])
        .unwrap();
    opened.session.advance(30_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, negative);
    opened.session.close().unwrap();
}

fn assert_scroll_restores_background_position(command: &str, middle_pixel: usize) {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        format!(".stage * BG.png 0 0\r\n.{command}\r\n.endscroll false\r\n.end\r\n").as_bytes(),
    );
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    let original = frame.0.clone();
    opened.session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut frame).unwrap();
    let middle = frame.0.clone();
    assert_ne!(middle, original);
    assert_eq!(&middle[..4], &[0, 0, 0, 255]);
    assert_eq!(
        &middle[middle_pixel * 4..middle_pixel * 4 + 4],
        &original[..4]
    );
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        FamilyStatus::Finished
    );
    opened.session.visit_frame(&mut frame).unwrap();
    let finished = frame.0.clone();
    assert_ne!(finished, middle);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, middle);
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        FamilyStatus::Finished
    );
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, finished);
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_axis_scroll_waits_and_restores_background_position() {
    assert_scroll_restores_background_position("hscroll 8 1", 4);
}

#[test]
fn native_gpu_linear_scroll_waits_and_restores_background_position() {
    assert_scroll_restores_background_position("scroll 8 4 1", 2 * 1280 + 4);
}

#[path = "tests/wscroll2.rs"]
mod wscroll2;

#[path = "tests/particles.rs"]
mod particles;

#[path = "tests/character.rs"]
mod character;

#[path = "tests/message.rs"]
mod message;

#[path = "tests/backlog.rs"]
mod backlog;

#[path = "tests/voice_preferences.rs"]
mod voice_preferences;

#[path = "tests/playback.rs"]
mod playback;

#[path = "tests/read_skip.rs"]
mod read_skip;

#[path = "tests/encoding.rs"]
mod encoding;

#[path = "tests/includes.rs"]
mod includes;
