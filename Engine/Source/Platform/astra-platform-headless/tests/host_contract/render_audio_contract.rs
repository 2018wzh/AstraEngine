use super::*;

#[tokio::test]
pub(super) async fn executes_render_audio_save_package_and_zero_leak_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"real package bytes";
    fs::write(temp.path().join("fixture.astrapkg"), package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.headless",
        hash(b"build"),
        hash(package),
    );
    profile.artifacts.namespace = "contract".into();
    profile.artifacts.max_audio_frames = 1_600;
    profile.artifacts.max_duration_ns = 800 * 1_000_000_000 / 48_000;
    let session = HeadlessPlatformFactory::new(temp.path().join("artifacts"), temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;

    let package_handle = client
        .open_package(PackageSourceRequest::Bundled {
            relative_path: "fixture.astrapkg".into(),
            expected_hash: hash(package),
        })
        .await
        .unwrap();
    assert_eq!(
        client
            .read_package_range(package_handle, 5, 7)
            .await
            .unwrap(),
        b"package"
    );
    client.close_package(package_handle).await.unwrap();

    let save = client.begin_save("slot-a").await.unwrap();
    client.write_save(save, b"state".to_vec()).await.unwrap();
    assert_eq!(client.commit_save(save).await.unwrap(), hash(b"state"));
    assert_eq!(client.read_save("slot-a").await.unwrap(), b"state");
    client.delete_save("slot-a").await.unwrap();
    assert_eq!(
        client.read_save("slot-a").await.unwrap_err().code,
        PlatformErrorCode::Io
    );
    let aborted = client.begin_save("slot-aborted").await.unwrap();
    client
        .write_save(aborted, b"discarded".to_vec())
        .await
        .unwrap();
    client.abort_save(aborted).await.unwrap();
    assert_eq!(
        client.commit_save(aborted).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );

    let window = client
        .create_window(WindowRequest {
            title: "Headless".into(),
            width: 2,
            height: 2,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 2,
            height: 2,
        })
        .await
        .unwrap();
    let pixels = vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 1,
                width: 2,
                height: 2,
                rgba8: pixels.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        client
            .capture_surface(surface)
            .await
            .unwrap()
            .rgba8
            .as_ref(),
        pixels.as_slice()
    );
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 2,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                semantics: None,
                commands: vec![astra_media_core::SceneCommand::rect(
                    "real-rect",
                    0,
                    0,
                    1,
                    1,
                    [12, 34, 56, 255],
                )],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        &client.capture_surface(surface).await.unwrap().rgba8[0..4],
        &[12, 34, 56, 255]
    );
    let duplicate = client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 2,
                width: 2,
                height: 2,
                rgba8: vec![0; 16],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(duplicate.code, PlatformErrorCode::InvalidState);
    client.destroy_surface(surface).await.unwrap();
    assert_eq!(
        client.capture_surface(surface).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );
    client.destroy_window(window).await.unwrap();

    let mut encoded_image = Vec::new();
    PngEncoder::new(&mut encoded_image)
        .write_image(
            &[255, 0, 0, 255, 0, 255, 0, 255],
            2,
            1,
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    let image = client.open_decode(DecodeKind::Image).await.unwrap();
    let decoded_image = client
        .decode(
            image,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Image,
                codec: "png".into(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                stream_action: astra_platform::DecodeStreamAction::OneShot,
                bytes: encoded_image.into(),
            },
        )
        .await
        .unwrap();
    let astra_platform::DecodeOutput::CpuBuffer { format, bytes } = decoded_image else {
        panic!("headless image decode returned a native media frame");
    };
    assert_eq!(format, "rgba8");
    assert_eq!(bytes.as_slice(), [255, 0, 0, 255, 0, 255, 0, 255]);
    client.close_decode(image).await.unwrap();

    let video = client.open_decode(DecodeKind::Video).await.unwrap();
    let video_error = client
        .decode(
            video,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: "h264".into(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: Some(2),
                coded_height: Some(2),
                keyframe: true,
                stream_action: astra_platform::DecodeStreamAction::OneShot,
                bytes: vec![1, 2, 3].into(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(video_error.code, PlatformErrorCode::InvalidState);
    assert_eq!(video_error.operation, "decode.video");
    client.close_decode(video).await.unwrap();

    let mut audio = client
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
    let samples = (0..800)
        .flat_map(|frame| {
            let value = ((frame as f32 / 48_000.0) * 440.0 * std::f32::consts::TAU).sin() * 0.25;
            [value, value]
        })
        .collect();
    audio.lane.submit(samples).unwrap();
    assert_eq!(audio.lane.consumed_samples(), 1_600);
    client.pause_audio(audio.handle).await.unwrap();
    client.resume_audio(audio.handle).await.unwrap();
    client.close_audio(audio.handle).await.unwrap();
    assert_eq!(
        client.close_audio(audio.handle).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );

    let mut uncaptured_audio = client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 800,
            max_buffered_frames: 800,
            start_paused: false,
            capture_samples: false,
        })
        .await
        .unwrap();
    uncaptured_audio.lane.submit(vec![0.0; 1_600]).unwrap();
    client.close_audio(uncaptured_audio.handle).await.unwrap();

    // Recreated outputs restart their packet sequence after restore. Artifact paths must
    // remain unique for the whole host session instead of reusing the packet sequence.
    let mut restored_audio = client
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
    restored_audio.lane.submit(vec![0.125; 1_600]).unwrap();
    client.close_audio(restored_audio.handle).await.unwrap();
    client.shutdown().await.unwrap();

    assert!(temp
        .path()
        .join("artifacts/frames/frame-0000000001.png")
        .is_file());
    assert!(temp
        .path()
        .join("artifacts/audio/output-0000000001.wav")
        .is_file());
    assert!(temp
        .path()
        .join("artifacts/audio/output-0000000002.wav")
        .is_file());
    assert!(temp
        .path()
        .join("artifacts/artifact-manifest.json")
        .is_file());
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[tokio::test]
pub(super) async fn ffmpeg_video_rejects_one_shot_decode() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"video package identity";
    fs::write(temp.path().join("fixture.astrapkg"), package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-video-test",
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
    let bytes = include_bytes!("../../../../../Fixtures/PublicDomainMedia/flower.mp4").to_vec();
    let decoded = client
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
                stream_action: astra_platform::DecodeStreamAction::OneShot,
                bytes: bytes.into(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(decoded.code, PlatformErrorCode::InvalidState);
    client.close_decode(session).await.unwrap();
    client.shutdown().await.unwrap();
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[tokio::test]
pub(super) async fn ffmpeg_video_start_enforces_frame_budget_independently_of_stream_budget() {
    const FRAME_BYTES: usize = 960 * 540 * 4;
    for frame_budget in [FRAME_BYTES - 1, FRAME_BYTES] {
        let temp = tempfile::tempdir().unwrap();
        let package = b"video frame budget identity";
        fs::write(temp.path().join("fixture.astrapkg"), package).unwrap();
        let mut profile = HeadlessHostProfile::reference(
            "headless-video-frame-budget",
            "fixture.astrapkg",
            hash(b"build"),
            hash(package),
        );
        profile.viewport_width = 16;
        profile.viewport_height = 16;
        profile.providers.video_decode = "ffmpeg-vcpkg".into();
        profile.max_decode_output_bytes = 3 * 1024 * 1024 * 1024;
        profile.limits.max_frame_bytes = frame_budget;
        let factory = HeadlessPlatformFactory::new(temp.path().join("run"), temp.path());
        let host = factory.start(profile.into()).await.unwrap();
        let client = host.client.clone();
        let session = client.open_decode(DecodeKind::Video).await.unwrap();
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
                    bytes: include_bytes!("../../../../../Fixtures/PublicDomainMedia/flower.mp4")
                        .to_vec()
                        .into(),
                },
            )
            .await;
        if frame_budget < FRAME_BYTES {
            let error = started.unwrap_err();
            assert_eq!(error.code, PlatformErrorCode::IntegrityMismatch);
            // Reject the encoded dimensions at Start, before any decoded
            // frame is requested. The same stream succeeds at the exact limit.
            assert_eq!(
                error.fields.get("diagnostic_codes").map(String::as_str),
                Some("ASTRA_FFMPEG_STREAM_VIDEO_BUDGET"),
                "{error:?}"
            );
        } else {
            assert!(matches!(
                started.unwrap(),
                DecodeOutput::VideoStreamStart { .. }
            ));
        }
        client.close_decode(session).await.unwrap();
        client.shutdown().await.unwrap();
    }
}
