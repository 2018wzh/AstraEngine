use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use astra_audio_kira::{
    AstraChunkBackendSettings, AudioAssetRevision, AudioServiceCommand, AudioServiceConfig,
    AudioServiceEvent, AudioServiceSession,
};
use astra_platform::{AudioOutputLane, PlatformError};

struct ConsumingEndpoint {
    consumed: u64,
}

impl AudioOutputLane for ConsumingEndpoint {
    fn wait_for_capacity(
        &mut self,
        _requested_samples: usize,
        stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        if !stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        self.consumed += samples.len() as u64;
        Ok(samples)
    }

    fn consumed_samples(&self) -> u64 {
        self.consumed
    }

    fn underflow_count(&self) -> u64 {
        0
    }
}

fn asset() -> AudioAssetRevision {
    AudioAssetRevision {
        package_id: "test.package".into(),
        uri: "asset://voice/line".into(),
        revision: "1".into(),
        byte_len: 256,
    }
}

#[astra_headless_test::test]
fn decoder_allocation_is_preserved_and_completion_uses_consumed_samples() {
    let mut session = AudioServiceSession::new(
        AudioServiceConfig {
            max_voices: 4,
            max_buses: 2,
            max_events: 8,
            pcm_cache_bytes: 64 * 1024,
        },
        AstraChunkBackendSettings {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 64,
            endpoint: Box::new(ConsumingEndpoint { consumed: 0 }),
            deterministic_fixed_tick_hz: None,
        },
    )
    .expect("audio service");
    let samples = vec![0.25; 128];
    let pointer = samples.as_ptr();
    assert_eq!(
        session
            .prepare_pcm(asset(), 48_000, 2, samples)
            .expect("prepare"),
        pointer
    );
    let sequence = session
        .apply(AudioServiceCommand::Play {
            voice_id: "voice".into(),
            bus: "voice".into(),
            asset: asset(),
            start_frame: 0,
            looping: false,
        })
        .expect("play");

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        session.poll_fixed_tick().expect("fixed tick poll");
        let events = session.take_events();
        if !events.is_empty() {
            assert_eq!(
                events,
                [AudioServiceEvent::VoiceCompleted {
                    sequence,
                    voice_id: "voice".into(),
                }]
            );
            break;
        }
        assert!(Instant::now() < deadline, "completion timed out");
        thread::yield_now();
    }
}

#[astra_headless_test::test]
fn active_pcm_is_pinned_when_cache_budget_is_exhausted() {
    let mut session = AudioServiceSession::new(
        AudioServiceConfig {
            max_voices: 2,
            max_buses: 1,
            max_events: 4,
            pcm_cache_bytes: 512,
        },
        AstraChunkBackendSettings {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 64,
            endpoint: Box::new(ConsumingEndpoint { consumed: 0 }),
            deterministic_fixed_tick_hz: None,
        },
    )
    .expect("audio service");
    session
        .prepare_pcm(asset(), 48_000, 2, vec![0.0; 128])
        .expect("prepare active asset");
    session
        .apply(AudioServiceCommand::Play {
            voice_id: "loop".into(),
            bus: "bgm".into(),
            asset: asset(),
            start_frame: 0,
            looping: true,
        })
        .expect("play loop");
    let mut other = asset();
    other.uri = "asset://voice/other".into();
    assert!(session
        .prepare_pcm(other, 48_000, 2, vec![0.0; 128])
        .is_err());
}
