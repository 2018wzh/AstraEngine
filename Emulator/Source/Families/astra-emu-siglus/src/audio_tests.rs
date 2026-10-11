use super::*;
use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{AudioSink, AudioSink_TO, FfiFamilyResult};
use siglus_scene_vm::audio::kira_hub::{hosted_tap, AudioHub};
use std::sync::{mpsc, Condvar};
use std::time::Duration;

#[derive(Default)]
struct SinkState {
    writes: usize,
    allow_writes: bool,
    cancelled: bool,
    cancellations: usize,
}

type SharedSink = Arc<(Mutex<SinkState>, Condvar)>;

struct BlockingSink(SharedSink);

impl AudioSink for BlockingSink {
    fn configure(&self, format: PcmFormatSpec) -> FfiFamilyResult<()> {
        assert_eq!(format, OUTPUT_FORMAT);
        Ok(()).into()
    }

    fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().unwrap();
        state.writes += 1;
        wake.notify_all();
        while !state.allow_writes && !state.cancelled {
            state = wake.wait(state).unwrap();
        }
        Ok(if state.cancelled {
            AudioWriteStatus::Cancelled
        } else {
            AudioWriteStatus::Accepted
        })
        .into()
    }

    fn is_cancelled(&self) -> bool {
        self.0 .0.lock().unwrap().cancelled
    }

    fn cancel(&self) -> FfiFamilyResult<()> {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().unwrap();
        state.cancelled = true;
        state.cancellations += 1;
        wake.notify_all();
        Ok(()).into()
    }
}

fn wait_for_writes(sink: &SharedSink, expected: usize) {
    let (lock, wake) = &**sink;
    let (state, _) = wake
        .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(3), |state| {
            state.writes < expected
        })
        .unwrap();
    assert!(
        state.writes >= expected,
        "native replacement mixer did not produce PCM"
    );
}

#[test]
fn native_mixer_replacement_preserves_host_sink_and_close_joins_blocked_workers() {
    for _ in 0..3 {
        let sink = Arc::new((Mutex::new(SinkState::default()), Condvar::new()));
        let bridge = PcmBridge::new(AudioSink_TO::from_value(
            BlockingSink(Arc::clone(&sink)),
            TD_Opaque,
        ))
        .unwrap();
        hosted_tap::install(bridge.tap_factory(), OUTPUT_FORMAT.sample_rate);
        let old = AudioHub::new();
        old.check_initialization().unwrap();
        wait_for_writes(&sink, 1);
        // Native reset constructs the new mixer before dropping the old one.
        let replacement = AudioHub::new();
        replacement.check_initialization().unwrap();
        let (finished, received) = mpsc::channel();
        let dropping = thread::spawn(move || {
            drop(old);
            finished.send(()).unwrap();
        });
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        dropping.join().unwrap();
        assert_eq!(sink.0.lock().unwrap().cancellations, 0);
        bridge.check_error().unwrap();
        {
            let mut state = sink.0.lock().unwrap();
            state.allow_writes = true;
            sink.1.notify_all();
        }
        wait_for_writes(&sink, 2);
        bridge.check_error().unwrap();
        sink.0.lock().unwrap().allow_writes = false;
        bridge.close().unwrap();
        drop(replacement);
        hosted_tap::clear();
        let missing_tap = AudioHub::new();
        assert!(!missing_tap.is_enabled());
        assert_eq!(
            missing_tap.check_initialization().unwrap_err().to_string(),
            "ASTRA_SIGLUS_HOSTED_AUDIO_TAP_REQUIRED"
        );
        assert!(bridge.worker.lock().unwrap().is_none());
        assert!(sink.0.lock().unwrap().cancelled);
    }
}
