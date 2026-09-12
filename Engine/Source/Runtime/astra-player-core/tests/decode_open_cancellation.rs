use astra_platform::{
    host_channel, DecodeSessionHandle, HostCommand, PlatformBackendChannels, PlatformError,
    PlatformErrorCode, PlatformHostProfile,
};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodeKind, PlayerHostCommand, PlayerHostCommandBatch,
    PlayerHostCommandExecutor, PlayerHostResourceId,
};
use tokio::sync::oneshot;

type Executor = PlayerHostCommandExecutor<PlatformCommandSink>;
type OpenReply = oneshot::Sender<Result<DecodeSessionHandle, PlatformError>>;

fn setup() -> (Executor, PlatformBackendChannels) {
    let (client, backend, _) = host_channel(
        PlatformHostProfile::windows_release("nativevn-game", "com.example.game"),
        8,
        8,
    )
    .unwrap();
    (
        PlayerHostCommandExecutor::new(PlatformCommandSink::new(client)),
        backend,
    )
}

fn open(logical: u64) -> PlayerHostCommandBatch {
    PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenDecode {
        sequence: logical + 1,
        session: PlayerHostResourceId(logical),
        kind: PlayerDecodeKind::Video,
    }])
    .unwrap()
}

async fn abandon_open(
    executor: &mut Executor,
    backend: &mut PlatformBackendChannels,
    logical: u64,
) -> OpenReply {
    let work = executor.execute_batch(open(logical));
    tokio::pin!(work);
    tokio::select! {
        command = backend.next_command() => {
            let HostCommand::OpenDecode { reply, .. } = command.unwrap() else {
                panic!("expected decoder open")
            };
            reply
        },
        result = &mut work => panic!("open completed before the host replied: {result:?}"),
    }
}

#[tokio::test]
async fn abandoned_open_and_interrupted_cleanup_keep_both_responses_owned() {
    let (mut executor, mut backend) = setup();
    let reply = abandon_open(&mut executor, &mut backend, 1).await;
    let native = DecodeSessionHandle::from_parts(4, 2).unwrap();
    assert!(executor.sink().has_live_resources());
    {
        let cleanup = executor.sink_mut().cleanup_pending_decode_opens();
        tokio::pin!(cleanup);
        tokio::select! {
            biased;
            result = &mut cleanup => panic!("cleanup cannot finish before open reply: {result:?}"),
            _ = std::future::ready(()) => {},
        }
    }
    reply.send(Ok(native)).unwrap();
    let close_reply = {
        let cleanup = executor.sink_mut().cleanup_pending_decode_opens();
        tokio::pin!(cleanup);
        tokio::select! {
            command = backend.next_command() => {
                let HostCommand::CloseDecode { session, reply } = command.unwrap() else {
                    panic!("expected abandoned decoder close")
                };
                assert_eq!(session, native);
                reply
            },
            result = &mut cleanup => panic!("cleanup completed before close reply: {result:?}"),
        }
    };
    close_reply.send(Ok(())).unwrap();
    assert!(executor.sink().has_live_resources());
    executor
        .sink_mut()
        .cleanup_pending_decode_opens()
        .await
        .unwrap();
    assert!(!executor.sink().has_live_resources());
}

#[tokio::test]
async fn cleanup_failure_retains_handle_and_does_not_reopen_decoder() {
    let (mut executor, mut backend) = setup();
    let reply = abandon_open(&mut executor, &mut backend, 1).await;
    let native = DecodeSessionHandle::from_parts(4, 2).unwrap();
    reply.send(Ok(native)).unwrap();
    for attempt in 0..2 {
        let result = {
            let cleanup = executor.sink_mut().cleanup_pending_decode_opens();
            tokio::pin!(cleanup);
            tokio::select! {
                command = backend.next_command() => {
                    let HostCommand::CloseDecode { session, reply } = command.unwrap() else {
                        panic!("expected close retry")
                    };
                    assert_eq!(session, native);
                    reply.send(if attempt == 0 {
                        Err(PlatformError::new(PlatformErrorCode::Io, "decode.close", "test close failure"))
                    } else { Ok(()) }).unwrap();
                },
                result = &mut cleanup => panic!("cleanup ended before command: {result:?}"),
            }
            cleanup.await
        };
        assert_eq!(result.is_err(), attempt == 0);
        assert_eq!(executor.sink().has_live_resources(), attempt == 0);
    }
}

#[tokio::test]
async fn pending_open_budget_and_logical_identity_are_checked_before_dispatch() {
    let (mut executor, mut backend) = setup();
    let mut replies = Vec::new();
    for logical in 0..64 {
        replies.push(abandon_open(&mut executor, &mut backend, logical).await);
    }
    assert!(executor
        .execute_batch(open(0))
        .await
        .unwrap_err()
        .to_string()
        .contains("already exists"));
    assert!(executor
        .execute_batch(open(64))
        .await
        .unwrap_err()
        .to_string()
        .contains("budget exceeded"));
    for reply in replies {
        reply
            .send(Err(PlatformError::new(
                PlatformErrorCode::Io,
                "decode.open",
                "test open failure",
            )))
            .unwrap();
        assert!(executor
            .sink_mut()
            .cleanup_pending_decode_opens()
            .await
            .is_err());
    }
    assert!(!executor.sink().has_live_resources());
}
