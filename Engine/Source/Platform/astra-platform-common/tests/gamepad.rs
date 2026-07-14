use astra_platform::{GamepadControl, PlatformEventKind};
use astra_platform_common::{GamepadMapper, RawGamepadEvent};

#[test]
fn mapper_assigns_session_ids_and_normalizes_axes() {
    let mut mapper = GamepadMapper::new(0.2).expect("valid deadzone");
    assert_eq!(
        mapper
            .apply_checked(RawGamepadEvent::Connected { raw_device_id: 41 })
            .unwrap(),
        vec![PlatformEventKind::GamepadConnected { device_id: 0 }]
    );
    assert_eq!(
        mapper
            .apply_checked(RawGamepadEvent::Axis {
                raw_device_id: 41,
                control: GamepadControl::LeftStickX,
                value: 0.1,
            })
            .unwrap(),
        Vec::new()
    );
    let events = mapper
        .apply_checked(RawGamepadEvent::Axis {
            raw_device_id: 41,
            control: GamepadControl::LeftStickX,
            value: 0.6,
        })
        .unwrap();
    assert!(matches!(
        events.as_slice(),
        [PlatformEventKind::GamepadInput {
            device_id: 0,
            control: GamepadControl::LeftStickX,
            value,
        }] if (*value - 0.5).abs() < f32::EPSILON
    ));
    assert_eq!(
        mapper
            .apply_checked(RawGamepadEvent::Disconnected { raw_device_id: 41 })
            .unwrap(),
        vec![PlatformEventKind::GamepadDisconnected { device_id: 0 }]
    );
}

#[test]
fn mapper_rejects_invalid_axis_values() {
    let mut mapper = GamepadMapper::new(0.2).expect("valid deadzone");
    mapper
        .apply_checked(RawGamepadEvent::Connected { raw_device_id: 7 })
        .unwrap();
    assert!(mapper
        .apply_checked(RawGamepadEvent::Axis {
            raw_device_id: 7,
            control: GamepadControl::LeftTrigger,
            value: f32::NAN,
        })
        .is_err());
}
