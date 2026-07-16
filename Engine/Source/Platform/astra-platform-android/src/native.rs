use android_activity::AndroidApp;
use astra_platform::{HostLaunchProfile, PlatformError, PlatformErrorCode, PlatformHostSession};

pub async fn start_registered_activity(
    _profile: HostLaunchProfile,
) -> Result<PlatformHostSession, PlatformError> {
    Err(PlatformError::new(
        PlatformErrorCode::InvalidState,
        "host.start",
        "Android factory must be driven by run_player_host on the GameActivity thread",
    ))
}

pub fn run_player_host<F>(
    app: AndroidApp,
    profile: HostLaunchProfile,
    player: F,
) -> Result<(), PlatformError>
where
    F: FnOnce(PlatformHostSession) -> Result<(), PlatformError> + Send + 'static,
{
    tracing::info!(
        event = "platform.android.host.starting",
        provider = "android.game_activity",
        "Android host is entering the GameActivity event loop"
    );
    crate::native::host::run(app, profile, player)
}

#[derive(Debug, Clone, Default)]
struct JniBridgeState {
    insets: [i32; 4],
    insets_dirty: bool,
    audio_focus: i32,
    audio_focus_dirty: bool,
    saf_import: Option<SafImport>,
    recreation_count: u64,
    new_intent_count: u64,
    gamepad_events: std::collections::VecDeque<BridgeGamepadEvent>,
    gamepad_overflow_count: u64,
}

#[derive(Debug, Clone, Copy)]
enum BridgeGamepadEvent {
    Connected {
        device_id: u32,
        connected: bool,
    },
    Input {
        device_id: u32,
        control: u8,
        value: f32,
    },
}

#[derive(Debug, Clone)]
pub(super) struct SafImport {
    pub token: String,
    pub sha256: String,
    pub size: u64,
    pub permission_persisted: bool,
}

fn bridge_state() -> &'static std::sync::Mutex<JniBridgeState> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<JniBridgeState>> =
        std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(JniBridgeState::default()))
}

fn with_bridge_state(update: impl FnOnce(&mut JniBridgeState)) {
    let mut state = bridge_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    update(&mut state);
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnInsets(
    _environment: *mut jni::sys::JNIEnv,
    _activity: jni::sys::jobject,
    left: jni::sys::jint,
    top: jni::sys::jint,
    right: jni::sys::jint,
    bottom: jni::sys::jint,
) {
    with_bridge_state(|state| {
        state.insets = [left, top, right, bottom];
        state.insets_dirty = true;
    });
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnAudioFocus(
    _environment: *mut jni::sys::JNIEnv,
    _activity: jni::sys::jobject,
    change: jni::sys::jint,
) {
    with_bridge_state(|state| {
        state.audio_focus = change;
        state.audio_focus_dirty = true;
    });
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnGamepadDevice(
    _environment: *mut jni::sys::JNIEnv,
    _activity: jni::sys::jobject,
    device_id: jni::sys::jint,
    connected: jni::sys::jboolean,
) {
    let Ok(device_id) = u32::try_from(device_id) else {
        return;
    };
    with_bridge_state(|state| {
        if state.gamepad_events.len() < 1_024 {
            state
                .gamepad_events
                .push_back(BridgeGamepadEvent::Connected {
                    device_id,
                    connected,
                });
        } else {
            state.gamepad_overflow_count = state.gamepad_overflow_count.saturating_add(1);
        }
    });
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnGamepadInput(
    _environment: *mut jni::sys::JNIEnv,
    _activity: jni::sys::jobject,
    device_id: jni::sys::jint,
    control: jni::sys::jint,
    value: jni::sys::jfloat,
) {
    let (Ok(device_id), Ok(control)) = (u32::try_from(device_id), u8::try_from(control)) else {
        return;
    };
    if !value.is_finite() {
        return;
    }
    with_bridge_state(|state| {
        if state.gamepad_events.len() < 1_024 {
            state.gamepad_events.push_back(BridgeGamepadEvent::Input {
                device_id,
                control,
                value: value.clamp(-1.0, 1.0),
            });
        } else {
            state.gamepad_overflow_count = state.gamepad_overflow_count.saturating_add(1);
        }
    });
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnSafResult<'caller>(
    mut environment: jni::EnvUnowned<'caller>,
    _activity: jni::objects::JObject<'caller>,
    token: jni::objects::JString<'caller>,
    sha256: jni::objects::JString<'caller>,
    size: jni::sys::jlong,
    persisted: jni::sys::jboolean,
) {
    let strings = environment
        .with_env(|environment| -> jni::errors::Result<_> {
            let token = (!token.is_null())
                .then(|| token.try_to_string(environment).ok())
                .flatten();
            let sha256 = (!sha256.is_null())
                .then(|| sha256.try_to_string(environment).ok())
                .flatten();
            Ok((token, sha256))
        })
        .into_outcome();
    let (token, sha256) = match strings {
        jni::Outcome::Ok(strings) => strings,
        jni::Outcome::Err(_) | jni::Outcome::Panic(_) => (None, None),
    };
    with_bridge_state(|state| {
        state.saf_import = match (token, sha256, u64::try_from(size).ok()) {
            (Some(token), Some(sha256), Some(size)) if size > 0 => Some(SafImport {
                token,
                sha256,
                size,
                permission_persisted: persisted,
            }),
            _ => None,
        };
    });
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnRecreated(
    _environment: *mut jni::sys::JNIEnv,
    _activity: jni::sys::jobject,
    recreated: jni::sys::jboolean,
) {
    if recreated {
        with_bridge_state(|state| state.recreation_count += 1);
    }
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "system" fn Java_com_astra_player_AstraGameActivity_nativeOnNewIntent(
    _environment: *mut jni::sys::JNIEnv,
    _activity: jni::sys::jobject,
) {
    with_bridge_state(|state| state.new_intent_count += 1);
}

pub(super) fn drain_bridge_events(
    window: Option<astra_platform::WindowHandle>,
) -> Result<Vec<astra_platform::PlatformEventKind>, PlatformError> {
    use astra_platform::{AudioFocusState, GamepadControl, PlatformEventKind, WindowInsets};
    let mut state = bridge_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.gamepad_overflow_count != 0 {
        let dropped_count = std::mem::take(&mut state.gamepad_overflow_count);
        tracing::error!(
            event = "platform.android.input.queue_overflow",
            diagnostic_code = "ASTRA_ANDROID_INPUT_QUEUE_OVERFLOW",
            dropped_count,
            "Android native input queue overflowed"
        );
        state.gamepad_events.clear();
        return Err(PlatformError::new(
            PlatformErrorCode::QueueOverflow,
            "input.android.drain",
            "Android native input queue overflowed",
        ));
    }
    let mut events = Vec::with_capacity(2);
    if state.insets_dirty {
        state.insets_dirty = false;
        if let Some(window) = window {
            let [left, top, right, bottom] = state.insets;
            events.push(PlatformEventKind::WindowInsetsChanged {
                window,
                insets: WindowInsets {
                    left: left.max(0) as u32,
                    top: top.max(0) as u32,
                    right: right.max(0) as u32,
                    bottom: bottom.max(0) as u32,
                },
            });
        }
    }
    if state.audio_focus_dirty {
        state.audio_focus_dirty = false;
        let focus = match state.audio_focus {
            1 => Some(AudioFocusState::Gained),
            -1 => Some(AudioFocusState::Lost),
            -2 => Some(AudioFocusState::LostTransient),
            -3 => Some(AudioFocusState::Duck),
            _ => None,
        };
        if let Some(focus) = focus {
            events.push(PlatformEventKind::AudioFocusChanged { state: focus });
        }
    }
    while let Some(event) = state.gamepad_events.pop_front() {
        match event {
            BridgeGamepadEvent::Connected {
                device_id,
                connected: true,
            } => events.push(PlatformEventKind::GamepadConnected { device_id }),
            BridgeGamepadEvent::Connected {
                device_id,
                connected: false,
            } => events.push(PlatformEventKind::GamepadDisconnected { device_id }),
            BridgeGamepadEvent::Input {
                device_id,
                control,
                value,
            } => {
                let control = match control {
                    0 => GamepadControl::South,
                    1 => GamepadControl::East,
                    2 => GamepadControl::West,
                    3 => GamepadControl::North,
                    4 => GamepadControl::DpadUp,
                    5 => GamepadControl::DpadDown,
                    6 => GamepadControl::DpadLeft,
                    7 => GamepadControl::DpadRight,
                    8 => GamepadControl::LeftShoulder,
                    9 => GamepadControl::RightShoulder,
                    10 => GamepadControl::LeftTrigger,
                    11 => GamepadControl::RightTrigger,
                    12 => GamepadControl::LeftStickX,
                    13 => GamepadControl::LeftStickY,
                    14 => GamepadControl::RightStickX,
                    15 => GamepadControl::RightStickY,
                    16 => GamepadControl::LeftStickButton,
                    17 => GamepadControl::RightStickButton,
                    18 => GamepadControl::Start,
                    19 => GamepadControl::Select,
                    _ => continue,
                };
                events.push(PlatformEventKind::GamepadInput {
                    device_id,
                    control,
                    value,
                });
            }
        }
    }
    Ok(events)
}

pub(super) fn take_saf_import() -> Option<SafImport> {
    bridge_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .saf_import
        .take()
}

#[path = "native_host.rs"]
mod host;
