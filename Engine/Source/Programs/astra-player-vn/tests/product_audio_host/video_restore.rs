use super::*;

#[tokio::test]
pub(super) async fn shared_product_audio_host_owns_format_queue_control_and_cleanup() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let native_output = AudioOutputHandle::from_parts(3, 1).unwrap();
    let consumed_samples = Arc::new(AtomicU64::new(0));
    let backend_consumed_samples = Arc::clone(&consumed_samples);
    let lane_dropped = Arc::new(AtomicBool::new(false));
    let close_requested = Arc::new(AtomicBool::new(false));
    let submissions_after_close = Arc::new(AtomicU64::new(0));
    let backend_lane_dropped = Arc::clone(&lane_dropped);
    let backend_close_lane_dropped = Arc::clone(&lane_dropped);
    let backend_close_requested = Arc::clone(&close_requested);
    let backend_close_flag = Arc::clone(&close_requested);
    let backend_submissions_after_close = Arc::clone(&submissions_after_close);
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::OpenAudioOutput { request, reply } => {
                assert_eq!(request.sample_rate, 48_000);
                assert_eq!(request.channels, 2);
                reply
                    .send(Ok(OpenedAudioOutput {
                        handle: native_output,
                        format: AudioDeviceFormat {
                            sample_rate: 48_000,
                            channels: 2,
                        },
                        lane: Box::new(ProductAudioTestLane {
                            consumed_samples: backend_consumed_samples,
                            underflow_count: 64,
                            dropped: backend_lane_dropped,
                            close_requested: backend_close_requested,
                            submissions_after_close: backend_submissions_after_close,
                        }),
                        capture: None,
                    }))
                    .unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::CloseAudio { output, reply } => {
                assert_eq!(output, native_output);
                assert!(backend_close_lane_dropped.load(Ordering::Acquire));
                backend_close_flag.store(true, Ordering::Release);
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
        target_id: "bgm.authored".into(),
        command: "bgm".into(),
        attributes: BTreeMap::from([("loop".into(), "true".into()), ("fade".into(), "40".into())]),
        asset_id: "asset:/bgm/main".into(),
        codec: "wav".into(),
        encoded_bytes: vec![1].into(),
        encoded_length: 1,
    };
    let audio = PlayerDecodedAudio {
        sample_rate: 44_100,
        channels: 1,
        samples: vec![0.25; 4_410],
    };
    let mut signals = BTreeSet::new();
    let mut previous_endpoint_consumed = 0;

    host.start(&mut source, &mut executor, &request, audio, &mut signals)
        .await
        .unwrap();
    signals.insert("bgm.authored.end".into());
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
    assert!(!signals.contains("bgm.authored.end"));
    host.pump(&mut source, &mut executor, &mut signals, false)
        .await
        .unwrap();
    assert_endpoint_consumed_is_monotonic(&consumed_samples, &mut previous_endpoint_consumed);
    assert_eq!(host.last_meter().unwrap().underflow_count, 64);
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.pause".into(),
            action: "pause".into(),
            target: "bgm.authored".into(),
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
            target: "bgm.authored".into(),
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
            target: "bgm.authored".into(),
            duration_ms: Some(40),
            fence: Some("bgm.fade.complete".into()),
        },
        &mut signals,
    )
    .unwrap();
    host.pump(&mut source, &mut executor, &mut signals, false)
        .await
        .unwrap();
    assert_endpoint_consumed_is_monotonic(&consumed_samples, &mut previous_endpoint_consumed);
    let snapshot = host.snapshot();
    host.restore(snapshot).unwrap();
    assert!(host.is_active());
    host.pump(&mut source, &mut executor, &mut signals, false)
        .await
        .unwrap();
    assert!(!signals.contains("bgm.fade.complete"));
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !signals.contains("bgm.authored.end") || !signals.contains("bgm.fade.complete") {
            host.pump(&mut source, &mut executor, &mut signals, false)
                .await
                .unwrap();
            assert_endpoint_consumed_is_monotonic(
                &consumed_samples,
                &mut previous_endpoint_consumed,
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("device-consumed completion reaches the next fixed tick");
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.fade-stop-again".into(),
            action: "fade_stop".into(),
            target: "bgm.authored".into(),
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
    assert_eq!(unknown.operation, "player.audio.fade_stop");
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.stop-after-fade".into(),
            action: "stop".into(),
            target: "bgm.authored".into(),
            duration_ms: None,
            fence: None,
        },
        &mut signals,
    )
    .unwrap();
    host.shutdown(&mut source, &mut executor).await.unwrap();
    let final_meter = host.last_meter().unwrap();
    let endpoint_consumed = consumed_samples.load(Ordering::Acquire);
    assert_endpoint_consumed_is_monotonic(&consumed_samples, &mut previous_endpoint_consumed);
    assert!(final_meter.consumed_samples <= endpoint_consumed);
    assert!(final_meter.submitted_samples <= endpoint_consumed);
    assert!(final_meter.submitted_samples > 0);
    backend_task.await.unwrap();
    assert!(lane_dropped.load(Ordering::Acquire));
    assert_eq!(submissions_after_close.load(Ordering::Acquire), 0);
}

#[test]
pub(super) fn video_frame_commands_carry_current_pixels_and_dimensions() {
    use astra_media_core::{SceneCommand, TextureFrame};
    let mut source = support::source_for_video(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    stage viewport:320x180 safe_area:16:9 #@id stage.main\n    layer id:video kind:video z:100 blend:normal clip:stage #@id layer.video\n    movie layer:video asset:asset:/video/intro loop:true end:wait fence:movie.intro.end fallback:asset:/video/intro-fallback interrupt:reject #@id movie.intro\n    text key:line.after #@id line.after\n",
    );
    source.launch().unwrap();
    let request = source.take_video_requests().remove(0);
    for (index, width) in [1, 1, 2].into_iter().enumerate() {
        let pixels = vec![index as u8 + 1; width * 4];
        let batch = source
            .bind_decoded_video_frame(
                &request,
                TextureFrame::from_vec(width as u32, 1, pixels.clone()).unwrap(),
                false,
            )
            .unwrap();
        let [astra_player_core::PlayerHostCommand::PresentScene { commands, .. }] =
            batch.commands.as_slice()
        else {
            panic!("expected scene")
        };
        assert!(commands.iter().any(|command| matches!(command,
            SceneCommand::VideoFrame { id, frame, .. }
            if id == "vn.movie.video" && frame.width == width as u32 && frame.rgba8.to_vec() == pixels
        )));
    }
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[tokio::test]
pub(super) async fn cold_product_restore_prepares_packaged_pcm_and_preserves_saved_cursor() {
    use astra_audio_kira::{AudioAssetRevision, AudioBusState, AudioVoiceState};
    let mut source = support::source_for_audio(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
    );
    source.launch().unwrap();
    source
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    source
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 0)
        .unwrap();
    let mut media = NativeVnProductMediaHost::default();
    let mut saved = media.snapshot();
    saved.audio.timeline.command_sequence = 1;
    saved.audio.timeline.buses.insert(
        "bgm".into(),
        AudioBusState {
            gain: 0.5,
            fade_id: None,
            fade_sequence: 0,
            fade_start_gain: None,
            fade_target_gain: None,
            fade_total_frames: 0,
            fade_rendered_frames: 0,
        },
    );
    saved.audio.timeline.voices.insert(
        "saved.voice".into(),
        AudioVoiceState {
            command_sequence: 1,
            bus: "bgm".into(),
            asset: AudioAssetRevision {
                package_id: "com.example.player.audio".into(),
                uri: "asset:/audio/restore".into(),
                revision: "com.example.player.audio".into(),
                byte_len: 256,
            },
            cursor_frames: 8,
            looping: true,
            paused: true,
        },
    );
    saved
        .audio
        .voice_kinds
        .insert("saved.voice".into(), "bgm".into());
    let bytes = source
        .save_with_product_media_snapshot("slot.01", Some(serde_json::to_vec(&saved).unwrap()))
        .unwrap();
    let mut foreign = saved.clone();
    foreign
        .audio
        .timeline
        .voices
        .get_mut("saved.voice")
        .unwrap()
        .asset
        .package_id = "foreign".into();
    let foreign_bytes = source
        .save_with_product_media_snapshot("slot.01", Some(serde_json::to_vec(&foreign).unwrap()))
        .unwrap();
    let mut wrong_length = saved.clone();
    wrong_length
        .audio
        .timeline
        .voices
        .get_mut("saved.voice")
        .unwrap()
        .asset
        .byte_len += 4;
    let wrong_length_bytes = source
        .save_with_product_media_snapshot(
            "slot.01",
            Some(serde_json::to_vec(&wrong_length).unwrap()),
        )
        .unwrap();
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.player.audio");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let task = tokio::spawn(async move {
        let HostCommand::OpenAudioOutput { reply, .. } = backend.next_command().await.unwrap()
        else {
            panic!("expected audio open")
        };
        let output = AudioOutputHandle::from_parts(3, 1).unwrap();
        reply
            .send(Ok(OpenedAudioOutput {
                handle: output,
                format: AudioDeviceFormat {
                    sample_rate: 48_000,
                    channels: 2,
                },
                lane: Box::new(ProductAudioTestLane::new(Arc::new(AtomicU64::new(0)), 0)),
                capture: None,
            }))
            .unwrap();
        let HostCommand::OpenDecode { kind, reply } = backend.next_command().await.unwrap() else {
            panic!("expected audio decoder")
        };
        assert_eq!(kind, DecodeKind::Audio);
        reply
            .send(Ok(DecodeSessionHandle::from_parts(9, 1).unwrap()))
            .unwrap();
        let HostCommand::Decode { request, reply, .. } = backend.next_command().await.unwrap()
        else {
            panic!("expected packaged audio decode")
        };
        assert!(request.bytes.starts_with(b"RIFF"));
        reply
            .send(Ok(DecodeOutput::AudioPcmF32 {
                sample_rate: 48_000,
                channels: 2,
                samples: vec![0.0; 64],
            }))
            .unwrap();
        let HostCommand::CloseDecode { reply, .. } = backend.next_command().await.unwrap() else {
            panic!("expected decoder close")
        };
        reply.send(Ok(())).unwrap();
        // Each successful restore closes the old queue before opening a fresh endpoint.
        // Prepared PCM survives both cycles without another decoder request.
        let mut current = output;
        for generation in 2..=3 {
            let HostCommand::CloseAudio { output, reply } = backend.next_command().await.unwrap()
            else {
                panic!("expected old output close")
            };
            assert_eq!(output, current);
            reply.send(Ok(())).unwrap();
            let HostCommand::OpenAudioOutput { reply, .. } = backend.next_command().await.unwrap()
            else {
                panic!("expected new output after close")
            };
            current = AudioOutputHandle::from_parts(3, generation).unwrap();
            reply
                .send(Ok(OpenedAudioOutput {
                    handle: current,
                    format: AudioDeviceFormat {
                        sample_rate: 48_000,
                        channels: 2,
                    },
                    lane: Box::new(ProductAudioTestLane::new(Arc::new(AtomicU64::new(0)), 0)),
                    capture: None,
                }))
                .unwrap();
        }

        let HostCommand::CloseAudio { output, reply } = backend.next_command().await.unwrap()
        else {
            panic!("expected failing restore close")
        };
        assert_eq!(output, current);
        reply
            .send(Err(PlatformError::new(
                astra_platform::PlatformErrorCode::DeviceLost,
                "audio.close",
                "test endpoint close failed",
            )))
            .unwrap();
        let HostCommand::CloseAudio { output, reply } = backend.next_command().await.unwrap()
        else {
            panic!("expected retained handle cleanup")
        };
        assert_eq!(output, current);
        reply.send(Ok(())).unwrap();
    });
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    assert!(source
        .restore_product_session(&foreign_bytes, &mut media, &mut executor)
        .await
        .unwrap_err()
        .to_string()
        .contains("REVISION_CONFLICT"));
    assert!(media.snapshot().audio.timeline.voices.is_empty());
    source
        .restore_product_session(&bytes, &mut media, &mut executor)
        .await
        .unwrap();
    let actual = media.snapshot().audio;
    assert_eq!(actual.timeline.voices, saved.audio.timeline.voices);
    assert_eq!(actual.timeline.buses, saved.audio.timeline.buses);
    let wait = source.pending_wait().cloned();
    assert!(source
        .restore_product_session(&wrong_length_bytes, &mut media, &mut executor)
        .await
        .unwrap_err()
        .to_string()
        .contains("PCM_LENGTH_MISMATCH"));
    assert_eq!(source.pending_wait(), wait.as_ref());
    assert_eq!(
        media.snapshot().audio.timeline.voices,
        saved.audio.timeline.voices
    );

    source
        .restore_product_session(&bytes, &mut media, &mut executor)
        .await
        .unwrap();
    assert_eq!(
        media.snapshot().audio.timeline.voices,
        saved.audio.timeline.voices
    );
    let error = source
        .restore_product_session(&bytes, &mut media, &mut executor)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_PLAYER_MEDIA_RESTORE_FAILED"));
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_PRESENTATION_SESSION_FAILED"));
    media.shutdown(&mut source, &mut executor).await.unwrap();
    source.release_resources().unwrap();
    source.shutdown().unwrap();
    task.await.unwrap();
}
