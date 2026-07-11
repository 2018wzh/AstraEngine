use astra_player_core::{
    PlayerAudioQueueController, PlayerDecodedAudio, PlayerPersistentAudioMixer,
    PlayerPersistentVoiceSpec,
};

fn stereo(samples: Vec<f32>) -> PlayerDecodedAudio {
    PlayerDecodedAudio {
        sample_rate: 48_000,
        channels: 2,
        samples,
    }
}

#[test]
fn queue_controller_primes_startup_then_blocks_new_underflow() {
    let mut queue = PlayerAudioQueueController::new(4_096, 1_024).unwrap();
    assert_eq!(queue.observe(0, 128).unwrap(), 1_024);
    queue.record_submit().unwrap();
    assert_eq!(queue.observe(3_000, 256).unwrap(), 1_024);
    assert_eq!(queue.observe(4_000, 256).unwrap(), 96);
    assert_eq!(
        queue.observe(4_000, 257).unwrap_err().code(),
        "ASTRA_PLAYER_AUDIO_UNDERFLOW"
    );
}

#[test]
fn persistent_mixer_loops_and_clamps_real_samples() {
    let mut mixer = PlayerPersistentAudioMixer::new(48_000, 2, 4, 16).unwrap();
    mixer
        .start_voice(PlayerPersistentVoiceSpec {
            id: "bgm.main".into(),
            bus: "bgm".into(),
            audio: stereo(vec![0.75, -0.75, 0.5, -0.5]),
            looping: true,
            gain: 2.0,
        })
        .unwrap();

    let mixed = mixer.render(3).unwrap();

    assert_eq!(mixed.samples, [1.0, -1.0, 1.0, -1.0, 1.0, -1.0]);
    assert!(mixed.completed.is_empty());
    assert_eq!(mixer.active_voice_count(), 1);
}

#[test]
fn persistent_mixer_fades_per_frame_and_completes_one_shot() {
    let mut mixer = PlayerPersistentAudioMixer::new(48_000, 2, 4, 16).unwrap();
    mixer
        .start_voice(PlayerPersistentVoiceSpec {
            id: "se.scan".into(),
            bus: "se".into(),
            audio: stereo(vec![1.0; 8]),
            looping: false,
            gain: 1.0,
        })
        .unwrap();
    mixer.set_bus_gain("se", 1.0).unwrap();
    mixer.fade_bus("se", 0.0, 4).unwrap();

    let mixed = mixer.render(4).unwrap();

    assert_eq!(mixed.samples, [0.75, 0.75, 0.5, 0.5, 0.25, 0.25, 0.0, 0.0]);
    assert_eq!(mixed.completed[0].voice_id, "se.scan");
    assert_eq!(mixed.completed[0].rendered_frames, 4);
    assert_eq!(mixer.active_voice_count(), 0);
}

#[test]
fn persistent_mixer_rejects_duplicate_format_and_budget_bypasses() {
    let mut mixer = PlayerPersistentAudioMixer::new(48_000, 2, 1, 4).unwrap();
    let voice = PlayerPersistentVoiceSpec {
        id: "bgm.main".into(),
        bus: "bgm".into(),
        audio: stereo(vec![0.0; 4]),
        looping: true,
        gain: 1.0,
    };
    mixer.start_voice(voice.clone()).unwrap();
    assert_eq!(
        mixer.start_voice(voice).unwrap_err().code(),
        "ASTRA_PLAYER_MIXER_VOICE_DUPLICATE"
    );
    assert_eq!(
        mixer.render(5).unwrap_err().code(),
        "ASTRA_PLAYER_MIXER_RENDER_BUDGET"
    );
    assert_eq!(
        mixer
            .start_voice(PlayerPersistentVoiceSpec {
                id: "voice.other".into(),
                bus: "voice".into(),
                audio: PlayerDecodedAudio {
                    sample_rate: 44_100,
                    channels: 2,
                    samples: vec![0.0; 4]
                },
                looping: false,
                gain: 1.0,
            })
            .unwrap_err()
            .code(),
        "ASTRA_PLAYER_MIXER_FORMAT_MISMATCH"
    );
}
