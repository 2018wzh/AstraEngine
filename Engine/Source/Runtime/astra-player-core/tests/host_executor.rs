use std::{future::Future, pin::Pin};

use astra_player_core::{
    PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandExecutor, PlayerHostCommandResult,
    PlayerHostCommandSink, PlayerHostResourceId,
};

struct RecordingSink {
    seen: Vec<u64>,
}

impl PlayerHostCommandSink for RecordingSink {
    type Error = &'static str;

    fn execute<'a>(
        &'a mut self,
        command: &'a PlayerHostCommand,
    ) -> Pin<Box<dyn Future<Output = Result<PlayerHostCommandResult, Self::Error>> + 'a>> {
        self.seen.push(command.sequence());
        Box::pin(std::future::ready(Ok(
            PlayerHostCommandResult::SaveCommitted {
                transaction: PlayerHostResourceId(1),
                hash: "sha256:test".to_string(),
            },
        )))
    }
}

#[tokio::test]
async fn executor_preserves_command_order_and_logical_results() {
    let batch = PlayerHostCommandBatch::new(vec![
        PlayerHostCommand::BeginSave {
            sequence: 1,
            slot: "slot-main".to_string(),
            transaction: PlayerHostResourceId(1),
        },
        PlayerHostCommand::CommitSave {
            sequence: 2,
            transaction: PlayerHostResourceId(1),
        },
    ])
    .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(RecordingSink { seen: Vec::new() });
    let results = executor.execute_batch(batch).await.unwrap();
    assert_eq!(executor.sink().seen, [1, 2]);
    assert_eq!(results.len(), 2);
}
