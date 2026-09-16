use super::*;
use crate::test_fixture as fixture;

fn setup() -> (tempfile::TempDir, MinoriMountedVfs, Mixer) {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), &[0]);
    let archive =
        crate::mount_minori(root.path(), &root.path().join(crate::MINORI_PROFILE_FILE)).unwrap();
    let mut mixer = Mixer::new().unwrap();
    mixer
        .load(1, "minori:/bgm/tone.ogg", &archive, &AtomicBool::new(false))
        .unwrap();
    (root, archive, mixer)
}

fn rendered(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut samples = vec![0.0; frames * 2];
    mixer.render(&mut samples);
    samples
}

#[test]
fn restore_rejects_cursor_beyond_asset_without_changing_live_audio() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    let before = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    for playing in [false, true] {
        let mut snapshot: Vec<SoundSnapshot> = postcard::from_bytes(&before).unwrap();
        snapshot[0].position = 1_000_000.0;
        snapshot[0].playing = playing;
        let failure = mixer
            .restore(snapshot, &archive, &AtomicBool::new(false))
            .unwrap_err();
        assert_eq!(failure.code.as_str(), "ASTRA_EMU_MINORI_AUDIO_SNAPSHOT");
        assert_eq!(postcard::to_allocvec(&mixer.snapshot()).unwrap(), before);
    }
    assert!(rendered(&mut mixer, 512)
        .iter()
        .any(|sample| sample.abs() > 0.001));
}

#[test]
fn restore_accepts_a_stopped_sound_at_its_exact_endpoint() {
    let (_root, archive, mut mixer) = setup();
    let data = &mixer.sounds[&1].data;
    let endpoint = data.num_frames() as f64 / f64::from(data.sample_rate);
    let mut snapshot = mixer.snapshot();
    snapshot[0].position = endpoint;
    mixer
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    let restored = mixer.snapshot();
    assert_eq!(restored[0].position, endpoint);
    assert!(!restored[0].playing);
}

#[test]
fn cancelled_empty_restore_cannot_clear_the_live_mixer() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    let before = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    let error = mixer
        .restore(Vec::new(), &archive, &AtomicBool::new(true))
        .unwrap_err();
    assert_eq!(error.code.as_str(), "ASTRA_EMU_AUDIO_CANCELLED");
    assert_eq!(postcard::to_allocvec(&mixer.snapshot()).unwrap(), before);
    assert!(rendered(&mut mixer, 512)
        .iter()
        .any(|sample| sample.abs() > 0.001));
}

#[test]
fn fade_in_snapshot_roundtrip_resumes_gain_and_remaining_duration() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.8, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 2048);
    let encoded = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    let snapshot: Vec<SoundSnapshot> = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(snapshot[0].fade.as_ref().unwrap().elapsed, 2048);
    let mut restored = Mixer::new().unwrap();
    restored
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    let expected = rendered(&mut mixer, 1024);
    let actual = rendered(&mut restored, 1024);
    // A newly created resampler warms up its first few samples. Compare the
    // remaining waveform, including its evolving gain rather than a meter.
    let difference = expected[16..]
        .iter()
        .zip(&actual[16..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(difference < 0.002, "waveform difference={difference}");
    assert!(restored.snapshot()[0].fade.is_some());
    rendered(&mut restored, 4800 - 3072);
    assert!(restored.snapshot()[0].fade.is_none());
    assert!(restored.snapshot()[0].playing);
}

#[test]
fn restored_fade_out_stops_at_remaining_sample_boundary() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 1.0, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    mixer
        .apply(
            MinoriAudioCommand::Stop {
                sequence: 1,
                stream_id: 1,
                fade_ms: 100,
            },
            &archive,
            &AtomicBool::new(false),
        )
        .unwrap();
    rendered(&mut mixer, 1024);
    let snapshot = mixer.snapshot();
    let mut restored = Mixer::new().unwrap();
    restored
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    rendered(&mut restored, 3775);
    assert!(restored.snapshot()[0].playing);
    rendered(&mut restored, 1);
    assert!(!restored.snapshot()[0].playing);
    assert!(rendered(&mut restored, 512)
        .iter()
        .all(|s| s.abs() < 0.00001));
}

#[test]
fn invalid_fade_does_not_replace_current_mixer() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 512);
    let mut snapshot = mixer.snapshot();
    snapshot[0].fade.as_mut().unwrap().frames = 0;
    assert!(mixer
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .is_err());
    assert_eq!(mixer.snapshot()[0].fade.as_ref().unwrap().elapsed, 512);
    assert!(rendered(&mut mixer, 512).iter().any(|s| s.abs() > 0.0001));
}

#[test]
fn new_volume_command_replaces_fade_without_stopping_stream() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 1.0, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 512);
    mixer
        .apply(
            MinoriAudioCommand::SetParams {
                sequence: 2,
                stream_id: 1,
                volume: 0.5,
                pan: 0.0,
                repeat: true,
            },
            &archive,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(mixer.snapshot()[0].fade.is_none());
    assert!(rendered(&mut mixer, 512).iter().any(|v| v.abs() > 0.1));
    assert!(mixer.snapshot()[0].playing);
}

#[test]
fn stopping_unplayed_resource_keeps_a_restorable_snapshot() {
    let (_root, archive, mut mixer) = setup();
    mixer
        .apply(
            MinoriAudioCommand::Stop {
                sequence: 1,
                stream_id: 1,
                fade_ms: 100,
            },
            &archive,
            &AtomicBool::new(false),
        )
        .unwrap();
    let snapshot = mixer.snapshot();
    assert!(!snapshot[0].playing);
    assert!(snapshot[0].fade.is_none());
    mixer
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
}
