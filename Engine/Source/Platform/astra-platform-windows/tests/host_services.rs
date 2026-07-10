#![cfg(target_os = "windows")]

use astra_platform::{
    AudioOutputRequest, AudioPacket, DecodeKind, DecodeOutput, PlatformDecodeRequest,
    PlatformHostFactory, PlatformHostProfile,
};

#[tokio::test]
async fn windows_host_uses_real_wasapi_stream_and_wmf_decode_session() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let session = astra_platform_windows::factory()
        .start(profile)
        .await
        .expect("start Windows host");

    let audio = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 4_800,
        })
        .await
        .expect("open WASAPI output");
    let mut samples = Vec::with_capacity(960);
    for frame in 0..480 {
        let sample = ((frame as f32 / 48_000.0) * 440.0 * std::f32::consts::TAU).sin() * 0.2;
        samples.extend_from_slice(&[sample, sample]);
    }
    session
        .client
        .submit_audio(
            audio,
            AudioPacket {
                sequence: 1,
                channels: 2,
                samples,
            },
        )
        .await
        .expect("submit audio");
    let meter = session
        .client
        .drain_audio(audio)
        .await
        .expect("drain audio");
    assert!(meter.sample_count > 0);
    assert!(meter.peak_dbfs > -80.0);
    session.client.close_audio(audio).await.unwrap();

    let decode = session
        .client
        .open_decode(DecodeKind::Video)
        .await
        .expect("open WMF session");
    let output = session
        .client
        .decode(
            decode,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: "mp4".to_string(),
                bytes: include_bytes!("../../../../Fixtures/PublicDomainMedia/flower.mp4").to_vec(),
            },
        )
        .await
        .expect("decode public MP4");
    match output {
        DecodeOutput::CpuBuffer { bytes, hash, .. } => {
            assert!(bytes.len() >= 320 * 180 * 4);
            assert!(hash.starts_with("sha256:"));
        }
        DecodeOutput::MediaFrame(_) => panic!("WMF conformance requires bounded CPU output"),
    }
    session.client.close_decode(decode).await.unwrap();
    session.client.shutdown().await.unwrap();
}
