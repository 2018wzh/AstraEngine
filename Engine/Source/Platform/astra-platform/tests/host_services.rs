use astra_platform::{
    host_channel, AudioMeter, AudioOutputFormat, AudioOutputHandle, AudioOutputRequest,
    AudioOutputState, AudioPacket, CapturedFrame, HostCommand, PackageSourceHandle,
    PackageSourceRequest, PlatformErrorCode, PlatformHostProfile, SaveTransactionHandle,
    SurfaceHandle, SurfaceRequest, WindowHandle,
};

#[tokio::test]
async fn client_exposes_surface_audio_save_and_package_commands() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let window = WindowHandle::from_parts(1, 1).unwrap();

    let create_surface = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .create_surface(SurfaceRequest {
                    window,
                    width: 800,
                    height: 600,
                })
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::CreateSurface { request, reply } => {
            assert_eq!(request.window, window);
            reply
                .send(Ok(SurfaceHandle::from_parts(1, 1).unwrap()))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    let surface = create_surface.await.unwrap().unwrap();

    let capture = tokio::spawn({
        let client = client.clone();
        async move { client.capture_surface(surface).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::CaptureSurface {
            surface: actual,
            reply,
        } => {
            assert_eq!(actual, surface);
            reply
                .send(Ok(CapturedFrame {
                    width: 1,
                    height: 1,
                    rgba8: vec![1, 2, 3, 255],
                }))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(capture.await.unwrap().unwrap().rgba8, [1, 2, 3, 255]);

    let format = tokio::spawn({
        let client = client.clone();
        async move { client.preferred_audio_output_format().await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::QueryAudioOutputFormat { reply } => reply
            .send(Ok(AudioOutputFormat {
                sample_rate: 48_000,
                channels: 2,
            }))
            .unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(format.await.unwrap().unwrap().sample_rate, 48_000);

    let open_audio = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .open_audio_output(AudioOutputRequest {
                    sample_rate: 48_000,
                    channels: 2,
                    max_buffered_frames: 4_800,
                })
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::OpenAudioOutput { reply, .. } => reply
            .send(Ok(AudioOutputHandle::from_parts(1, 1).unwrap()))
            .unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    let audio = open_audio.await.unwrap().unwrap();
    let submit = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .submit_audio(
                    audio,
                    AudioPacket {
                        sequence: 1,
                        channels: 2,
                        samples: vec![0.25, -0.25, 0.5, -0.5],
                    },
                )
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::SubmitAudio { packet, reply, .. } => {
            assert_eq!(packet.frame_count(), 2);
            reply.send(Ok(())).unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    submit.await.unwrap().unwrap();

    let query = tokio::spawn({
        let client = client.clone();
        async move { client.query_audio(audio).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::QueryAudio { output, reply } => {
            assert_eq!(output, audio);
            reply
                .send(Ok(AudioOutputState {
                    queued_frames: 1,
                    submitted_samples: 4,
                    consumed_samples: 2,
                    underflow_count: 0,
                    meter: AudioMeter {
                        sample_count: 2,
                        peak_dbfs: -1.0,
                        rms_dbfs: -3.0,
                    },
                }))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(query.await.unwrap().unwrap().queued_frames, 1);

    let begin_save = tokio::spawn({
        let client = client.clone();
        async move { client.begin_save("slot-1").await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::BeginSave { slot, reply } => {
            assert_eq!(slot, "slot-1");
            reply
                .send(Ok(SaveTransactionHandle::from_parts(1, 1).unwrap()))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(begin_save.await.unwrap().unwrap().parts(), (1, 1));

    let open_package = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .open_package(PackageSourceRequest::Bundled {
                    relative_path: "package/nativevn.astrapkg".to_string(),
                    expected_hash: "sha256:package".to_string(),
                })
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::OpenPackage { source, reply } => {
            assert!(matches!(source, PackageSourceRequest::Bundled { .. }));
            reply
                .send(Ok(PackageSourceHandle::from_parts(1, 1).unwrap()))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(open_package.await.unwrap().unwrap().parts(), (1, 1));
}

#[tokio::test]
async fn client_rejects_oversized_or_undeclared_operations_before_dispatch() {
    let mut profile = PlatformHostProfile::web_release("nativevn-web", "com.example.game");
    profile.limits.max_audio_frames = 1;
    let (client, _backend, _events) = host_channel(profile, 2, 2).unwrap();
    let audio = AudioOutputHandle::from_parts(1, 1).unwrap();
    let error = client
        .submit_audio(
            audio,
            AudioPacket {
                sequence: 1,
                channels: 2,
                samples: vec![0.0; 4],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::InvalidState);

    let error = client
        .open_package(PackageSourceRequest::HttpsRange {
            url: "https://cdn.example/game.astrapkg".to_string(),
            expected_hash: "sha256:package".to_string(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::PermissionDenied);
}

#[test]
fn audio_drain_timeout_covers_long_form_playback_and_callback_margin() {
    let request = AudioOutputRequest {
        sample_rate: 48_000,
        channels: 2,
        max_buffered_frames: 4_096,
    };

    assert_eq!(
        request.drain_timeout(48_000 * 2 * 30),
        std::time::Duration::from_secs(32)
    );
    assert_eq!(request.drain_timeout(0), std::time::Duration::from_secs(2));
}
