use astra_platform::{host_channel, HostCommand, PlatformHostProfile, SaveTransactionHandle};
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
