use astra_player_core::{
    PlayerAction, PlayerActionMap, PlayerHostCommand, PlayerHostCommandBatch,
    PlayerHostCommandError, PlayerHostResourceId,
};

#[test]
fn standard_action_map_exposes_real_backlog_key() {
    assert_eq!(
        PlayerActionMap::standard().keyboard("KeyB"),
        Some(PlayerAction::OpenSystemPage {
            page: "backlog".to_string()
        })
    );
}

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

#[test]
fn decode_request_sequence_is_independent_from_command_order() {
    let command = PlayerHostCommand::Decode {
        sequence: 9,
        request_sequence: 1,
        session: PlayerHostResourceId(2),
        kind: astra_player_core::PlayerDecodeKind::Video,
        codec: "vp09.00.10.08".to_string(),
        description: Vec::new(),
        sample_rate: None,
        channels: None,
        coded_width: Some(16),
        coded_height: Some(16),
        keyframe: true,
        bytes: vec![1],
    };
    assert_eq!(command.sequence(), 9);
    assert!(matches!(
        command,
        PlayerHostCommand::Decode {
            request_sequence: 1,
            ..
        }
    ));
}
