use astra_platform::{
    InputState, PlatformEvent, PlatformEventKind, PointerButton, TouchPhase, WindowHandle,
};

#[test]
fn platform_events_are_typed_and_do_not_carry_native_handles() {
    let window = WindowHandle::from_parts(1, 1).unwrap();
    let keyboard = PlatformEvent::new(
        1,
        PlatformEventKind::Keyboard {
            window,
            physical_key: "Space".to_string(),
            logical_key: Some(" ".to_string()),
            state: InputState::Pressed,
            repeat: false,
        },
    );
    let ime = PlatformEvent::new(
        2,
        PlatformEventKind::ImeCommit {
            window,
            text: "中文".to_string(),
        },
    );
    let touch = PlatformEvent::new(
        3,
        PlatformEventKind::Touch {
            window,
            id: 7,
            x: 12.5,
            y: 42.0,
            phase: TouchPhase::Moved,
        },
    );
    let pointer = PlatformEvent::new(
        4,
        PlatformEventKind::PointerButton {
            window,
            button: PointerButton::Primary,
            state: InputState::Released,
        },
    );
    let resize = PlatformEvent::new(
        5,
        PlatformEventKind::WindowResized {
            window,
            width: 1280,
            height: 720,
            scale_factor: 1.5,
        },
    );
    let gamepad = PlatformEvent::new(
        6,
        PlatformEventKind::Gamepad {
            device_id: 0,
            control: "button_a".to_string(),
            value: 1.0,
        },
    );

    assert_eq!(keyboard.sequence, 1);
    assert!(matches!(ime.kind, PlatformEventKind::ImeCommit { .. }));
    assert!(matches!(touch.kind, PlatformEventKind::Touch { id: 7, .. }));
    assert!(matches!(
        pointer.kind,
        PlatformEventKind::PointerButton {
            button: PointerButton::Primary,
            ..
        }
    ));
    assert!(matches!(
        resize.kind,
        PlatformEventKind::WindowResized { width: 1280, .. }
    ));
    assert!(matches!(
        gamepad.kind,
        PlatformEventKind::Gamepad { device_id: 0, .. }
    ));
}
