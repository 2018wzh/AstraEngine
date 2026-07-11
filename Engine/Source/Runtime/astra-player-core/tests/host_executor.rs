use std::{future::Future, pin::Pin};

use astra_player_core::{
    PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandExecutor, PlayerHostCommandResult,
    PlayerHostCommandSink, PlayerHostResourceId, PlayerSaveTransactionError,
    PlayerSaveTransactionPlan,
};

struct RecordingSink {
    seen: Vec<u64>,
}

struct FailingSaveSink {
    seen: Vec<&'static str>,
}

impl PlayerHostCommandSink for FailingSaveSink {
    type Error = &'static str;

    fn execute<'a>(
        &'a mut self,
        command: &'a PlayerHostCommand,
    ) -> Pin<Box<dyn Future<Output = Result<PlayerHostCommandResult, Self::Error>> + 'a>> {
        let (kind, result) = match command {
            PlayerHostCommand::BeginSave { .. } => ("begin", Ok(PlayerHostCommandResult::Unit)),
            PlayerHostCommand::WriteSave { .. } => ("write", Err("write failed")),
            PlayerHostCommand::AbortSave { .. } => ("abort", Ok(PlayerHostCommandResult::Unit)),
            PlayerHostCommand::CommitSave { .. } => ("commit", Ok(PlayerHostCommandResult::Unit)),
            _ => ("unexpected", Err("unexpected command")),
        };
        self.seen.push(kind);
        Box::pin(std::future::ready(result))
    }
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

#[tokio::test]
async fn save_transaction_aborts_after_write_failure() {
    let transaction = PlayerHostResourceId(9);
    let batch = |command| PlayerHostCommandBatch::new(vec![command]).unwrap();
    let plan = PlayerSaveTransactionPlan {
        begin: batch(PlayerHostCommand::BeginSave {
            sequence: 1,
            slot: "slot-main".to_string(),
            transaction,
        }),
        write: batch(PlayerHostCommand::WriteSave {
            sequence: 2,
            transaction,
            bytes: vec![1],
        }),
        commit: batch(PlayerHostCommand::CommitSave {
            sequence: 3,
            transaction,
        }),
        abort: batch(PlayerHostCommand::AbortSave {
            sequence: 4,
            transaction,
        }),
    };
    let mut executor = PlayerHostCommandExecutor::new(FailingSaveSink { seen: Vec::new() });

    let error = executor.execute_save_transaction(plan).await.unwrap_err();

    assert!(matches!(
        error,
        PlayerSaveTransactionError::Write {
            source: _,
            abort: None
        }
    ));
    assert_eq!(executor.sink().seen, ["begin", "write", "abort"]);
}
