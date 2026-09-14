//! Shared helpers for the Artemis family headless tests.

#![allow(dead_code)] // each test binary uses a subset

use std::sync::atomic::{AtomicU64, Ordering};

use abi_stable::std_types::ROption;
use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FamilyEvent, FamilyOpen, FamilyProvider,
    FamilySession, KeyCode, KeyModifiers, KeyState, OpenRequest, PcmChunk, PcmFormatSpec,
    PointerButton, WindowState,
};

static PUSHED_FRAMES: AtomicU64 = AtomicU64::new(0);

pub(crate) struct CountingSink;

impl AudioSink for CountingSink {
    fn configure(&self, _format: PcmFormatSpec) -> astra_emu_family_api::FfiFamilyResult<()> {
        Ok(()).into()
    }
    fn write(&self, chunk: PcmChunk) -> astra_emu_family_api::FfiFamilyResult<AudioWriteStatus> {
        PUSHED_FRAMES.fetch_add(
            u64::try_from(chunk.sample_count() / 2).unwrap_or(0),
            Ordering::Relaxed,
        );
        Ok(AudioWriteStatus::Accepted).into()
    }
    fn is_cancelled(&self) -> bool {
        false
    }
    fn cancel(&self) -> astra_emu_family_api::FfiFamilyResult<()> {
        Ok(()).into()
    }
}

pub(crate) fn pushed_frames() -> u64 {
    PUSHED_FRAMES.load(Ordering::Relaxed)
}

pub(crate) fn none_modifiers() -> KeyModifiers {
    KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    }
}

/// Hover phase of a click: the Artemis UI binds buttons through queued Lua
/// hover handlers, so the pointer move must be its own engine tick before
/// the press edge, or the button cursor is not set yet.
pub(crate) fn hover(x: f32, y: f32) -> Vec<FamilyEvent> {
    vec![FamilyEvent::PointerMove { x, y }]
}

pub(crate) fn click() -> Vec<FamilyEvent> {
    vec![
        FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Pressed,
        },
        FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Released,
        },
    ]
}

pub(crate) fn enter_press() -> Vec<FamilyEvent> {
    vec![
        FamilyEvent::Key {
            code: KeyCode::Enter,
            state: KeyState::Pressed,
            modifiers: none_modifiers(),
        },
        FamilyEvent::Key {
            code: KeyCode::Enter,
            state: KeyState::Released,
            modifiers: none_modifiers(),
        },
    ]
}

/// Opens a session and returns it with the ABI-reported stage size, so
/// drivers can aim clicks at real stage coordinates.
pub(crate) fn open_session(game: &std::path::Path) -> (Box<dyn FamilySession>, (u32, u32)) {
    let mut provider = astra_emu_artemis::ArtemisProvider::default();
    let FamilyOpen { response, session } = provider
        .open(OpenRequest {
            game_path: game.display().to_string().into(),
            initial_window: WindowState {
                width: 1280,
                height: 720,
                focused: true,
                visible: true,
            },
            host: astra_emu_family_api::FamilyHostServices {
                audio_sink: ROption::RSome(AudioSink_TO::from_value(CountingSink, TD_Opaque)),
                text_replacement: ROption::RNone,
            },
        })
        .expect("artemis session opens");
    (session, (response.frame.width, response.frame.height))
}
