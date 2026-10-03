//! Tests only the dedicated IPC/ownership path. The fixture never starts a
//! native VM, renders with a GPU, accepts gameplay input or reports NativeReady.
#![cfg(feature = "transport-test-child")]
use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    AudioPlaybackClock, AudioPlaybackProgress, AudioSink, AudioSinkBox, AudioSink_TO,
    AudioWriteStatus, DiagnosticEvent, DiagnosticLevel, DiagnosticSink, DiagnosticSinkBox,
    DiagnosticSink_TO, FamilyError, FfiFamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};
use astra_emu_krkr::{
    parent::SessionTransport,
    wire::{Command as WireCommand, PlaybackClock, Progress, Text, WindowState},
};
use std::{
    io,
    process::Command,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct State {
    configured: usize,
    writes: usize,
    cancelled: bool,
    accepted: u64,
    played: u64,
    copied: Vec<i16>,
}
struct TestSink {
    state: Arc<(Mutex<State>, Condvar)>,
    block_write: bool,
    unsupported_progress: bool,
}
impl AudioSink for TestSink {
    fn configure(&self, format: PcmFormatSpec) -> FfiFamilyResult<()> {
        let mut state = self.state.0.lock().unwrap();
        if state.configured != 0
            || format
                != (PcmFormatSpec {
                    sample_rate: 48000,
                    channels: 2,
                    format: PcmFormat::I16,
                })
        {
            return Err(FamilyError::new("TEST_CONFIGURE", "unexpected configure")).into();
        }
        state.configured += 1;
        Ok(()).into()
    }
    fn write(&self, chunk: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        let PcmChunk::I16(samples) = chunk else {
            return Err(FamilyError::new("TEST_FORMAT", "expected i16")).into();
        };
        let mut state = self.state.0.lock().unwrap();
        state.writes += 1;
        self.state.1.notify_all();
        if self.block_write {
            while !state.cancelled {
                state = self.state.1.wait(state).unwrap();
            }
            return Ok(AudioWriteStatus::Cancelled).into();
        }
        if state.cancelled {
            return Ok(AudioWriteStatus::Cancelled).into();
        }
        // Explicit silent test consumption, not a device timestamp claim.
        state.copied = samples.into_vec();
        state.accepted += (state.copied.len() / 2) as u64;
        state.played = state.accepted;
        Ok(AudioWriteStatus::Accepted).into()
    }
    fn is_cancelled(&self) -> bool {
        self.state.0.lock().unwrap().cancelled
    }
    fn cancel(&self) -> FfiFamilyResult<()> {
        self.state.0.lock().unwrap().cancelled = true;
        self.state.1.notify_all();
        Ok(()).into()
    }
    fn playback_progress(&self) -> FfiFamilyResult<AudioPlaybackProgress> {
        let state = self.state.0.lock().unwrap();
        if self.unsupported_progress {
            return Err(FamilyError::new(
                "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED",
                "progress unavailable",
            ))
            .into();
        }
        if state.cancelled {
            return Err(FamilyError::new("TEST_PROGRESS", "progress unavailable")).into();
        }
        Ok(AudioPlaybackProgress {
            accepted_source_frames: state.accepted,
            played_source_frames: state.played,
            clock: AudioPlaybackClock::TestConsumption,
        })
        .into()
    }
}
type SinkState = Arc<(Mutex<State>, Condvar)>;
fn sink(block_write: bool, unsupported_progress: bool) -> (Arc<AudioSinkBox>, SinkState) {
    let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
    let sink = AudioSink_TO::from_value(
        TestSink {
            state: Arc::clone(&state),
            block_write,
            unsupported_progress,
        },
        TD_Opaque,
    );
    (Arc::new(sink), state)
}
fn fixture(mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_krkr-transport-test-child"));
    command.env("KRKR_TRANSPORT_FIXTURE", mode);
    command
}
#[derive(Default)]
struct DiagnosticState {
    records: usize,
    samples: Vec<DiagnosticEvent>,
}
struct TestDiagnostics(Arc<Mutex<DiagnosticState>>);
impl DiagnosticSink for TestDiagnostics {
    fn enabled(&self, _: DiagnosticLevel) -> bool {
        true
    }
    fn emit(&self, record: DiagnosticEvent) {
        record.validate().unwrap();
        let mut state = self.0.lock().unwrap();
        state.records += 1;
        if state.samples.len() < 8 {
            state.samples.push(record);
        }
    }
}
fn recording_diagnostics() -> (Arc<DiagnosticSinkBox>, Arc<Mutex<DiagnosticState>>) {
    let state = Arc::new(Mutex::new(DiagnosticState::default()));
    let sink = DiagnosticSink_TO::from_value(TestDiagnostics(Arc::clone(&state)), TD_Opaque);
    (Arc::new(sink), state)
}
fn diagnostics() -> Arc<DiagnosticSinkBox> {
    recording_diagnostics().0
}
fn boot() -> WireCommand {
    WireCommand::Boot {
        game_path: Text::new("transport-test-only-no-game".to_owned()).unwrap(),
        window: WindowState {
            width: 640,
            height: 480,
            focused: true,
            visible: true,
        },
        progress: Progress {
            accepted_source_frames: 0,
            played_source_frames: 0,
            clock: PlaybackClock::TestConsumption,
        },
    }
}

#[test]
fn fresh_children_join_before_relaunch_with_stdio_reserved_for_diagnostics() {
    let mut pids = Vec::new();
    for (session, mode) in [(101, "stdio-noise"), (102, "")] {
        let (audio, state) = sink(false, false);
        let (diagnostic, records) = recording_diagnostics();
        let owner = SessionTransport::launch(
            fixture(mode),
            Arc::clone(&audio),
            Arc::clone(&diagnostic),
            session,
        )
        .unwrap();
        pids.push(owner.process_id());
        owner.close().unwrap();
        assert_eq!(
            Arc::strong_count(&diagnostic),
            1,
            "diagnostic pipe/publish workers must have joined"
        );
        if mode == "stdio-noise" {
            let records = records.lock().unwrap();
            assert_eq!(records.records, 2);
            assert!(records
                .samples
                .iter()
                .any(|record| record.target == "krkr.native.stdout"));
            assert!(records
                .samples
                .iter()
                .any(|record| record.target == "krkr.native.stderr"));
        }
        assert_eq!(
            Arc::strong_count(&audio),
            1,
            "parent workers/callbacks must have joined"
        );
        let state = state.0.lock().unwrap();
        assert!(state.cancelled);
        assert_eq!(state.configured, 1);
        assert_eq!(state.writes, 0);
    }
    assert_ne!(pids[0], pids[1]);
}

#[test]
fn diagnostic_flood_is_drained_without_blocking_handshake_or_stop() {
    let (audio, _) = sink(false, false);
    let (diagnostic, records) = recording_diagnostics();
    let owner =
        SessionTransport::launch(fixture("stdio-flood"), audio, Arc::clone(&diagnostic), 113)
            .unwrap();
    owner.close().unwrap();
    assert_eq!(Arc::strong_count(&diagnostic), 1);
    let records = records.lock().unwrap();
    assert!(records.records > 0);
    assert!(records
        .samples
        .iter()
        .all(|record| record.validate().is_ok()));
}

#[test]
fn unavailable_native_open_preserves_failure_after_full_partial_cleanup() {
    let (audio, state) = sink(false, false);
    let error = SessionTransport::open(fixture(""), Arc::clone(&audio), diagnostics(), 103, boot())
        .err()
        .expect("fixture never opens a native core");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
    assert_eq!(Arc::strong_count(&audio), 1);
    assert!(state.0.lock().unwrap().cancelled);
}

#[test]
fn pcm_ack_requires_host_copy_and_real_source_frame_progress() {
    let (audio, state) = sink(false, false);
    let error = SessionTransport::open(
        fixture("pcm-once"),
        Arc::clone(&audio),
        diagnostics(),
        104,
        boot(),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
    assert_eq!(Arc::strong_count(&audio), 1);
    let state = state.0.lock().unwrap();
    assert_eq!(state.writes, 1);
    assert_eq!(state.copied, vec![7; 960]);
    assert_eq!((state.accepted, state.played), (480, 480));
}

#[test]
fn cancellation_unblocks_host_write_child_pcm_wait_and_joins_all_workers() {
    let (audio, state) = sink(true, false);
    let mut owner =
        SessionTransport::launch(fixture("pcm-wait"), Arc::clone(&audio), diagnostics(), 105)
            .unwrap();
    let cancel = owner.cancellation();
    let worker = thread::spawn(move || {
        let first = owner.boot(boot()).unwrap_err();
        let _ = owner.close();
        first.kind()
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut data = state.0.lock().unwrap();
    while data.writes == 0 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "fixture PCM was not submitted");
        data = state.1.wait_timeout(data, remaining).unwrap().0;
    }
    drop(data);
    cancel.cancel();
    assert_eq!(worker.join().unwrap(), io::ErrorKind::Interrupted);
    drop(cancel);
    assert_eq!(Arc::strong_count(&audio), 1);
    assert!(state.0.lock().unwrap().cancelled);
}

#[test]
fn unsupported_progress_rejects_before_child_spawn_and_cancels_sink() {
    let (audio, state) = sink(false, true);
    let error = SessionTransport::launch(fixture(""), Arc::clone(&audio), diagnostics(), 106)
        .err()
        .expect("unsupported progress must reject open");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<FamilyError>()
            .unwrap()
            .code(),
        "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED"
    );
    assert_eq!(Arc::strong_count(&audio), 1);
    let state = state.0.lock().unwrap();
    assert_eq!(state.configured, 1);
    assert!(state.cancelled);
}

#[test]
fn eof_partial_unknown_and_stale_control_reap_the_child() {
    for (session, mode) in [
        (107, "early-exit"),
        (108, "partial-control"),
        (109, "unknown-control"),
        (110, "stale-control"),
    ] {
        let (audio, state) = sink(false, false);
        match SessionTransport::launch(fixture(mode), Arc::clone(&audio), diagnostics(), session) {
            Ok(mut owner) => {
                assert!(owner.boot(boot()).is_err(), "{mode}");
                assert!(
                    owner.close().is_err(),
                    "malformed protocol cannot close successfully"
                );
            }
            Err(_) => assert_eq!(mode, "early-exit"),
        }
        assert_eq!(Arc::strong_count(&audio), 1, "{mode}");
        assert!(state.0.lock().unwrap().cancelled);
    }
}

#[test]
fn blocked_raw_bulk_does_not_delay_control_stop_or_worker_join() {
    let (audio, _) = sink(false, false);
    let owner = SessionTransport::launch(
        fixture("blocked-frame"),
        Arc::clone(&audio),
        diagnostics(),
        111,
    )
    .unwrap();
    // Fixture writes 64 MiB while no frame request/reader consumes it.
    thread::sleep(Duration::from_secs(1));
    let start = Instant::now();
    owner.close().unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(Arc::strong_count(&audio), 1);
}

#[test]
fn forced_child_kill_wait_is_a_failure_and_releases_every_parent_worker() {
    let (audio, _) = sink(false, false);
    let owner = SessionTransport::launch(
        fixture("ignore-stop"),
        Arc::clone(&audio),
        diagnostics(),
        112,
    )
    .unwrap();
    assert!(owner.close().is_err());
    assert_eq!(Arc::strong_count(&audio), 1);
}
