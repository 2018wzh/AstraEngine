use astra_player_core::{
    PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandError, PlayerHostResourceId,
};

#[test]
fn command_batch_requires_strictly_increasing_sequences() {
    let error = PlayerHostCommandBatch::new(vec![
        PlayerHostCommand::BeginSave {
            sequence: 1,
            slot: "slot-main".to_string(),
            transaction: PlayerHostResourceId(1),
        },
        PlayerHostCommand::CommitSave {
            sequence: 1,
            transaction: PlayerHostResourceId(1),
        },
    ])
    .unwrap_err();

    assert_eq!(error, PlayerHostCommandError::SequenceNotStrictlyIncreasing);
}
