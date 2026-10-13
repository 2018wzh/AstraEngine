use std::io::{BufReader, Cursor};

use astra_headless_protocol::{
    AudioTolerance, ButtonState, CheckpointConfig, CheckpointExpectation, ImageTolerance,
    InputMessage, JsonlReader, JsonlWriter, PhysicalInput, SequenceValidator,
    HEADLESS_CHECKPOINT_CONFIG_SCHEMA, USER_INPUT_SEQUENCE_SCHEMA,
};

fn hash(label: &str) -> String {
    format!("sha256:{:064x}", label.len())
}

#[test]
fn file_and_stdio_framing_share_one_strict_jsonl_contract() {
    let message = InputMessage {
        schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
        session: "run.one".into(),
        sequence: 1,
        tick: 0,
        event: PhysicalInput::Keyboard {
            physical_key: "Enter".into(),
            logical_key: Some("Enter".into()),
            state: ButtonState::Pressed,
            repeat: false,
        },
    };
    let mut bytes = Vec::new();
    JsonlWriter::new(&mut bytes).write(&message).unwrap();
    let decoded = JsonlReader::new(BufReader::new(Cursor::new(bytes)), 4096)
        .unwrap()
        .read::<InputMessage>()
        .unwrap()
        .unwrap();
    assert_eq!(decoded, message);
    decoded.validate().unwrap();
}

#[test]
fn sequence_validator_blocks_duplicates_reversal_and_cross_session_input() {
    let mut validator = SequenceValidator::default();
    validator.accept("a", 2, 0).unwrap();
    assert!(validator.accept("a", 2, 0).is_err());
    assert!(validator.accept("a", 4, 0).is_ok());
    assert!(validator.accept("a", 3, 1).is_err());
    assert!(validator.accept("a", 5, 1).is_ok());
    assert!(validator.accept("a", 6, 0).is_err());
    assert!(validator.accept("b", 6, 1).is_err());
}

#[test]
fn framing_and_schema_reject_semantic_shortcuts_unknown_fields_and_partial_lines() {
    let semantic_shortcut = b"{\"schema\":\"astra.user_input_sequence.v1\",\"session\":\"run.one\",\"sequence\":1,\"tick\":0,\"event\":{\"type\":\"choose\",\"id\":\"route-a\"}}\n";
    assert!(
        JsonlReader::new(BufReader::new(Cursor::new(semantic_shortcut)), 4096)
            .unwrap()
            .read::<InputMessage>()
            .is_err()
    );

    let unknown_field = b"{\"schema\":\"astra.user_input_sequence.v1\",\"session\":\"run.one\",\"sequence\":1,\"tick\":0,\"unexpected\":true,\"event\":{\"type\":\"resume\"}}\n";
    assert!(
        JsonlReader::new(BufReader::new(Cursor::new(unknown_field)), 4096)
            .unwrap()
            .read::<InputMessage>()
            .is_err()
    );

    let partial = b"{\"schema\":\"astra.user_input_sequence.v1\"}";
    let error = JsonlReader::new(BufReader::new(Cursor::new(partial)), 4096)
        .unwrap()
        .read::<InputMessage>()
        .unwrap_err();
    assert_eq!(error.operation, "jsonl.read");
}

#[test]
fn custom_tolerances_are_validated_without_external_approval() {
    let mut config = CheckpointConfig {
        schema: HEADLESS_CHECKPOINT_CONFIG_SCHEMA.into(),
        id: "tolerance.test".into(),
        input_sequence_hash: hash("input"),
        renderer_identity_hash: astra_headless_protocol::RendererExecutionIdentity::cpu_reference()
            .hash()
            .unwrap(),
        checkpoints: vec![CheckpointExpectation {
            id: "final".into(),
            required: true,
            observation_hash: None,
            image_baseline_path: None,
            image_baseline_hash: None,
            audio_baseline_path: None,
            audio_baseline_hash: None,
            image_tolerance: ImageTolerance {
                changed_pixel_ratio: 0.002,
                ..ImageTolerance::default()
            },
            audio_tolerance: AudioTolerance {
                max_duration_delta_ms: 10.0,
                ..AudioTolerance::default()
            },
        }],
    };
    config.validate().unwrap();

    config.checkpoints[0].image_tolerance.changed_pixel_ratio = f64::NAN;
    assert!(config.validate().is_err());
    config.checkpoints[0].image_tolerance.changed_pixel_ratio = 0.002;
    config.checkpoints[0].audio_tolerance.max_duration_delta_ms = -1.0;
    assert!(config.validate().is_err());
}
