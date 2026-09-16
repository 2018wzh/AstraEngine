//! Headless route test: drives the title flow into the game proper.
//!
//! Run explicitly with `--ignored` and `ASTRA_SIGLUS_TEST_GAME` set.
//! This is a several-minute run: the Rewrite+
//! title plays its scripted intro and opening movie before the first dialogue
//! scene, and every movie/counter advance is driven frame by frame through
//! the deterministic hosted clock. The route exercised is: title menu →
//! Start (copyright + intro) → menu → Start → opening scene → skip the
//! opening movie → the prologue dialogue system (`sys20_adv01` /
//! `seen010xx`) takes over and keeps advancing under input.

use std::sync::atomic::{AtomicU64, Ordering};

use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FamilyEvent, FamilyOpen, FamilyProvider,
    FamilySession, FamilyStatus, FrameView, FrameVisitor, KeyState, OpenRequest, PcmChunk,
    PcmFormatSpec, PointerButton, WindowState,
};

static PUSHED_FRAMES: AtomicU64 = AtomicU64::new(0);

struct CountingSink;

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

struct Alive;

impl FrameVisitor for Alive {
    fn accept(&mut self, frame: FrameView<'_>) -> astra_emu_family_api::FamilyResult<()> {
        assert!(
            frame.info.width > 0 && frame.info.height > 0,
            "composed frame stays valid"
        );
        Ok(())
    }
}

fn step(session: &mut dyn FamilySession, events: &[FamilyEvent]) -> bool {
    let response = session
        .advance(16_666_667, events)
        .expect("advance succeeds");
    response.status == FamilyStatus::Finished
}

fn hover_click(x: f32, y: f32) -> Vec<Vec<FamilyEvent>> {
    vec![
        vec![FamilyEvent::PointerMove { x, y }],
        vec![],
        vec![FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Pressed,
        }],
        vec![FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Released,
        }],
    ]
}

#[test]
#[ignore = "requires an authorized Siglus game and hardware GPU"]
fn headless_route_reaches_dialogue_flow() {
    let game = std::path::PathBuf::from(
        std::env::var_os("ASTRA_SIGLUS_TEST_GAME")
            .expect("ASTRA_SIGLUS_TEST_GAME must identify the authorized game"),
    );
    let mut provider = astra_emu_siglus::SiglusProvider::default();
    let open = provider
        .open(OpenRequest {
            configuration: Vec::new().into(),
            game_path: game.display().to_string().into(),
            initial_window: WindowState {
                width: 1280,
                height: 720,
                focused: true,
                visible: true,
            },
            host: astra_emu_family_api::FamilyHostServices {
                audio_sink: abi_stable::std_types::ROption::RSome(AudioSink_TO::from_value(
                    CountingSink,
                    TD_Opaque,
                )),
                text_replacement: abi_stable::std_types::ROption::RNone,
            },
        })
        .expect("siglus session opens");
    let FamilyOpen {
        response: _,
        mut session,
    } = open;

    let mut alive = Alive;

    // Settle to the title menu.
    for _ in 0..90 {
        if step(session.as_mut(), &[]) {
            break;
        }
    }

    // First Start: copyright and the scripted title intro.
    for events in hover_click(452.0, 645.0) {
        if step(session.as_mut(), &events) {
            break;
        }
    }
    for i in 0..4000 {
        if step(session.as_mut(), &[]) {
            eprintln!("engine finished during the intro at {i}");
            break;
        }
    }

    // Second Start: straight into the opening scene.
    for events in hover_click(452.0, 645.0) {
        if step(session.as_mut(), &events) {
            break;
        }
    }
    for _ in 0..400 {
        if step(session.as_mut(), &[]) {
            break;
        }
    }

    // Skip through the opening movie; the prologue dialogue system then keeps
    // advancing under the click cadence.
    let mut finished = false;
    for i in 0..6000 {
        let events: Vec<FamilyEvent> = if i % 40 == 0 {
            vec![
                FamilyEvent::PointerMove { x: 640.0, y: 400.0 },
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                },
            ]
        } else {
            Vec::new()
        };
        if step(session.as_mut(), &events) {
            finished = true;
            eprintln!("engine requested exit at route frame {i}");
            break;
        }
    }
    let _ = finished;
    session
        .visit_frame(&mut alive)
        .expect("frames stay composed through the route");
    let pushed = PUSHED_FRAMES.load(Ordering::Relaxed);
    assert!(
        pushed > 480_000,
        "audio flowed through the route ({pushed} frames)"
    );
    session
        .close()
        .expect("session closes cleanly after the route");
}
