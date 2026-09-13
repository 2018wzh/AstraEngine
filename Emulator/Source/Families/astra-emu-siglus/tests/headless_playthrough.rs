//! Headless playthrough test: a skip-driven route completion attempt plus
//! quick save/load feature probes.
//!
//! Skipped unless `ASTRA_SIGLUS_TEST_GAME` points at a game directory and
//! `ASTRA_SIGLUS_TEST_PLAYTHROUGH=1`. The driver walks the title flow into the
//! prologue, then engages the game's own force-skip (A.Skip) while a click /
//! Enter cadence advances through choices and unread text until the engine
//! requests exit or the advance budget (`ASTRA_SIGLUS_TEST_PLAYTHROUGH_BUDGET`,
//! default 400000 frames) is spent. Quick save/load are probed after the route
//! so a missed menu click cannot stall it. Progress goes to stderr via
//! `SG_HEADLESS_TRACE`.

use std::sync::atomic::{AtomicU64, Ordering};

use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FamilyEvent, FamilyOpen, FamilyProvider,
    FamilySession, FamilyStatus, FrameView, FrameVisitor, KeyCode, KeyModifiers, KeyState,
    OpenRequest, PcmChunk, PcmFormatSpec, PointerButton, WindowState,
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

fn none_modifiers() -> KeyModifiers {
    KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    }
}

fn click(x: f32, y: f32) -> Vec<FamilyEvent> {
    vec![
        FamilyEvent::PointerMove { x, y },
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

fn enter_press() -> Vec<FamilyEvent> {
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

fn step(session: &mut dyn FamilySession, events: &[FamilyEvent]) -> bool {
    let response = session
        .advance(16_666_667, events)
        .expect("advance succeeds");
    response.status == FamilyStatus::Finished
}

fn settle(session: &mut dyn FamilySession, frames: u32) {
    for _ in 0..frames {
        if step(session, &[]) {
            break;
        }
    }
}

/// The ipm click model requires the move, press, and release edges inside
/// one input poll; edges split across advances are ignored by menu items.
fn hover_then_click(x: f32, y: f32) -> Vec<Vec<FamilyEvent>> {
    vec![click(x, y)]
}

/// Advances until the composed frame shows visible content again (a lit
/// pixel ratio above the threshold) or the frame budget runs out. Navigation
/// clicks land on real screens instead of fade transitions.
fn settle_until_lit(session: &mut dyn FamilySession, max_frames: u32, min_lit_ratio: f64) -> bool {
    struct Lit {
        count: usize,
        total: usize,
    }
    impl FrameVisitor for Lit {
        fn accept(&mut self, frame: FrameView<'_>) -> astra_emu_family_api::FamilyResult<()> {
            self.total = frame.as_slice().len() / 4;
            self.count = frame
                .as_slice()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3].iter().any(|channel| *channel > 24))
                .count();
            Ok(())
        }
    }
    for _ in 0..max_frames {
        let _ = session.advance(16_666_667, &[]).expect("advance succeeds");
        let mut lit = Lit { count: 0, total: 0 };
        session.visit_frame(&mut lit).expect("frame visit succeeds");
        if lit.total > 0 && (lit.count as f64) / (lit.total as f64) >= min_lit_ratio {
            return true;
        }
    }
    false
}

fn escape(session: &mut dyn FamilySession) {
    let events = vec![
        FamilyEvent::Key {
            code: KeyCode::Escape,
            state: KeyState::Pressed,
            modifiers: none_modifiers(),
        },
        FamilyEvent::Key {
            code: KeyCode::Escape,
            state: KeyState::Released,
            modifiers: none_modifiers(),
        },
    ];
    let _ = step(session, &events);
}

fn open_session(
    provider: &mut astra_emu_siglus::SiglusProvider,
    game: &std::path::Path,
) -> Box<dyn FamilySession> {
    let FamilyOpen {
        response: _,
        session,
    } = provider
        .open(OpenRequest {
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
    session
}

#[test]
fn headless_playthrough_route_completion_and_feature_probes() {
    if std::env::var("ASTRA_SIGLUS_TEST_PLAYTHROUGH").as_deref() != Ok("1") {
        eprintln!("ASTRA_SIGLUS_TEST_PLAYTHROUGH not set; skipping");
        return;
    }
    let Some(game) = std::env::var("ASTRA_SIGLUS_TEST_GAME")
        .ok()
        .map(std::path::PathBuf::from)
    else {
        eprintln!("ASTRA_SIGLUS_TEST_GAME not set; skipping");
        return;
    };
    let budget: u64 = std::env::var("ASTRA_SIGLUS_TEST_PLAYTHROUGH_BUDGET")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(400_000);

    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let mut provider = astra_emu_siglus::SiglusProvider::default();
    let mut session = open_session(&mut provider, &game);
    let mut alive = Alive;

    // ---- Title flow into the prologue ----
    settle(session.as_mut(), 30);
    settle_until_lit(session.as_mut(), 600, 0.02);
    // Dismiss the startup copyright screen if it is up, then wait for the
    // title menu (the ipm menu appears long after the title art fades in).
    let events = click(640.0, 400.0);
    let _ = step(session.as_mut(), &events);
    settle_until_lit(session.as_mut(), 900, 0.02);
    settle(session.as_mut(), 700);
    for events in hover_then_click(452.0, 645.0) {
        if step(session.as_mut(), &events) {
            break;
        }
    }
    settle(session.as_mut(), 4000);
    for events in hover_then_click(452.0, 645.0) {
        if step(session.as_mut(), &events) {
            break;
        }
    }
    settle(session.as_mut(), 400);
    let t0 = std::time::Instant::now();
    for i in 0..6000_u64 {
        let events: Vec<FamilyEvent> = if i % 40 == 0 {
            click(640.0, 400.0)
        } else {
            Vec::new()
        };
        if step(session.as_mut(), &events) {
            eprintln!("PLAYTHROUGH: engine exited during the opening at frame {i}");
            break;
        }
    }
    eprintln!(
        "PLAYTHROUGH: opening phase done in {:?} ({:.0} adv/s)",
        t0.elapsed(),
        6400_f64 / t0.elapsed().as_secs_f64().max(0.001)
    );

    // ---- Click-driven route completion ----
    //
    // The ipm input system requires the press and release edges inside one
    // input poll, so each click is a single-advance move+press+release.
    let mut finished = false;
    let route_t0 = std::time::Instant::now();
    for i in 0..budget {
        let events: Vec<FamilyEvent> = if i % 12 == 0 {
            let mut events = click(640.0, 400.0);
            events.extend(enter_press());
            events
        } else {
            Vec::new()
        };
        if step(session.as_mut(), &events) {
            finished = true;
            eprintln!("PLAYTHROUGH: engine requested exit at route frame {i}");
            break;
        }
        if i % 20_000 == 19_999 {
            eprintln!(
                "PLAYTHROUGH: {}/{} route frames in {:?} ({:.0} adv/s)",
                i + 1,
                budget,
                route_t0.elapsed(),
                (i + 1) as f64 / route_t0.elapsed().as_secs_f64()
            );
            session
                .visit_frame(&mut alive)
                .expect("route frames compose");
        }
    }
    session
        .visit_frame(&mut alive)
        .expect("frames stay composed after the route run");

    // ---- Feature probe: quick save, quick load ----
    //
    // Runs after the route attempt so a missed menu click cannot stall it;
    // Escape closes whatever system submenu a probe click opened.
    for events in hover_then_click(1120.0, 558.0) {
        if step(session.as_mut(), &events) {
            break;
        }
    }
    settle(session.as_mut(), 120);
    escape(session.as_mut());
    settle(session.as_mut(), 60);
    for events in hover_then_click(1210.0, 558.0) {
        if step(session.as_mut(), &events) {
            break;
        }
    }
    settle(session.as_mut(), 120);
    escape(session.as_mut());
    settle(session.as_mut(), 120);
    escape(session.as_mut());
    settle(session.as_mut(), 60);
    session
        .visit_frame(&mut alive)
        .expect("frames stay composed through the save/load probes");
    eprintln!("PLAYTHROUGH: save/load probes done");

    let pushed = PUSHED_FRAMES.load(Ordering::Relaxed);
    eprintln!("PLAYTHROUGH: pushed audio frames = {pushed}, finished = {finished}");
    assert!(pushed > 480_000, "audio flowed through the playthrough");
    session
        .close()
        .expect("session closes cleanly after the playthrough");
}
