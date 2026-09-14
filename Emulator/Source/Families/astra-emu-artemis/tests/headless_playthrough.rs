//! Headless playthrough test: a click-driven route completion attempt from
//! the title menu into the prologue.
//!
//! Skipped unless `ASTRA_ARTEMIS_TEST_PLAYTHROUGH=1` and
//! `ASTRA_ARTEMIS_TEST_GAME` point at an Artemis game directory. The driver
//! clicks through the title flow (an opening movie wait is resolved by the
//! adapter's no-FFmpeg immediate completion), then keeps a click/Enter
//! cadence advancing dialogue until the engine requests exit or the advance
//! budget (`ASTRA_ARTEMIS_TEST_PLAYTHROUGH_BUDGET`, default 200000 frames)
//! is spent. Escape closes whatever system page a stray click opened, and a
//! frame-composition probe runs after every phase.

mod common;

use std::time::Instant;

use astra_emu_family_api::{
    FamilyEvent, FamilyProvider, FamilySession, FamilyStatus, FrameView, FrameVisitor, KeyCode,
    KeyState, ProbeRequest,
};

use common::{click, enter_press, hover, none_modifiers};

/// Copies the current frame into a buffer for the private snapshot dump.
struct FrameCapture<'a> {
    out: &'a mut Vec<u8>,
    width: u32,
    height: u32,
}

impl FrameVisitor for FrameCapture<'_> {
    fn accept(&mut self, frame: FrameView<'_>) -> astra_emu_family_api::FamilyResult<()> {
        self.out.clear();
        self.out.extend_from_slice(frame.as_slice());
        debug_assert_eq!(frame.info.width, self.width);
        debug_assert_eq!(frame.info.height, self.height);
        Ok(())
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

#[derive(Default)]
struct LitTracker {
    count: usize,
    total: usize,
}

impl FrameVisitor for LitTracker {
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

/// Advances until the composed frame shows visible content again (a lit
/// pixel ratio above the threshold) or the frame budget runs out.
fn settle_until_lit(session: &mut dyn FamilySession, max_frames: u32, min_lit_ratio: f64) -> bool {
    let mut lit = LitTracker::default();
    for _ in 0..max_frames {
        let _ = session.advance(16_666_667, &[]).expect("advance succeeds");
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

#[test]
fn headless_playthrough_route_completion() {
    if std::env::var("ASTRA_ARTEMIS_TEST_PLAYTHROUGH").as_deref() != Ok("1") {
        eprintln!("ASTRA_ARTEMIS_TEST_PLAYTHROUGH not set; skipping");
        return;
    }
    let Some(game) = std::env::var("ASTRA_ARTEMIS_TEST_GAME")
        .ok()
        .map(std::path::PathBuf::from)
    else {
        eprintln!("ASTRA_ARTEMIS_TEST_GAME not set; skipping");
        return;
    };
    let budget: u64 = std::env::var("ASTRA_ARTEMIS_TEST_PLAYTHROUGH_BUDGET")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200_000);

    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let provider = astra_emu_artemis::ArtemisProvider::default();
    let report = provider
        .probe(ProbeRequest {
            game_path: game.display().to_string().into(),
        })
        .expect("probe succeeds")
        .expect("an Artemis game directory probes as artemis");
    eprintln!("PLAYTHROUGH: probed game_id={}", report.game_id);

    let (mut session, stage) = common::open_session(&game);
    let mut alive = Alive;
    eprintln!("PLAYTHROUGH: stage {}x{}", stage.0, stage.1);
    // Optional private snapshot directory for visual progress checks; never
    // written unless the environment provides it.
    let snapshot_dir = std::env::var("ASTRA_ARTEMIS_TEST_SNAPSHOT_DIR")
        .ok()
        .map(std::path::PathBuf::from);
    if let Some(dir) = &snapshot_dir {
        let _ = std::fs::create_dir_all(dir);
    }
    let snap = |session: &mut dyn FamilySession, name: &str| {
        let Some(dir) = &snapshot_dir else {
            return;
        };
        let mut copy = Vec::new();
        let mut copier = FrameCapture {
            out: &mut copy,
            width: stage.0,
            height: stage.1,
        };
        if session.visit_frame(&mut copier).is_ok() {
            let _ = image::save_buffer(
                dir.join(format!("route-{name}.png")),
                &copy,
                stage.0,
                stage.1,
                image::ColorType::Rgba8,
            );
        }
    };

    // ---- Title flow into the game ----
    settle_until_lit(session.as_mut(), 900, 0.02);
    settle(session.as_mut(), 60);
    // The title menu is script-drawn; a configurable click (default: stage
    // center) lands on START. Two clicks cover the startup-screen and the
    // menu itself; misses fall through to the click cadence below.
    let title_click = std::env::var("ASTRA_ARTEMIS_TEST_TITLE_CLICK")
        .ok()
        .and_then(|value| {
            let mut parts = value.split(',');
            let x = parts.next()?.trim().parse::<f32>().ok()?;
            let y = parts.next()?.trim().parse::<f32>().ok()?;
            Some((x, y))
        })
        .unwrap_or((stage.0 as f32 * 0.5, stage.1 as f32 * 0.5));
    // The opening logos and movie sit between boot and the title menu; a
    // blind settle carries the driver past them before aiming the title
    // click. Snapshots make the aimed position verifiable.
    snap(session.as_mut(), "00-early");
    settle(session.as_mut(), 2400);
    snap(session.as_mut(), "00-title");
    if step(session.as_mut(), &hover(title_click.0, title_click.1))
        || step(session.as_mut(), &click())
    {
        eprintln!("PLAYTHROUGH: engine exited during the title flow");
        session.close().expect("session closes");
        return;
    }
    settle(session.as_mut(), 900);
    snap(session.as_mut(), "01-after-title");

    // ---- Click-driven route completion ----
    let mut finished = false;
    let t0 = Instant::now();
    for i in 0..budget {
        // Hover precedes the press by one tick so queued Lua hover handlers
        // set the button cursor before the click edge lands.
        let events: Vec<FamilyEvent> = match i % 12 {
            0 => hover(stage.0 as f32 * 0.5, stage.1 as f32 * 0.5),
            2 => {
                let mut events = click();
                events.extend(enter_press());
                events
            }
            _ => Vec::new(),
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
                t0.elapsed(),
                (i + 1) as f64 / t0.elapsed().as_secs_f64().max(0.001)
            );
            session
                .visit_frame(&mut alive)
                .expect("route frames compose");
            snap(session.as_mut(), &format!("{:06}", i + 1));
        }
    }
    session
        .visit_frame(&mut alive)
        .expect("frames stay composed after the route run");

    // ---- Feature probe: Escape closes any opened system page ----
    escape(session.as_mut());
    settle(session.as_mut(), 60);
    session
        .visit_frame(&mut alive)
        .expect("frames stay composed through the probes");

    let pushed = common::pushed_frames();
    eprintln!(
        "PLAYTHROUGH: pushed audio frames = {pushed}, finished = {finished}, elapsed = {:?}",
        t0.elapsed()
    );
    assert!(pushed > 0, "audio flowed through the playthrough");
    session
        .close()
        .expect("session closes cleanly after the playthrough");
}
