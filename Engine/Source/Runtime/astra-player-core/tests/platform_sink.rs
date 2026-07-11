use astra_platform::{
    host_channel, AudioMeter, AudioOutputHandle, AudioOutputState, HostCommand,
    PlatformHostProfile, SaveTransactionHandle,
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
            HostCommand::OpenAudioOutput { reply, .. } => reply.send(Ok(native)).unwrap(),
            _ => panic!("unexpected command"),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::QueryAudio { output, reply } => {
                assert_eq!(output, native);
                reply
                    .send(Ok(AudioOutputState {
                        queued_frames: 256,
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
        PlayerHostCommand::OpenAudio {
            sequence: 1,
            output: logical,
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 8_192,
        },
        PlayerHostCommand::QueryAudio {
            sequence: 2,
            output: logical,
        },
    ])
    .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));

    let results = executor.execute_batch(batch).await.unwrap();

    assert!(matches!(
        results.as_slice(),
        [
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
