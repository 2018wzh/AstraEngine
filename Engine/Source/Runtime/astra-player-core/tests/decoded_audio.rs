use astra_player_core::PlayerDecodedAudio;

#[astra_headless_test::test]
fn decoded_pcm_s16le_is_converted_to_interleaved_f32() {
    let bytes = [i16::MIN, 0, i16::MAX, 16384]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect::<Vec<_>>();

    let audio = PlayerDecodedAudio::parse("pcm_s16le:48000:2", &bytes, 16).unwrap();

    assert_eq!(audio.sample_rate, 48_000);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.frame_count(), 2);
    assert_eq!(audio.samples[0], -1.0);
    assert_eq!(audio.samples[1], 0.0);
    assert!((audio.samples[2] - 1.0).abs() < 0.0001);
    assert!((audio.samples[3] - 0.500_015).abs() < 0.0001);
}

#[astra_headless_test::test]
fn decoded_audio_rejects_truncation_alignment_and_capacity() {
    assert!(PlayerDecodedAudio::parse("pcm_s16le:48000:2", &[0], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_PCM_TRUNCATED"));
    assert!(PlayerDecodedAudio::parse("pcm_s16le:48000:2", &[0, 0], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_FRAME_ALIGNMENT"));
    assert!(
        PlayerDecodedAudio::parse("pcm_s16le:48000:1", &[0, 0, 0, 0], 1)
            .unwrap_err()
            .to_string()
            .contains("ASTRA_PLAYER_AUDIO_SAMPLE_BUDGET")
    );
}

#[astra_headless_test::test]
fn decoded_audio_rejects_unknown_format_and_invalid_stream_shape() {
    assert!(PlayerDecodedAudio::parse("pcm_f64:48000:2", &[], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_FORMAT_UNSUPPORTED"));
    assert!(PlayerDecodedAudio::parse("pcm_s16le:0:2", &[], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_SAMPLE_RATE"));
    assert!(PlayerDecodedAudio::parse("pcm_s16le:48000:0", &[], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_CHANNELS"));
}
