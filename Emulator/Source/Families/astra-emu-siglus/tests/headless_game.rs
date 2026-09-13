//! Headless smoke test against a real SiglusEngine game directory.
//!
//! Skipped unless `ASTRA_SIGLUS_TEST_GAME` points at a game directory (with
//! its `key.toml` when the title is protected). Drives the family through the
//! static provider: boot, drive title/dialogue inputs, confirm composed
//! frames gain visible content and keep changing, confirm mixed PCM arrives,
//! then close cleanly and open a fresh session.

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

struct FrameDigest {
    len: usize,
    sum: u64,
    lit_pixels: usize,
}

impl FrameVisitor for FrameDigest {
    fn accept(&mut self, frame: FrameView<'_>) -> astra_emu_family_api::FamilyResult<()> {
        self.len = frame.as_slice().len();
        self.sum = frame
            .as_slice()
            .iter()
            .fold(0_u64, |acc, byte| acc.wrapping_add(u64::from(*byte)));
        // Count pixels a human could see: any color channel above a low
        // threshold. A fade-from-black boot keeps this at zero for a while.
        self.lit_pixels = frame
            .as_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[..3].iter().any(|channel| *channel > 24))
            .count();
        Ok(())
    }
}

fn digest(session: &dyn FamilySession) -> FrameDigest {
    let mut digest = FrameDigest {
        len: 0,
        sum: 0,
        lit_pixels: 0,
    };
    session
        .visit_frame(&mut digest)
        .expect("frame visit succeeds");
    digest
}

fn test_game() -> Option<std::path::PathBuf> {
    std::env::var("ASTRA_SIGLUS_TEST_GAME").ok().map(Into::into)
}

fn no_modifiers() -> KeyModifiers {
    KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    }
}

/// One driven frame of the title/intro flow: cycles hover, click, and Enter
/// so the universal advance controls are all exercised.
fn drive_events(index: u32) -> Vec<FamilyEvent> {
    match index % 6 {
        0 => vec![
            FamilyEvent::PointerMove { x: 640.0, y: 400.0 },
            FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            },
        ],
        1 => vec![FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Released,
        }],
        3 => vec![
            FamilyEvent::Key {
                code: KeyCode::Enter,
                state: KeyState::Pressed,
                modifiers: no_modifiers(),
            },
            FamilyEvent::Key {
                code: KeyCode::Enter,
                state: KeyState::Released,
                modifiers: no_modifiers(),
            },
        ],
        _ => Vec::new(),
    }
}

fn open_request(game: &std::path::Path) -> OpenRequest {
    OpenRequest {
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
    }
}

#[test]
fn headless_boot_inputs_frames_audio_and_close() {
    let Some(game) = test_game() else {
        eprintln!("ASTRA_SIGLUS_TEST_GAME not set; skipping");
        return;
    };
    let mut provider = astra_emu_siglus::SiglusProvider::default();
    let open = provider
        .open(open_request(&game))
        .expect("siglus session opens");
    let FamilyOpen {
        response,
        mut session,
    } = open;
    assert!(
        response.frame.width > 0 && response.frame.height > 0,
        "open reports a real frame size"
    );

    // Drive the title flow long enough for the boot fade and the first
    // scripted presentation to finish. The composition must produce visible
    // content and keep changing while inputs advance the flow.
    let boot = digest(session.as_ref());
    assert_eq!(
        boot.len,
        (response.frame.width * response.frame.height * 4) as usize,
        "frame buffer matches the reported size"
    );
    let mut max_lit = boot.lit_pixels;
    let mut distinct_sums = std::collections::BTreeSet::new();
    distinct_sums.insert(boot.sum);
    for index in 0..150_u32 {
        let events = drive_events(index);
        let result = session
            .advance(16_666_667, &events)
            .expect("advance succeeds");
        let snapshot = digest(session.as_ref());
        max_lit = max_lit.max(snapshot.lit_pixels);
        distinct_sums.insert(snapshot.sum);
        if result.status == FamilyStatus::Finished {
            break;
        }
    }
    assert!(
        max_lit > boot.len / 4 / 100,
        "driven flow shows visible content (max lit pixels = {max_lit})"
    );
    assert!(
        distinct_sums.len() > 1,
        "frames change across the driven inputs (distinct digests = {})",
        distinct_sums.len()
    );

    // Mixed PCM must have reached the host sink during the run.
    let pushed = PUSHED_FRAMES.load(Ordering::Relaxed);
    assert!(pushed > 4_800, "audio arrived (pushed frames = {pushed})");

    session.close().expect("session closes cleanly");

    // The provider must accept a fresh session after a clean close.
    let reopen = provider
        .open(open_request(&game))
        .expect("a second session opens after a clean close");
    let FamilyOpen {
        response: reopened,
        session: second,
    } = reopen;
    assert_eq!(
        (reopened.frame.width, reopened.frame.height),
        (response.frame.width, response.frame.height),
        "second session reports the same frame size"
    );
    second.close().expect("second session closes cleanly");
}
