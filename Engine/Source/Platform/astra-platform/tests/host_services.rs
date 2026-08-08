use std::sync::atomic::AtomicBool;

use astra_platform::{
    host_channel, AudioDeviceFormat, AudioOutputHandle, AudioOutputLane, AudioOutputRequest,
    CapturedFrame, HostCommand, OpenedAudioOutput, PackageSourceHandle, PackageSourceRequest,
    PlatformError, PlatformErrorCode, PlatformHostProfile, SaveTransactionHandle, SurfaceHandle,
    SurfaceRequest, WindowHandle,
};

#[derive(Default)]
struct TestAudioLane {
    consumed_samples: u64,
    underflow_count: u64,
}

impl AudioOutputLane for TestAudioLane {
    fn wait_for_capacity(
        &mut self,
        _requested_samples: usize,
        _stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        self.consumed_samples += samples.len() as u64;
        Ok(samples)
    }

    fn consumed_samples(&self) -> u64 {
        self.consumed_samples
    }

    fn underflow_count(&self) -> u64 {
        self.underflow_count
    }
}

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
                    rgba8: vec![1, 2, 3, 255].into(),
                }))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(
        capture.await.unwrap().unwrap().rgba8.as_ref(),
        [1, 2, 3, 255]
    );

    let open_audio = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .open_audio_output(AudioOutputRequest {
                    sample_rate: 48_000,
                    channels: 2,
                    chunk_frames: 800,
                    max_buffered_frames: 4_800,
                    start_paused: false,
                    capture_samples: false,
                })
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::OpenAudioOutput { reply, .. } => reply
            .send(Ok(OpenedAudioOutput {
                handle: AudioOutputHandle::from_parts(1, 1).unwrap(),
                format: AudioDeviceFormat {
                    sample_rate: 48_000,
                    channels: 2,
                },
                lane: Box::<TestAudioLane>::default(),
                capture: None,
            }))
            .unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    let mut audio = open_audio.await.unwrap().unwrap();
    let samples = vec![0.25, -0.25, 0.5, -0.5];
    let pointer = samples.as_ptr() as usize;
    let returned = audio.lane.submit(samples).unwrap();
    assert_eq!(pointer, returned.as_ptr() as usize);
    assert_eq!(audio.lane.consumed_samples(), 4);
    assert_eq!(audio.lane.underflow_count(), 0);

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
    let error = client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            chunk_frames: 2,
            max_buffered_frames: 2,
            start_paused: false,
            capture_samples: false,
        })
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
        chunk_frames: 512,
        max_buffered_frames: 4_096,
        start_paused: false,
        capture_samples: false,
    };

    assert_eq!(
        request.drain_timeout(48_000 * 2 * 30),
        std::time::Duration::from_secs(32)
    );
    assert_eq!(request.drain_timeout(0), std::time::Duration::from_secs(2));
}
