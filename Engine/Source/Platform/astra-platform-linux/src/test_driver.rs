use astra_platform::{PlatformError, PlatformErrorCode};
use evdev::{
    uinput::VirtualDevice, AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode,
};

pub struct LinuxTestDriver {
    keyboard: VirtualDevice,
    mouse: VirtualDevice,
}

impl LinuxTestDriver {
    pub fn open() -> Result<Self, PlatformError> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in [
            KeyCode::KEY_ENTER,
            KeyCode::KEY_ESC,
            KeyCode::KEY_SPACE,
            KeyCode::KEY_UP,
            KeyCode::KEY_DOWN,
            KeyCode::KEY_LEFT,
            KeyCode::KEY_RIGHT,
        ] {
            keys.insert(key);
        }
        let keyboard = VirtualDevice::builder()
            .and_then(|builder| builder.name("Astra Linux Test Keyboard").with_keys(&keys))
            .and_then(|builder| builder.build())
            .map_err(|_| {
                driver_error("test_driver.uinput.open", "uinput keyboard is unavailable")
            })?;

        let axes = AttributeSet::from_iter([
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
        ]);
        let mut buttons = AttributeSet::<KeyCode>::new();
        buttons.insert(KeyCode::BTN_LEFT);
        let mouse = VirtualDevice::builder()
            .and_then(|builder| builder.name("Astra Linux Test Mouse").with_keys(&buttons))
            .and_then(|builder| builder.with_relative_axes(&axes))
            .and_then(|builder| builder.build())
            .map_err(|_| driver_error("test_driver.uinput.open", "uinput mouse is unavailable"))?;
        Ok(Self { keyboard, mouse })
    }

    pub fn send_key(&mut self, key: KeyCode) -> Result<(), PlatformError> {
        self.keyboard
            .emit(&[
                InputEvent::new(EventType::KEY.0, key.code(), 1),
                InputEvent::new(EventType::KEY.0, key.code(), 0),
            ])
            .map_err(|_| driver_error("test_driver.uinput.key", "uinput key emission failed"))
    }

    pub fn move_mouse(&mut self, delta_x: i32, delta_y: i32) -> Result<(), PlatformError> {
        self.mouse
            .emit(&[
                InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, delta_x),
                InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, delta_y),
            ])
            .map_err(|_| {
                driver_error(
                    "test_driver.uinput.pointer",
                    "uinput pointer emission failed",
                )
            })
    }

    pub fn click_primary(&mut self) -> Result<(), PlatformError> {
        self.mouse
            .emit(&[
                InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.code(), 1),
                InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.code(), 0),
            ])
            .map_err(|_| {
                driver_error(
                    "test_driver.uinput.pointer",
                    "uinput button emission failed",
                )
            })
    }
}

fn driver_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}
