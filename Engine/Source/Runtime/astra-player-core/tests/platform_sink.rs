use astra_media_core::{BlendMode, GlyphBitmap, GlyphBitmapFormat, SceneCommand};
use astra_platform::{
    host_channel, AudioMeter, AudioOutputFormat, AudioOutputHandle, AudioOutputState, HostCommand,
    PlatformHostProfile, SaveTransactionHandle, SurfaceHandle,
};
use astra_player_core::{
    PlatformCommandSink, PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandExecutor,
    PlayerHostCommandResult, PlayerHostResourceId,
};

#[tokio::test]
async fn platform_sink_keeps_native_save_handles_out_of_results() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 8, 8).unwrap();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::BeginSave { reply, .. } => {
                reply
                    .send(Ok(SaveTransactionHandle::from_parts(7, 3).unwrap()))
                    .unwrap();
            }
            _ => panic!("unexpected command"),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::CommitSave { reply, .. } => {
                reply.send(Ok("sha256:save".to_string())).unwrap();
            }
            _ => panic!("unexpected command"),
        }
    });
    let batch = PlayerHostCommandBatch::new(vec![
        PlayerHostCommand::BeginSave {
            sequence: 1,
            slot: "slot-main".to_string(),
            transaction: PlayerHostResourceId(11),
        },
        PlayerHostCommand::CommitSave {
            sequence: 2,
            transaction: PlayerHostResourceId(11),
        },
    ])
    .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let results = executor.execute_batch(batch).await.unwrap();
    assert_eq!(
        results[0],
        PlayerHostCommandResult::SaveStarted {
            transaction: PlayerHostResourceId(11)
        }
    );
    assert_eq!(
        results[1],
        PlayerHostCommandResult::SaveCommitted {
            transaction: PlayerHostResourceId(11),
            hash: "sha256:save".to_string()
        }
    );
    backend_task.await.unwrap();
}

#[tokio::test]
async fn platform_sink_exposes_bounded_audio_queue_state_without_native_handles() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 8, 8).unwrap();
    let native = AudioOutputHandle::from_parts(4, 2).unwrap();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::QueryAudioOutputFormat { reply } => reply
                .send(Ok(AudioOutputFormat {
                    sample_rate: 48_000,
                    channels: 2,
                }))
                .unwrap(),
            _ => panic!("unexpected command"),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::OpenAudioOutput { reply, .. } => reply.send(Ok(native)).unwrap(),
            _ => panic!("unexpected command"),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::QueryAudio { output, reply } => {
                assert_eq!(output, native);
                reply
                    .send(Ok(AudioOutputState {
                        queued_frames: 256,
                        callback_count: 1,
                        submitted_samples: 1_024,
                        consumed_samples: 512,
                        underflow_count: 3,
                        meter: AudioMeter {
                            sample_count: 512,
                            peak_dbfs: -2.0,
                            rms_dbfs: -6.0,
                        },
                    }))
                    .unwrap();
            }
            _ => panic!("unexpected command"),
        }
    });
    let logical = PlayerHostResourceId(77);
    let batch = PlayerHostCommandBatch::new(vec![
        PlayerHostCommand::QueryAudioFormat { sequence: 1 },
        PlayerHostCommand::OpenAudio {
            sequence: 2,
            output: logical,
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 8_192,
        },
        PlayerHostCommand::QueryAudio {
            sequence: 3,
            output: logical,
        },
    ])
    .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));

    let results = executor.execute_batch(batch).await.unwrap();

    assert!(matches!(
        results.as_slice(),
        [
            PlayerHostCommandResult::AudioFormat {
                sample_rate: 48_000,
                channels: 2,
            },
            PlayerHostCommandResult::AudioOpened { output },
            PlayerHostCommandResult::AudioState {
                output: state_output,
                queued_frames: 256,
                submitted_samples: 1_024,
                consumed_samples: 512,
                underflow_count: 3,
                ..
            }
        ] if *output == logical && *state_output == logical
    ));
    backend_task.await.unwrap();
}

#[tokio::test]
async fn platform_sink_forwards_renderer_ready_glyph_commands_without_cpu_frames() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 8, 8).unwrap();
    let logical = PlayerHostResourceId(9);
    let native = SurfaceHandle::from_parts(4, 2).unwrap();
    let pixels = vec![255_u8; 4];
    let hash = astra_core::Hash256::from_sha256(&pixels);
    let commands = vec![
        SceneCommand::UploadGlyph {
            resource_id: "glyph:test".into(),
            glyph: GlyphBitmap {
                width: 2,
                height: 2,
                format: GlyphBitmapFormat::Alpha8,
                pixels,
                hash,
            },
        },
        SceneCommand::GlyphRun {
            id: "layout:test".into(),
            glyphs: vec![astra_media_core::GlyphInstance {
                resource_id: "glyph:test".into(),
                x: 4,
                y: 5,
            }],
            rgba: [255; 4],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
    ];
    let expected = commands.clone();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                assert_eq!(surface, native);
                assert_eq!(frame.sequence, 1);
                assert_eq!(frame.width, 64);
                assert_eq!(frame.height, 32);
                assert_eq!(frame.clear_rgba, [3, 5, 8, 255]);
                assert_eq!(frame.commands, expected);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected command"),
        }
    });
    let mut sink = PlatformCommandSink::new(client);
    sink.bind_surface(logical, native).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let results = executor
        .execute_batch(
            PlayerHostCommandBatch::new(vec![PlayerHostCommand::PresentScene {
                sequence: 1,
                surface: logical,
                width: 64,
                height: 32,
                clear_rgba: [3, 5, 8, 255],
                commands,
            }])
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        results,
        [PlayerHostCommandResult::Presented { surface: logical }]
    );
    backend_task.await.unwrap();
}
