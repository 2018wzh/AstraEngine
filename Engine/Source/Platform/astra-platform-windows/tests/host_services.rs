#![cfg(target_os = "windows")]

use astra_platform::{
    AudioOutputRequest, AudioPacket, DecodeKind, DecodeOutput, PlatformDecodeRequest,
    PlatformHostFactory, PlatformHostProfile,
};
use cpal::traits::{DeviceTrait, HostTrait};

#[tokio::test]
async fn windows_host_uses_real_wasapi_stream_and_wmf_decode_session() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let session = astra_platform_windows::factory()
        .start(profile)
        .await
        .expect("start Windows host");

    let native_host = cpal::default_host();
    let native_device = native_host
        .default_output_device()
        .expect("WASAPI default output device");
    let native_format = native_device
        .default_output_config()
        .expect("WASAPI default output format");
    let sample_rate = native_format.sample_rate();
    let channels = native_format.channels();
    let audio = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate,
            channels,
            max_buffered_frames: 4_800,
        })
        .await
        .expect("open WASAPI output");
    let mut samples = Vec::with_capacity(960);
    for frame in 0..480 {
        let sample =
            ((frame as f32 / sample_rate as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.2;
        samples.extend(std::iter::repeat_n(sample, usize::from(channels)));
    }
    session
        .client
        .submit_audio(
            audio,
            AudioPacket {
                sequence: 1,
                channels,
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
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
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
