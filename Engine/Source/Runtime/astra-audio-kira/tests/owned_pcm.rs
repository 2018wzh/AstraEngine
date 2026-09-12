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

struct DeterministicEndpoint {
    consumed: u64,
}

impl AudioOutputLane for DeterministicEndpoint {
    fn wait_for_capacity(
        &mut self,
        _requested_samples: usize,
        _stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
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

#[test]
fn deterministic_backend_advances_a_long_route_without_wall_clock_deadline() {
    let mut session = AudioServiceSession::new(
        AudioServiceConfig {
            max_voices: 1,
            max_buses: 1,
            max_events: 1,
            pcm_cache_bytes: 1024,
        },
        AstraChunkBackendSettings {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 800,
            endpoint: Box::new(DeterministicEndpoint { consumed: 0 }),
            deterministic_fixed_tick_hz: Some(60),
        },
    )
    .expect("deterministic audio service");

    for _ in 0..15_000 {
        session.poll_fixed_tick().expect("deterministic fixed tick");
    }

    let telemetry = session.telemetry();
    assert_eq!(telemetry.submitted_samples, 15_000 * 800 * 2);
    assert_eq!(telemetry.consumed_samples, telemetry.submitted_samples);
}

#[test]
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

#[test]
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

#[test]
fn invalid_restore_preserves_live_audio_before_any_voice_is_stopped() {
    let mut session = AudioServiceSession::new(
        AudioServiceConfig {
            max_voices: 2,
            max_buses: 2,
            max_events: 8,
            pcm_cache_bytes: 64 * 1024,
        },
        AstraChunkBackendSettings {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 800,
            endpoint: Box::new(DeterministicEndpoint { consumed: 0 }),
            deterministic_fixed_tick_hz: Some(60),
        },
    )
    .unwrap();
    session
        .prepare_pcm(asset(), 48_000, 2, vec![0.25; 128])
        .unwrap();
    session
        .apply(AudioServiceCommand::Play {
            voice_id: "voice".into(),
            bus: "voice".into(),
            asset: asset(),
            start_frame: 0,
            looping: true,
        })
        .unwrap();
    let before = session.timeline().clone();
    for variant in 0..7 {
        let mut invalid = before.clone();
        match variant {
            0 => invalid.voices.get_mut("voice").unwrap().asset.revision = "missing".into(),
            1 => invalid.voices.get_mut("voice").unwrap().cursor_frames = 64,
            2 => invalid.voices.get_mut("voice").unwrap().bus = "missing".into(),
            3 => invalid.voices.get_mut("voice").unwrap().command_sequence = 0,
            4 => invalid.buses.get_mut("voice").unwrap().gain = f32::NAN,
            5 => invalid.buses.get_mut("voice").unwrap().fade_id = Some("incomplete".into()),
            _ => {
                let bus = invalid.buses["voice"].clone();
                invalid.buses.insert("extra.one".into(), bus.clone());
                invalid.buses.insert("extra.two".into(), bus);
            }
        }
        assert!(session.restore_timeline(invalid).is_err());
        assert_eq!(session.timeline(), &before);
    }
    session.validate_timeline_restore(&before).unwrap();
    session.restore_timeline(before).unwrap();
    session.poll_fixed_tick().unwrap();
    assert!(session.timeline().voices.contains_key("voice"));
}
