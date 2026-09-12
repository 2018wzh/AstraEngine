use astra_platform::{
    host_channel, DecodeOutput, DecodeSessionHandle, HostCommand, PlatformError, PlatformErrorCode,
    PlatformHostProfile,
};
use astra_player_core::{PlatformCommandSink, PlayerHostCommandExecutor};
use astra_player_vn::{NativeVnHostCommandSource, NativeVnProductMediaHost};

mod support;

fn source() -> NativeVnHostCommandSource {
    let mut source = support::source_for_video(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    stage viewport:320x180 safe_area:16:9 #@id stage.main\n    layer id:video kind:video z:100 blend:normal clip:stage #@id layer.video\n    movie layer:video asset:asset:/video/intro loop:true end:wait fence:movie.intro.end fallback:asset:/video/intro-fallback interrupt:reject #@id movie.intro\n    text key:line.after #@id line.after\n",
    );
    source.launch().unwrap();
    source
}

#[tokio::test]
async fn invalid_stream_start_closes_decoder_and_retries_failed_cleanup() {
    for case in 0..3 {
        let mut source = source();
        let mut media = NativeVnProductMediaHost::default();
        let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
        let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
        let native = DecodeSessionHandle::from_parts(9, 1).unwrap();
        let task = tokio::spawn(async move {
            let HostCommand::OpenDecode { reply, .. } = backend.next_command().await.unwrap()
            else {
                panic!("expected decoder open")
            };
            reply.send(Ok(native)).unwrap();
            let HostCommand::Decode { reply, .. } = backend.next_command().await.unwrap() else {
                panic!("expected stream start")
            };
            reply
                .send(match case {
                    0 => Ok(DecodeOutput::VideoStreamStart {
                        duration_us: Some(0),
                        frame_count: Some(1),
                        decoded_byte_count: Some(4),
                    }),
                    1 => Ok(DecodeOutput::AudioPcmF32 {
                        sample_rate: 48_000,
                        channels: 2,
                        samples: vec![0.0; 2],
                    }),
                    _ => Err(PlatformError::new(
                        PlatformErrorCode::Io,
                        "decode.start",
                        "test decode failure",
                    )),
                })
                .unwrap();
            for attempt in 0..2 {
                let HostCommand::CloseDecode { session, reply } =
                    backend.next_command().await.unwrap()
                else {
                    panic!("expected cleanup attempt {attempt}")
                };
                assert_eq!(session, native);
                if attempt == 0 {
                    reply
                        .send(Err(PlatformError::new(
                            PlatformErrorCode::Io,
                            "decode.close",
                            "test close failure",
                        )))
                        .unwrap();
                } else {
                    reply.send(Ok(())).unwrap();
                }
            }
        });
        let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
        let error = media
            .process(&mut source, &mut executor, 0, Vec::new())
            .await
            .unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("close failed"));
        assert!(diagnostic.contains(match case {
            0 => "ASTRA_PLAYER_VIDEO_STREAM_DESCRIPTOR_INVALID",
            1 => "ASTRA_PLAYER_VIDEO_STREAM_DESCRIPTOR_REQUIRED",
            _ => "test decode failure",
        }));
        assert!(media.snapshot().active_videos.is_empty());
        media.shutdown(&mut source, &mut executor).await.unwrap();
        source.release_resources().unwrap();
        source.shutdown().unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn dropping_a_pending_stream_start_preserves_decoder_cleanup() {
    let mut source = source();
    let mut media = NativeVnProductMediaHost::default();
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let (started, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let native = DecodeSessionHandle::from_parts(9, 1).unwrap();
    let task = tokio::spawn(async move {
        let HostCommand::OpenDecode { reply, .. } = backend.next_command().await.unwrap() else {
            panic!("expected decoder open")
        };
        reply.send(Ok(native)).unwrap();
        let HostCommand::Decode { reply, .. } = backend.next_command().await.unwrap() else {
            panic!("expected stream start")
        };
        started.send(()).unwrap();
        released.await.unwrap();
        assert!(
            reply
                .send(Ok(DecodeOutput::VideoStreamStart {
                    duration_us: Some(1_000),
                    frame_count: Some(1),
                    decoded_byte_count: Some(4),
                }))
                .is_err(),
            "cancelled start must not accept a late response"
        );
        let HostCommand::CloseDecode { session, reply } = backend.next_command().await.unwrap()
        else {
            panic!("expected pending decoder cleanup")
        };
        assert_eq!(session, native);
        reply.send(Ok(())).unwrap();
    });
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    {
        let process = media.process(&mut source, &mut executor, 0, Vec::new());
        tokio::pin!(process);
        tokio::select! {
            _ = waiting => {},
            result = &mut process => panic!("start finished before release: {result:?}"),
        }
    }
    release.send(()).unwrap();
    assert!(media.snapshot().active_videos.is_empty());
    media.shutdown(&mut source, &mut executor).await.unwrap();
    source.release_resources().unwrap();
    source.shutdown().unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn media_shutdown_reclaims_an_interrupted_decoder_open() {
    let mut source = source();
    let mut media = NativeVnProductMediaHost::default();
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let scope = astra_runtime::TaskScope::new();
    let reply = {
        let process = scope.run(media.process(&mut source, &mut executor, 0, Vec::new()));
        tokio::pin!(process);
        let reply = tokio::select! {
            command = backend.next_command() => {
                let HostCommand::OpenDecode { reply, .. } = command.unwrap() else {
                    panic!("expected decoder open")
                };
                reply
            },
            result = &mut process => panic!("open completed without a response: {result:?}"),
        };
        scope.cancel();
        assert_eq!(process.await, astra_runtime::TaskOutcome::Cancelled);
        reply
    };
    let native = DecodeSessionHandle::from_parts(9, 1).unwrap();
    reply.send(Ok(native)).unwrap();
    assert!(executor.sink().has_live_resources());
    let backend_task = tokio::spawn(async move {
        let HostCommand::CloseDecode { session, reply } = backend.next_command().await.unwrap()
        else {
            panic!("expected abandoned open cleanup without starting decode")
        };
        assert_eq!(session, native);
        reply.send(Ok(())).unwrap();
    });
    media.shutdown(&mut source, &mut executor).await.unwrap();
    assert!(!executor.sink().has_live_resources());
    assert!(media.snapshot().active_videos.is_empty());
    source.release_resources().unwrap();
    source.shutdown().unwrap();
    backend_task.await.unwrap();
}
