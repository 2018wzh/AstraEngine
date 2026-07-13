#![cfg(all(
    target_os = "windows",
    feature = "ffmpeg-vcpkg",
    feature = "platform-test-driver"
))]

use std::time::Duration;

use astra_media::{FfmpegStreamLimits, LateVideoPolicy, MediaPipelineLimits};
use astra_platform::{PlatformHostFactory, PlatformHostProfile, SurfaceRequest, WindowRequest};

#[tokio::test]
async fn windows_media_session_streams_ffmpeg_to_wasapi_and_wgpu() {
    let mut profile = PlatformHostProfile::windows_release("media-test", "com.astra.media-test");
    profile.decode.providers.push("ffmpeg".to_string());
    profile.decode.allow_software = true;
    let host = astra_platform_windows::factory()
        .start(profile)
        .await
        .expect("start Windows host");
    let window = host
        .client
        .create_window(WindowRequest {
            title: "Astra media session test".to_string(),
            width: 960,
            height: 540,
            visible: false,
        })
        .await
        .unwrap();
    let surface = host
        .client
        .create_surface(SurfaceRequest {
            window,
            width: 960,
            height: 540,
        })
        .await
        .unwrap();
    let mut media = astra_platform_windows::WindowsNativeMediaSession::open(
        host.client.clone(),
        surface,
        "mp4",
        &fixture_bytes("flower.mp4"),
        FfmpegStreamLimits {
            max_audio_clock_jump_us: 1_000_000,
            late_video_policy: LateVideoPolicy::Drop,
            ..FfmpegStreamLimits::default()
        },
        MediaPipelineLimits::default(),
    )
    .await
    .unwrap();

    let mut presented = 0;
    let mut heard_samples = false;
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let output = media.tick(10_000).await.unwrap();
        presented += usize::from(output.presented_surface_sequence.is_some());
        heard_samples |= output
            .audio_status
            .as_ref()
            .is_some_and(|status| status.meter.sample_count > 0);
        if presented >= 3 && heard_samples {
            break;
        }
    }
    assert!(presented >= 3);
    assert!(heard_samples);
    let captured = host.client.capture_surface(surface).await.unwrap();
    assert_eq!((captured.width, captured.height), (960, 540));
    assert!(captured.rgba8.iter().any(|value| *value != 0));

    let old_audio = media.audio_output_handle_for_test().unwrap();
    host.client
        .inject_audio_device_loss(old_audio)
        .await
        .unwrap();
    let recovered = media.tick(10_000).await.unwrap();
    assert!(recovered.audio_recovered);
    assert_ne!(media.audio_output_handle_for_test().unwrap(), old_audio);

    media.pause().await.unwrap();
    assert!(media.tick(10_000).await.is_err());
    media.resume().await.unwrap();
    let generation = media.generation();
    assert_eq!(media.seek(1_000_000).await.unwrap(), generation + 1);
    let mut post_seek_presented = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if media
            .tick(10_000)
            .await
            .unwrap()
            .presented_surface_sequence
            .is_some()
        {
            post_seek_presented = true;
            break;
        }
    }
    assert!(post_seek_presented);

    media.shutdown().await.unwrap();
    host.client.destroy_surface(surface).await.unwrap();
    host.client.destroy_window(window).await.unwrap();
    host.client.shutdown().await.unwrap();
}

fn fixture_bytes(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainMedia")
        .join(file);
    std::fs::read(path).unwrap()
}
