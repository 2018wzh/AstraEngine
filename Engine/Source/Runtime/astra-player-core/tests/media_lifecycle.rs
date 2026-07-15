use std::{future::Future, pin::Pin};

use astra_player_core::{
    PlayerAudioLifecyclePlan, PlayerDecodeKind, PlayerDecodeLifecyclePlan, PlayerHostCommand,
    PlayerHostCommandBatch, PlayerHostCommandExecutor, PlayerHostCommandResult,
    PlayerHostCommandSink, PlayerHostResourceId,
};

struct LifecycleSink {
    fail_operation: Option<&'static str>,
    invalid_operation: Option<&'static str>,
    seen: Vec<&'static str>,
}

impl PlayerHostCommandSink for LifecycleSink {
    type Error = &'static str;

    fn execute<'a>(
        &'a mut self,
        command: &'a PlayerHostCommand,
    ) -> Pin<Box<dyn Future<Output = Result<PlayerHostCommandResult, Self::Error>> + 'a>> {
        let (operation, result) = match command {
            PlayerHostCommand::OpenDecode { session, .. } => (
                "decode.open",
                PlayerHostCommandResult::DecodeOpened { session: *session },
            ),
            PlayerHostCommand::Decode { session, .. } => (
                "decode.submit",
                PlayerHostCommandResult::Decoded {
                    session: *session,
                    format: "pcm_s16le:48000:2".to_string(),
                    hash: astra_core::Hash256::from_sha256(&[0, 0, 0, 0]).to_string(),
                    bytes: vec![0, 0, 0, 0],
                },
            ),
            PlayerHostCommand::CloseDecode { session, .. } => (
                "decode.close",
                PlayerHostCommandResult::DecodeClosed { session: *session },
            ),
            PlayerHostCommand::OpenAudio { output, .. } => (
                "audio.open",
                PlayerHostCommandResult::AudioOpened { output: *output },
            ),
            PlayerHostCommand::SubmitAudio { .. } => {
                ("audio.submit", PlayerHostCommandResult::Unit)
            }
            PlayerHostCommand::DrainAudio { output, .. } => (
                "audio.drain",
                PlayerHostCommandResult::AudioDrained {
                    output: *output,
                    sample_count: 4,
                    peak_dbfs_bits: (-1.0_f32).to_bits(),
                    rms_dbfs_bits: (-3.0_f32).to_bits(),
                },
            ),
            PlayerHostCommand::CloseAudio { output, .. } => (
                "audio.close",
                PlayerHostCommandResult::AudioClosed { output: *output },
            ),
            _ => ("unexpected", PlayerHostCommandResult::Unit),
        };
        self.seen.push(operation);
        let result = if self.fail_operation == Some(operation) {
            Err(operation)
        } else if self.invalid_operation == Some(operation) {
            Ok(PlayerHostCommandResult::AudioOpened {
                output: PlayerHostResourceId(999),
            })
        } else {
            Ok(result)
        };
        Box::pin(std::future::ready(result))
    }
}

fn batch(command: PlayerHostCommand) -> PlayerHostCommandBatch {
    PlayerHostCommandBatch::new(vec![command]).unwrap()
}

fn decode_plan() -> PlayerDecodeLifecyclePlan {
    let session = PlayerHostResourceId(10);
    PlayerDecodeLifecyclePlan {
        session,
        open: batch(PlayerHostCommand::OpenDecode {
            sequence: 1,
            session,
            kind: PlayerDecodeKind::Audio,
        }),
        decode: batch(PlayerHostCommand::Decode {
            sequence: 2,
            request_sequence: 1,
            session,
            kind: PlayerDecodeKind::Audio,
            codec: "mp3".to_string(),
            description: vec![],
            sample_rate: None,
            channels: None,
            coded_width: None,
            coded_height: None,
            keyframe: true,
            bytes: vec![1],
        }),
        close: batch(PlayerHostCommand::CloseDecode {
            sequence: 3,
            session,
        }),
    }
}

#[astra_headless_test::tokio_test]
async fn decode_lifecycle_closes_after_submit_failure() {
    let mut executor = PlayerHostCommandExecutor::new(LifecycleSink {
        fail_operation: Some("decode.submit"),
        invalid_operation: None,
        seen: vec![],
    });

    let error = executor
        .execute_decode_lifecycle(decode_plan())
        .await
        .unwrap_err();

    assert_eq!(error.code(), "ASTRA_PLAYER_DECODE_SUBMIT");
    assert_eq!(
        executor.sink().seen,
        ["decode.open", "decode.submit", "decode.close"]
    );
}

#[astra_headless_test::tokio_test]
async fn decode_lifecycle_returns_validated_buffer_after_close() {
    let mut executor = PlayerHostCommandExecutor::new(LifecycleSink {
        fail_operation: None,
        invalid_operation: None,
        seen: vec![],
    });

    let decoded = executor
        .execute_decode_lifecycle(decode_plan())
        .await
        .unwrap();

    assert_eq!(decoded.format, "pcm_s16le:48000:2");
    assert_eq!(decoded.bytes, [0, 0, 0, 0]);
    assert_eq!(
        executor.sink().seen,
        ["decode.open", "decode.submit", "decode.close"]
    );
}

#[astra_headless_test::tokio_test]
async fn audio_lifecycle_drains_and_closes_with_same_run_meter() {
    let output = PlayerHostResourceId(20);
    let plan = PlayerAudioLifecyclePlan {
        output,
        expected_sample_count: 4,
        open: batch(PlayerHostCommand::OpenAudio {
            sequence: 1,
            output,
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 2,
        }),
        submits: vec![batch(PlayerHostCommand::SubmitAudio {
            sequence: 2,
            output,
            packet_sequence: 1,
            channels: 2,
            samples: vec![0.0; 4],
        })],
        drain: batch(PlayerHostCommand::DrainAudio {
            sequence: 3,
            output,
        }),
        close: batch(PlayerHostCommand::CloseAudio {
            sequence: 4,
            output,
        }),
    };
    let mut executor = PlayerHostCommandExecutor::new(LifecycleSink {
        fail_operation: None,
        invalid_operation: None,
        seen: vec![],
    });

    let meter = executor.execute_audio_lifecycle(plan).await.unwrap();

    assert_eq!(meter.sample_count, 4);
    assert_eq!(meter.peak_dbfs, -1.0);
    assert_eq!(meter.rms_dbfs, -3.0);
    assert_eq!(
        executor.sink().seen,
        ["audio.open", "audio.submit", "audio.drain", "audio.close"]
    );
}

#[astra_headless_test::tokio_test]
async fn audio_lifecycle_rejects_invalid_submit_result_and_closes() {
    let output = PlayerHostResourceId(20);
    let plan = PlayerAudioLifecyclePlan {
        output,
        expected_sample_count: 2,
        open: batch(PlayerHostCommand::OpenAudio {
            sequence: 1,
            output,
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 1,
        }),
        submits: vec![batch(PlayerHostCommand::SubmitAudio {
            sequence: 2,
            output,
            packet_sequence: 1,
            channels: 2,
            samples: vec![0.0; 2],
        })],
        drain: batch(PlayerHostCommand::DrainAudio {
            sequence: 3,
            output,
        }),
        close: batch(PlayerHostCommand::CloseAudio {
            sequence: 4,
            output,
        }),
    };
    let mut executor = PlayerHostCommandExecutor::new(LifecycleSink {
        fail_operation: None,
        invalid_operation: Some("audio.submit"),
        seen: vec![],
    });

    let error = executor.execute_audio_lifecycle(plan).await.unwrap_err();

    assert_eq!(error.code(), "ASTRA_PLAYER_AUDIO_SUBMIT_RESULT");
    assert_eq!(
        executor.sink().seen,
        ["audio.open", "audio.submit", "audio.close"]
    );
}
