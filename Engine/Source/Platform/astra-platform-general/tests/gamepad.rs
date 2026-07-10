use astra_platform::{GamepadControl, PlatformEventKind};
use astra_platform_general::{GamepadMapper, RawGamepadEvent};

#[test]
fn mapper_assigns_session_ids_and_normalizes_axes() {
    let mut mapper = GamepadMapper::new(0.2).expect("valid deadzone");
    assert_eq!(
        mapper.apply(RawGamepadEvent::Connected { raw_device_id: 41 }),
        vec![PlatformEventKind::GamepadConnected { device_id: 0 }]
    );
    assert_eq!(
        mapper.apply(RawGamepadEvent::Axis {
            raw_device_id: 41,
            control: GamepadControl::LeftStickX,
            value: 0.1,
        }),
        Vec::new()
    );
    let events = mapper.apply(RawGamepadEvent::Axis {
        raw_device_id: 41,
        control: GamepadControl::LeftStickX,
        value: 0.6,
    });
    assert!(matches!(
        events.as_slice(),
        [PlatformEventKind::GamepadInput {
            device_id: 0,
            control: GamepadControl::LeftStickX,
            value,
        }] if (*value - 0.5).abs() < f32::EPSILON
    ));
    assert_eq!(
        mapper.apply(RawGamepadEvent::Disconnected { raw_device_id: 41 }),
        vec![PlatformEventKind::GamepadDisconnected { device_id: 0 }]
    );
}

#[test]
fn mapper_rejects_invalid_axis_values() {
    let mut mapper = GamepadMapper::new(0.2).expect("valid deadzone");
    mapper.apply(RawGamepadEvent::Connected { raw_device_id: 7 });
    assert!(mapper
        .apply_checked(RawGamepadEvent::Axis {
            raw_device_id: 7,
            control: GamepadControl::LeftTrigger,
            value: f32::NAN,
        })
        .is_err());
}
