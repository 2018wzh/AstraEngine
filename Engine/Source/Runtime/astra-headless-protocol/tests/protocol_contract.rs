use std::io::{BufReader, Cursor};

use astra_headless_protocol::{
    AudioTolerance, ButtonState, CheckpointConfig, CheckpointExpectation, ImageTolerance,
    InputMessage, JsonlReader, JsonlWriter, PhysicalInput, PreflightLink, ReviewRecord,
    ReviewVerdict, ReviewerKind, SequenceValidator, ToleranceApproval, ToleranceApprovalBinding,
    ToleranceApproverKind, HEADLESS_CHECKPOINT_CONFIG_SCHEMA, HEADLESS_PREFLIGHT_LINK_SCHEMA,
    HEADLESS_REVIEW_SCHEMA, HEADLESS_TOLERANCE_APPROVAL_SCHEMA, USER_INPUT_SEQUENCE_SCHEMA,
};

fn hash(label: &str) -> String {
    format!("sha256:{:064x}", label.len())
}

#[astra_headless_test::test]
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

#[astra_headless_test::test]
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

#[astra_headless_test::test]
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

#[astra_headless_test::test]
fn review_and_preflight_reject_incomplete_or_unsafe_evidence() {
    let review = ReviewRecord {
        schema: HEADLESS_REVIEW_SCHEMA.into(),
        run_report_hash: hash("run"),
        reviewer_kind: ReviewerKind::Model,
        reviewer_identity: "model-v1".into(),
        tool_identity_hash: hash("tool"),
        checkpoints: vec![ReviewVerdict {
            checkpoint: "required.final".into(),
            passed: true,
            diagnostic_codes: Vec::new(),
        }],
    };
    review.validate().unwrap();
    let mut invalid = review.clone();
    invalid.checkpoints.push(invalid.checkpoints[0].clone());
    assert!(invalid.validate().is_err());

    let mut link = PreflightLink {
        schema: HEADLESS_PREFLIGHT_LINK_SCHEMA.into(),
        headless_run_report_hash: hash("headless"),
        platform_run_report_hash: hash("platform"),
        build_fingerprint: hash("build"),
        cooked_package_hash: hash("package"),
        input_sequence_hash: hash("input"),
        scenario: "full.route".into(),
        target: "nativevn-game".into(),
        content_identity: "content.v1".into(),
        headless_profile_id: "headless.profile".into(),
        headless_session_id: "headless.session".into(),
        platform_profile_id: "windows.profile".into(),
        platform_session_id: "windows.session".into(),
    };
    link.validate().unwrap();
    link.input_sequence_hash = "not-a-hash".into();
    assert!(link.validate().is_err());
}

#[astra_headless_test::test]
fn customized_tolerance_requires_hash_bound_human_approval() {
    let mut config = CheckpointConfig {
        schema: HEADLESS_CHECKPOINT_CONFIG_SCHEMA.into(),
        id: "approval.test".into(),
        input_sequence_hash: hash("input"),
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
            audio_tolerance: AudioTolerance::default(),
        }],
        tolerance_approval: None,
    };
    assert!(config.validate().is_err());
    config.tolerance_approval = Some(ToleranceApprovalBinding {
        relative_path: "approval.json".into(),
        sha256: hash("approval"),
    });
    config.validate().unwrap();
    let approval = ToleranceApproval {
        schema: HEADLESS_TOLERANCE_APPROVAL_SCHEMA.into(),
        approval_id: "approval.001".into(),
        approver_kind: ToleranceApproverKind::Human,
        approver_identity: "qa.owner".into(),
        approved_tolerance_hash: config.tolerance_hash().unwrap(),
        previous_config_hash: Some(hash("previous")),
        reason_codes: vec!["renderer.change.reviewed".into()],
    };
    approval.validate().unwrap();
}
