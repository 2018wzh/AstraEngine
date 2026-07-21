use astra_media::PlayerDecodedAudio;

#[astra_headless_test::test]
fn canonical_upsampling_preserves_tone_energy_and_channel_identity() {
    let source_rate = 44_100_u32;
    let frequency = 440.0_f32;
    let samples = (0..source_rate)
        .map(|frame| {
            ((frame as f32 / source_rate as f32) * frequency * std::f32::consts::TAU).sin() * 0.5
        })
        .collect::<Vec<_>>();
    let converted = PlayerDecodedAudio {
        sample_rate: source_rate,
        channels: 1,
        samples,
    }
    .into_converted(48_000, 2, 100_000)
    .expect("canonical upsample");

    assert_eq!(converted.sample_rate, 48_000);
    assert_eq!(converted.channels, 2);
    assert!(converted.samples.len() >= 95_900 && converted.samples.len() <= 96_100);
    assert!(converted
        .samples
        .chunks_exact(2)
        .all(|frame| frame[0].to_bits() == frame[1].to_bits()));
    let left = converted
        .samples
        .chunks_exact(2)
        .map(|frame| frame[0])
        .collect::<Vec<_>>();
    let rms = (left
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / left.len() as f64)
        .sqrt();
    assert!((rms - (0.5_f64 / 2.0_f64.sqrt())).abs() < 0.002);
    let zero_crossings = left
        .windows(2)
        .filter(|pair| pair[0].is_sign_negative() != pair[1].is_sign_negative())
        .count();
    assert!((876..=884).contains(&zero_crossings));
}
