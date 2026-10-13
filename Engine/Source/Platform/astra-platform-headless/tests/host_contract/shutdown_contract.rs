use super::*;

#[cfg(feature = "ffmpeg-vcpkg")]
#[tokio::test]
pub(super) async fn ffmpeg_video_stream_returns_complete_ordered_frames_incrementally() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"streaming video package identity";
    fs::write(temp.path().join("fixture.astrapkg"), package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-video-stream-test",
        "fixture.astrapkg",
        hash(b"build"),
        hash(package),
    );
    profile.providers.video_decode = "ffmpeg-vcpkg".into();
    profile.max_video_frames = 1_000;
    profile.max_decode_output_bytes = 512 * 1024 * 1024;
    let factory = HeadlessPlatformFactory::new(temp.path().join("run"), temp.path());
    let host = factory.start(profile.into()).await.unwrap();
    let client = host.client.clone();
    let session = client.open_decode(DecodeKind::Video).await.unwrap();
    let encoded = include_bytes!("../../../../../Fixtures/PublicDomainMedia/flower.mp4").to_vec();
    let started = client
        .decode(
            session,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: "mp4".into(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                stream_action: astra_platform::DecodeStreamAction::Start,
                bytes: encoded.into(),
            },
        )
        .await
        .unwrap();
    let DecodeOutput::VideoStreamStart {
        duration_estimate_us,
        frame_count,
        decoded_byte_count,
    } = started
    else {
        panic!("stream start returned an unexpected output");
    };
    let duration_us = duration_estimate_us.unwrap();
    assert!(duration_us > 0);
    assert_eq!(frame_count, None);
    assert_eq!(decoded_byte_count, None);
    let mut previous_pts = None;
    let mut total_bytes = 0_u64;

    let mut sequence = 2_u64;
    let mut frames = Vec::new();
    loop {
        let output = client
            .decode(
                session,
                PlatformDecodeRequest {
                    sequence,
                    kind: DecodeKind::Video,
                    codec: String::new(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: false,
                    stream_action: astra_platform::DecodeStreamAction::Next,
                    bytes: Vec::new().into(),
                },
            )
            .await
            .unwrap();
        sequence += 1;
        match output {
            DecodeOutput::VideoStreamEnd {
                frame_count,
                decoded_byte_count,
            } => {
                assert_eq!(frame_count, frames.len() as u64);
                assert_eq!(decoded_byte_count, total_bytes);
                break;
            }
            DecodeOutput::VideoFrame {
                sequence,
                pts_us,
                duration_us: frame_duration,
                width,
                height,
                bgra8,
            } => {
                assert_eq!(sequence, frames.len() as u64 + 1);
                assert!(previous_pts.is_none_or(|previous| pts_us >= previous));
                assert!(frame_duration > 0);
                assert!(pts_us < duration_us);
                assert!(width > 0 && height > 0);
                assert_eq!(bgra8.len() as u64, u64::from(width) * u64::from(height) * 4);
                total_bytes += bgra8.len() as u64;
                previous_pts = Some(pts_us);
                frames.push(hash(&bgra8));
            }
            _ => panic!("stream next returned an unexpected output"),
        }
    }
    assert!(frames.len() > 2);
    assert_ne!(frames.first(), frames.last());
    client.close_decode(session).await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
pub(super) async fn initially_paused_audio_does_not_consume_before_priming_resume() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.initial-paused-audio",
        hash(b"build"),
        hash(b"package"),
    );
    let session = HeadlessPlatformFactory::new(temp.path().join("artifacts"), temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let mut output = client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 480,
            max_buffered_frames: 480,
            start_paused: true,
            capture_samples: true,
        })
        .await
        .unwrap();
    assert_eq!(output.lane.consumed_samples(), 0);
    client.resume_audio(output.handle).await.unwrap();
    output
        .lane
        .wait_for_capacity(960, &AtomicBool::new(false))
        .unwrap();
    output.lane.submit(vec![0.125; 960]).unwrap();
    assert_eq!(output.lane.consumed_samples(), 960);
    client.close_audio(output.handle).await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
pub(super) async fn rejects_legacy_profile_shape_and_audio_limit_before_commit() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.profile",
        hash(b"build"),
        hash(b"package"),
    );
    let mut legacy = serde_json::to_value(&profile).unwrap();
    legacy.as_object_mut().unwrap().remove("product_profile");
    assert!(serde_json::from_value::<HeadlessHostProfile>(legacy).is_err());

    let mut unavailable = profile.clone();
    unavailable.providers.renderer = "missing_renderer".into();
    assert_eq!(
        HeadlessPlatformFactory::new(temp.path().join("missing"), temp.path())
            .start(unavailable.into())
            .await
            .err()
            .unwrap()
            .code,
        PlatformErrorCode::ProviderUnavailable
    );

    #[cfg(not(feature = "ffmpeg-vcpkg"))]
    {
        let mut unavailable = profile.clone();
        unavailable.providers.video_decode = "ffmpeg-vcpkg".into();
        assert_eq!(
            HeadlessPlatformFactory::new(temp.path().join("video"), temp.path())
                .start(unavailable.into())
                .await
                .err()
                .unwrap()
                .code,
            PlatformErrorCode::ProviderUnavailable
        );
    }

    let mut limited = profile;
    limited.artifacts.max_audio_frames = 799;
    let session = HeadlessPlatformFactory::new(temp.path(), temp.path())
        .start(limited.into())
        .await
        .unwrap();
    let mut audio = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 800,
            max_buffered_frames: 800,
            start_paused: false,
            capture_samples: true,
        })
        .await
        .unwrap();
    let error = audio.lane.submit(vec![0.0; 1_600]).unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::QueueOverflow);
    session.client.abort_audio(audio.handle).await.unwrap();
    session.client.shutdown().await.unwrap();

    let mut close_limited = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.close-limit",
        hash(b"build"),
        hash(b"package"),
    );
    close_limited.artifacts.max_audio_frames = 1_000;
    let session = HeadlessPlatformFactory::new(temp.path().join("close-limit"), temp.path())
        .start(close_limited.into())
        .await
        .unwrap();
    let mut first = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 800,
            max_buffered_frames: 800,
            start_paused: false,
            capture_samples: true,
        })
        .await
        .unwrap();
    let mut second = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 800,
            max_buffered_frames: 800,
            start_paused: false,
            capture_samples: true,
        })
        .await
        .unwrap();
    first.lane.submit(vec![0.0; 1_600]).unwrap();
    assert_eq!(
        second.lane.submit(vec![0.0; 1_600]).unwrap_err().code,
        PlatformErrorCode::QueueOverflow
    );
    session.client.close_audio(first.handle).await.unwrap();
    session.client.abort_audio(second.handle).await.unwrap();
    session.client.shutdown().await.unwrap();
}

#[tokio::test]
pub(super) async fn host_thread_initialization_failure_returns_and_allows_a_new_session() {
    let temp = tempfile::tempdir().unwrap();
    let run_root = temp.path().join("run");
    fs::write(&run_root, b"not a directory").unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-start-test",
        "com.example.start",
        hash(b"build"),
        hash(b"package"),
    );
    let factory = HeadlessPlatformFactory::new(&run_root, temp.path());
    let failed = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        factory.start(profile.clone().into()),
    )
    .await
    .expect("failed host initialization must complete its startup handshake");
    assert!(failed.is_err());
    fs::remove_file(&run_root).unwrap();
    for _ in 0..3 {
        let host = factory.start(profile.clone().into()).await.unwrap();
        host.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
pub(super) async fn shutdown_blocks_live_resource_leaks() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.leak",
        hash(b"build"),
        hash(b"package"),
    );
    let session = HeadlessPlatformFactory::new(temp.path(), temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let window = session
        .client
        .create_window(WindowRequest {
            title: "Leak".into(),
            width: 1,
            height: 1,
            visible: false,
        })
        .await
        .unwrap();
    assert_eq!(
        session.client.shutdown().await.unwrap_err().code,
        PlatformErrorCode::ResourceLeak
    );
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
}

#[tokio::test]
pub(super) async fn package_save_and_handle_boundaries_fail_closed_and_reopen_transactionally() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"bounded package source";
    let package_path = temp.path().join("authorized.astrapkg");
    fs::write(&package_path, package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-boundaries",
        "com.example.boundaries",
        hash(b"build"),
        hash(package),
    );
    profile
        .package_sources
        .push(PackageSourcePolicy::UserAuthorized);
    let run_root = temp.path().join("run");
    let session = HeadlessPlatformFactory::new(&run_root, temp.path())
        .with_user_authorized_package(&package_path)
        .start(profile.clone().into())
        .await
        .unwrap();
    let client = session.client;

    let authorized = client
        .open_package(PackageSourceRequest::UserAuthorized {
            expected_hash: hash(package),
        })
        .await
        .unwrap();
    assert_eq!(
        client.read_package_range(authorized, 8, 7).await.unwrap(),
        b"package"
    );
    assert_eq!(
        client
            .read_package_range(authorized, 0, 0)
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::InvalidState
    );
    client.close_package(authorized).await.unwrap();
    assert_eq!(
        client
            .read_package_range(authorized, 0, 1)
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::StaleHandle
    );
    assert_eq!(
        client
            .open_package(PackageSourceRequest::Bundled {
                relative_path: "../authorized.astrapkg".into(),
                expected_hash: hash(package),
            })
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::PermissionDenied
    );
    assert_eq!(
        client
            .open_package(PackageSourceRequest::Bundled {
                relative_path: "authorized.astrapkg".into(),
                expected_hash: hash(b"different"),
            })
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::IntegrityMismatch
    );

    let save = client.begin_save("persistent-slot").await.unwrap();
    client
        .write_save(save, b"persisted".to_vec())
        .await
        .unwrap();
    client.commit_save(save).await.unwrap();
    client.shutdown().await.unwrap();

    let reopened = HeadlessPlatformFactory::new(&run_root, temp.path())
        .start(profile.into())
        .await
        .unwrap();
    assert_eq!(
        reopened.client.list_saves().await.unwrap(),
        ["persistent-slot"]
    );
    assert_eq!(
        reopened.client.read_save("persistent-slot").await.unwrap(),
        b"persisted"
    );
    reopened.client.shutdown().await.unwrap();

    let occupied_root = temp.path().join("occupied-root");
    fs::write(&occupied_root, b"not a directory").unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-conflict",
        "com.example.conflict",
        hash(b"build"),
        hash(package),
    );
    assert!(HeadlessPlatformFactory::new(occupied_root, temp.path())
        .start(profile.into())
        .await
        .is_err());
}
