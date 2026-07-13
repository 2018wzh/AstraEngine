#![cfg(all(
    target_os = "windows",
    feature = "ffmpeg-vcpkg",
    feature = "platform-test-driver"
))]

use std::time::Duration;

use astra_core::{validate_performance_report, Hash256, PerformanceRunIdentity, PerformanceStatus};
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
    let performance_budget =
        astra_platform_windows::windows_media_performance_budget(host.client.profile(), "classic")
            .unwrap();
    let performance_identity = PerformanceRunIdentity {
        source_revision: "a".repeat(40),
        dirty: true,
        target: host.client.profile().target.clone(),
        profile: "classic".into(),
        profile_hash: host.client.profile().hash().unwrap(),
        package_hash: Hash256::from_sha256(&fixture_bytes("flower-roar.mp4")).to_string(),
        build_fingerprint: astra_platform::build_fingerprint(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            ["ffmpeg-vcpkg", "platform-test-driver"],
        ),
        session_id: "windows.media.test.1".into(),
    };
    let mut mismatched_budget = performance_budget.clone();
    mismatched_budget.profile_hash = format!("sha256:{}", "f".repeat(64));
    let error = astra_platform_windows::WindowsNativeMediaSession::open(
        host.client.clone(),
        surface,
        "mp4",
        &fixture_bytes("flower-roar.mp4"),
        astra_platform_windows::WindowsNativeMediaOpenConfig {
            stream_limits: FfmpegStreamLimits::default(),
            pipeline_limits: MediaPipelineLimits::default(),
            performance_identity: performance_identity.clone(),
            performance_budget: mismatched_budget,
        },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.operation, "media.open");
    assert!(error.message.contains("performance budget"));
    let mut media = astra_platform_windows::WindowsNativeMediaSession::open(
        host.client.clone(),
        surface,
        "mp4",
        &fixture_bytes("flower-roar.mp4"),
        astra_platform_windows::WindowsNativeMediaOpenConfig {
            stream_limits: FfmpegStreamLimits {
                max_audio_clock_jump_us: 5_000_000,
                late_video_policy: LateVideoPolicy::Drop,
                ..FfmpegStreamLimits::default()
            },
            pipeline_limits: MediaPipelineLimits::default(),
            performance_identity: performance_identity.clone(),
            performance_budget: performance_budget.clone(),
        },
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
            .is_some_and(|status| status.meter.sample_count > 0 && status.meter.peak_dbfs > -90.0);
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

    for iteration in 0..100 {
        if media.state() == astra_media::MediaPlaybackState::Ended {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        if let Err(error) = media.tick(10_000).await {
            let status = host
                .client
                .query_audio_output(media.audio_output_handle_for_test().unwrap())
                .await;
            panic!("long-run tick {iteration} failed: {error:?}; audio={status:?}");
        }
    }

    let performance = media.shutdown().await.unwrap();
    match performance.status {
        PerformanceStatus::Pass => {
            validate_performance_report(&performance_budget, &performance_identity, &performance)
                .unwrap();
        }
        PerformanceStatus::Blocked => assert!(!performance.diagnostics.is_empty()),
    }
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
