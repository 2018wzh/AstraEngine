use astra_media::PlayerDecodedAudio;

#[astra_headless_test::test]
fn decoded_audio_accepts_owned_i16_and_f32_samples() {
    let audio =
        PlayerDecodedAudio::from_i16(48_000, 2, vec![i16::MIN, 0, i16::MAX, 16_384], 16).unwrap();

    assert_eq!(audio.sample_rate, 48_000);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.frame_count(), 2);
    assert_eq!(audio.samples[0], -1.0);
    assert_eq!(audio.samples[2], 1.0);

    let samples = vec![0.25_f32, -0.25];
    let pointer = samples.as_ptr();
    let audio = PlayerDecodedAudio::from_f32(48_000, 2, samples, 16).unwrap();
    assert_eq!(audio.samples.as_ptr(), pointer);
}

#[astra_headless_test::test]
fn decoded_audio_rejects_alignment_capacity_and_non_finite_samples() {
    assert!(PlayerDecodedAudio::from_i16(48_000, 2, vec![0], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_SAMPLE_BUDGET"));
    assert!(PlayerDecodedAudio::from_i16(48_000, 1, vec![0, 0], 1)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_SAMPLE_BUDGET"));
    assert!(PlayerDecodedAudio::from_f32(48_000, 1, vec![f32::NAN], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_NON_FINITE"));
}

#[astra_headless_test::test]
fn decoded_audio_rejects_invalid_stream_shape() {
    assert!(PlayerDecodedAudio::from_f32(0, 2, vec![0.0, 0.0], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_SAMPLE_RATE"));
    assert!(PlayerDecodedAudio::from_f32(48_000, 0, vec![0.0], 16)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_CHANNELS"));
}
