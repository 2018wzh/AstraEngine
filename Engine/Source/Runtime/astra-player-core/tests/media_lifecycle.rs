use std::{future::Future, pin::Pin};

use astra_player_core::{
    PlayerDecodeKind, PlayerDecodeLifecyclePlan, PlayerHostCommand, PlayerHostCommandBatch,
    PlayerHostCommandExecutor, PlayerHostCommandResult, PlayerHostCommandSink,
    PlayerHostResourceId,
};

struct LifecycleSink {
    fail_operation: Option<&'static str>,
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
                    output: astra_platform::DecodeOutput::AudioPcmI16 {
                        sample_rate: 48_000,
                        channels: 2,
                        samples: vec![0, 0],
                    },
                },
            ),
            PlayerHostCommand::CloseDecode { session, .. } => (
                "decode.close",
                PlayerHostCommandResult::DecodeClosed { session: *session },
            ),
            _ => ("unexpected", PlayerHostCommandResult::Unit),
        };
        self.seen.push(operation);
        let result = if self.fail_operation == Some(operation) {
            Err(operation)
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
            stream_action: astra_player_core::PlayerDecodeStreamAction::OneShot,
            bytes: vec![1].into(),
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
        seen: vec![],
    });

    let decoded = executor
        .execute_decode_lifecycle(decode_plan())
        .await
        .unwrap();

    assert!(matches!(
        decoded.output,
        astra_platform::DecodeOutput::AudioPcmI16 {
            sample_rate: 48_000,
            channels: 2,
            samples,
        } if samples == [0, 0]
    ));
    assert_eq!(
        executor.sink().seen,
        ["decode.open", "decode.submit", "decode.close"]
    );
}
