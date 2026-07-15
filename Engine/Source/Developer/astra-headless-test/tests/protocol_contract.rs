use astra_headless_protocol::{
    ButtonState, InputMessage, PhysicalInput, USER_INPUT_SEQUENCE_SCHEMA,
};

#[astra_headless_test::test]
fn rejects_semantic_or_unknown_input_shape() {
    let json = r#"{"schema":"astra.user_input_sequence.v1","session":"s","sequence":1,"tick":0,"event":{"type":"choose","id":"x"}}"#;
    assert!(serde_json::from_str::<InputMessage>(json).is_err());
}

#[astra_headless_test::test]
fn accepts_physical_keyboard_input() {
    let input = InputMessage {
        schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
        session: "s".into(),
        sequence: 1,
        tick: 0,
        event: PhysicalInput::Keyboard {
            physical_key: "Enter".into(),
            logical_key: Some("Enter".into()),
            state: ButtonState::Pressed,
            repeat: false,
        },
    };
    input.validate().unwrap();
}
