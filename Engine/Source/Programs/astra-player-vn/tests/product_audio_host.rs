use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_platform::{
    host_channel, AudioMeter, AudioOutputFormat, AudioOutputHandle, AudioOutputState, DecodeKind,
    DecodeOutput, DecodeSessionHandle, HostCommand, PlatformHostProfile, SurfaceHandle,
};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerHostResourceId,
};
use astra_player_vn::{
    NativeVnAudioControlRequest, NativeVnAudioRequest, NativeVnHostCommandSource,
    NativeVnProductAudioHost, NativeVnProductMediaHost,
};
mod support;

fn source() -> NativeVnHostCommandSource {
    support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
    )
}

#[astra_headless_test::tokio_test]
async fn shared_product_media_host_completes_timeline_fence_and_presents_runtime_result() {
    let mut source = support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    timeline id:intro target:hero property:opacity keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro\n    text key:line.after #@id line.after\n",
    );
    source.launch().unwrap();

    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 4, 4).unwrap();
    let native_surface = SurfaceHandle::from_parts(7, 1).unwrap();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                assert_eq!(surface, native_surface);
                assert_eq!((frame.width, frame.height), (320, 180));
                assert!(!frame.commands.is_empty());
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
    });
    let mut sink = PlatformCommandSink::new(client);
    sink.bind_surface(PlayerHostResourceId(1), native_surface)
        .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let mut host = NativeVnProductMediaHost::default();

    host.process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap();
    assert!(host.is_active());
    host.poll_and_process(&mut source, &mut executor, 120)
        .await
        .unwrap();
    assert!(!host.is_active());
    assert_ne!(
        source.pending_wait().map(|wait| wait.fence.as_str()),
        Some("timeline.intro.complete")
    );
    backend_task.await.unwrap();
}

#[astra_headless_test::tokio_test]
async fn product_media_host_restores_uncommitted_timeline_tasks_after_capacity_failure() {
    let mut source = support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    timeline id:intro target:hero property:opacity keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro\n",
    );
    source.launch().unwrap();
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, _backend, _events) = host_channel(profile, 1, 1).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let mut host = NativeVnProductMediaHost::new(0);

    let error = host
        .process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap_err();

    assert_eq!(error.operation, "player.timeline.schedule");
    let tasks = source.take_timeline_tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "intro");
}

#[astra_headless_test::tokio_test]
async fn product_media_host_presents_every_video_frame_and_restores_by_asset_identity() {
    let mut source = support::source_for_video(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    stage viewport:320x180 safe_area:16:9 #@id stage.main\n    layer id:video kind:video z:100 blend:normal clip:stage #@id layer.video\n    movie layer:video asset:asset:/video/intro loop:true end:wait fence:movie.intro.end fallback:asset:/video/intro-fallback #@id movie.intro\n    text key:line.after #@id line.after\n",
    );
    source.launch().unwrap();
    let first = vec![1, 2, 3, 255];
    let second = vec![4, 5, 6, 255];
    let stream = astra_media::DecodedVideoStream {
        schema: astra_media::DECODED_VIDEO_STREAM_SCHEMA.into(),
        duration_us: 40_000,
        frames: vec![
            astra_media::DecodedVideoFrame {
                sequence: 1,
                pts_us: 0,
                duration_us: 20_000,
                width: 1,
                height: 1,
                content_hash: Hash256::from_sha256(&first),
                bgra8: first,
            },
            astra_media::DecodedVideoFrame {
                sequence: 2,
                pts_us: 20_000,
                duration_us: 20_000,
                width: 1,
                height: 1,
                content_hash: Hash256::from_sha256(&second),
                bgra8: second,
            },
        ],
    };
    let encoded = stream.encode(2, 1_024).unwrap();
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let decode = DecodeSessionHandle::from_parts(9, 1).unwrap();
    let surface = SurfaceHandle::from_parts(7, 1).unwrap();
    let backend_task = tokio::spawn(async move {
        for expected_cycle in 0..2 {
            match backend.next_command().await.unwrap() {
                HostCommand::OpenDecode { kind, reply } => {
                    assert_eq!(kind, DecodeKind::Video);
                    reply.send(Ok(decode)).unwrap();
                }
                command => panic!("unexpected command: {}", command.operation()),
            }
            match backend.next_command().await.unwrap() {
                HostCommand::Decode { request, reply, .. } => {
                    assert_eq!(request.kind, DecodeKind::Video);
                    reply
                        .send(Ok(DecodeOutput::CpuBuffer {
                            format: "postcard:astra.decoded_video_stream.v1".into(),
                            hash: Hash256::from_sha256(&encoded).to_string(),
                            bytes: encoded.clone(),
                        }))
                        .unwrap();
                }
                command => panic!("unexpected command: {}", command.operation()),
            }
            match backend.next_command().await.unwrap() {
                HostCommand::CloseDecode { reply, .. } => reply.send(Ok(())).unwrap(),
                command => panic!("unexpected command: {}", command.operation()),
            }
            match backend.next_command().await.unwrap() {
                HostCommand::PresentScene { frame, reply, .. } => {
                    assert!(!frame.commands.is_empty());
                    reply.send(Ok(())).unwrap();
                }
                command => panic!(
                    "unexpected command in video cycle {expected_cycle}: {}",
                    command.operation()
                ),
            }
        }
        match backend.next_command().await.unwrap() {
            HostCommand::PresentScene { frame, reply, .. } => {
                assert!(!frame.commands.is_empty());
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected completion command: {}", command.operation()),
        }
    });
    let mut sink = PlatformCommandSink::new(client);
    sink.bind_surface(PlayerHostResourceId(1), surface).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let mut media = NativeVnProductMediaHost::default();
    media
        .process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap();
    assert!(media.is_active());
    let snapshot = media.snapshot();
    let mut restored = NativeVnProductMediaHost::default();
    restored.restore(snapshot).unwrap();
    restored
        .poll_and_process(&mut source, &mut executor, 20)
        .await
        .unwrap();
    assert!(restored.has_active_video());
    assert!(restored.skip_active_videos(&mut source));
    restored
        .poll_and_process(&mut source, &mut executor, 21)
        .await
        .unwrap();
    assert!(!restored.has_active_video());
    assert_ne!(
        source.pending_wait().map(|wait| wait.fence.as_str()),
        Some("movie.intro.end")
    );
    backend_task.await.unwrap();
}

#[astra_headless_test::tokio_test]
async fn shared_product_audio_host_owns_format_queue_control_and_cleanup() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let native_output = AudioOutputHandle::from_parts(3, 1).unwrap();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::QueryAudioOutputFormat { reply } => reply
                .send(Ok(AudioOutputFormat {
                    sample_rate: 48_000,
                    channels: 2,
                }))
                .unwrap(),
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::OpenAudioOutput { request, reply } => {
                assert_eq!(request.sample_rate, 48_000);
                assert_eq!(request.channels, 2);
                reply.send(Ok(native_output)).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        for sequence in 1..=4 {
            match backend.next_command().await.unwrap() {
                HostCommand::QueryAudio { output, reply } => {
                    assert_eq!(output, native_output);
                    reply
                        .send(Ok(AudioOutputState {
                            queued_frames: 0,
                            callback_count: sequence,
                            submitted_samples: 0,
                            consumed_samples: 0,
                            underflow_count: 64,
                            meter: AudioMeter {
                                sample_count: 0,
                                peak_dbfs: -120.0,
                                rms_dbfs: -120.0,
                            },
                        }))
                        .unwrap();
                }
                command => panic!("unexpected command: {}", command.operation()),
            }
            match backend.next_command().await.unwrap() {
                HostCommand::SubmitAudio { packet, reply, .. } => {
                    assert_eq!(packet.sequence, sequence);
                    assert_eq!(packet.channels, 2);
                    assert_eq!(packet.frame_count(), 800);
                    reply.send(Ok(())).unwrap();
                }
                command => panic!("unexpected command: {}", command.operation()),
            }
        }
        match backend.next_command().await.unwrap() {
            HostCommand::DrainAudio { output, reply } => {
                assert_eq!(output, native_output);
                reply
                    .send(Ok(AudioMeter {
                        sample_count: 2_048,
                        peak_dbfs: -6.0,
                        rms_dbfs: -9.0,
                    }))
                    .unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::CloseAudio { output, reply } => {
                assert_eq!(output, native_output);
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
    });
    let mut source = source();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let mut host = NativeVnProductAudioHost::default();
    let request = NativeVnAudioRequest {
        command_id: "bgm.main".into(),
        command: "bgm".into(),
        attributes: BTreeMap::from([("loop".into(), "true".into()), ("fade".into(), "40".into())]),
        asset_id: "asset:/bgm/main".into(),
        codec: "wav".into(),
        encoded_bytes: vec![1],
        encoded_hash: Hash256::from_sha256(&[1]),
    };
    let audio = PlayerDecodedAudio {
        sample_rate: 44_100,
        channels: 1,
        samples: vec![0.25; 4_410],
    };
    let mut signals = BTreeSet::new();

    host.start(&mut source, &mut executor, &request, audio, &mut signals)
        .await
        .unwrap();
    signals.insert("bgm.main.end".into());
    let mut replacement = request.clone();
    replacement.asset_id = "asset:/bgm/replacement".into();
    host.start(
        &mut source,
        &mut executor,
        &replacement,
        PlayerDecodedAudio {
            sample_rate: 44_100,
            channels: 1,
            samples: vec![0.125; 4_410],
        },
        &mut signals,
    )
    .await
    .unwrap();
    assert!(!signals.contains("bgm.main.end"));
    host.pump(&mut source, &mut executor, &mut signals)
        .await
        .unwrap();
    assert_eq!(host.last_meter().unwrap().underflow_count, 64);
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.pause".into(),
            action: "pause".into(),
            target: "bgm.main".into(),
            duration_ms: None,
            fence: None,
        },
        &mut signals,
    )
    .unwrap();
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.resume".into(),
            action: "resume".into(),
            target: "bgm.main".into(),
            duration_ms: None,
            fence: None,
        },
        &mut signals,
    )
    .unwrap();
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.fade-stop".into(),
            action: "fade_stop".into(),
            target: "bgm.main".into(),
            duration_ms: Some(40),
            fence: Some("bgm.fade.complete".into()),
        },
        &mut signals,
    )
    .unwrap();
    host.pump(&mut source, &mut executor, &mut signals)
        .await
        .unwrap();
    let snapshot = host.snapshot();
    host.restore(snapshot).unwrap();
    assert!(host.is_active());
    host.pump(&mut source, &mut executor, &mut signals)
        .await
        .unwrap();
    assert!(!signals.contains("bgm.fade.complete"));
    host.pump(&mut source, &mut executor, &mut signals)
        .await
        .unwrap();
    assert!(signals.contains("bgm.main.end"));
    assert!(signals.contains("bgm.fade.complete"));
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.fade-stop-again".into(),
            action: "fade_stop".into(),
            target: "bgm.main".into(),
            duration_ms: Some(40),
            fence: Some("bgm.second-fade.complete".into()),
        },
        &mut signals,
    )
    .unwrap();
    assert!(signals.contains("bgm.second-fade.complete"));
    let unknown = host
        .control(
            &NativeVnAudioControlRequest {
                command_id: "audio.fade-stop-unknown".into(),
                action: "fade_stop".into(),
                target: "bgm.unknown".into(),
                duration_ms: Some(40),
                fence: Some("bgm.unknown.complete".into()),
            },
            &mut signals,
        )
        .unwrap_err();
    assert!(unknown
        .to_string()
        .contains("ASTRA_PLAYER_AUDIO_CONTROL_TARGET_UNKNOWN"));
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.stop-after-fade".into(),
            action: "stop".into(),
            target: "bgm.main".into(),
            duration_ms: None,
            fence: None,
        },
        &mut signals,
    )
    .unwrap();
    host.shutdown(&mut source, &mut executor).await.unwrap();
    let final_meter = host.last_meter().unwrap();
    assert_eq!(final_meter.consumed_samples, 2_048);
    assert_eq!(f32::from_bits(final_meter.peak_dbfs_bits), -6.0);
    backend_task.await.unwrap();
}
