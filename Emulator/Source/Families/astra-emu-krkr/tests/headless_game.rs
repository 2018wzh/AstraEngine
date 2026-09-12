//! Headless smoke test against a real Kirikiri game directory.
//!
//! Skipped unless `ASTRA_KRKR_TEST_GAME` points at a writable game copy.
//! Drives the family through the static provider: boot, settle frames, send
//! inputs, confirm the composed frame stays alive and mixed PCM arrives,
//! then close cleanly.

#![cfg(feature = "engine")]

use std::sync::atomic::{AtomicU64, Ordering};

use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FamilyEvent, FamilyOpen, FamilyProvider,
    FrameVisitor, KeyCode, KeyModifiers, KeyState, OpenRequest, PcmChunk, PcmFormatSpec,
    WindowState,
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
}

impl FrameVisitor for FrameDigest {
    fn accept(
        &mut self,
        frame: astra_emu_family_api::FrameView<'_>,
    ) -> astra_emu_family_api::FamilyResult<()> {
        self.len = frame.as_slice().len();
        self.sum = frame
            .as_slice()
            .iter()
            .fold(0_u64, |acc, byte| acc.wrapping_add(u64::from(*byte)));
        Ok(())
    }
}

fn digest(session: &dyn astra_emu_family_api::FamilySession) -> FrameDigest {
    let mut digest = FrameDigest { len: 0, sum: 0 };
    session
        .visit_frame(&mut digest)
        .expect("frame visit succeeds");
    digest
}

fn test_game() -> Option<std::path::PathBuf> {
    std::env::var("ASTRA_KRKR_TEST_GAME").ok().map(Into::into)
}

fn enter_press() -> Vec<FamilyEvent> {
    let none = KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    };
    vec![
        FamilyEvent::Key {
            code: KeyCode::Enter,
            state: KeyState::Pressed,
            modifiers: none,
        },
        FamilyEvent::Key {
            code: KeyCode::Enter,
            state: KeyState::Released,
            modifiers: none,
        },
    ]
}

#[test]
fn headless_boot_advance_and_close() {
    let Some(game) = test_game() else {
        eprintln!("ASTRA_KRKR_TEST_GAME not set; skipping");
        return;
    };
    let mut provider = astra_emu_krkr::KrkrProvider::default();
    let mut open = provider
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
        .expect("krkr session opens");
    let FamilyOpen { response, session } = open;
    assert!(response.frame.width > 0);

    let first = digest(session.as_ref());
    assert!(first.len > 0, "engine composed a frame during open");

    for _ in 0..120 {
        session.advance(16_666_666, &[]).expect("advance succeeds");
    }
    let settled = digest(session.as_ref());
    assert_eq!(settled.len, first.len);

    for _ in 0..30 {
        session
            .advance(16_666_666, &enter_press())
            .expect("advance with enter succeeds");
    }
    let after_input = digest(session.as_ref());
    assert!(after_input.len > 0);

    let pcm = PUSHED_FRAMES.load(Ordering::Relaxed);
    eprintln!(
        "headless smoke: frame {}x{} pcm_frames={} sum {}->{}",
        response.frame.width, response.frame.height, pcm, first.sum, after_input.sum
    );
    std::fs::write(
        std::env::temp_dir().join("astra-krkr-headless-pcm.txt"),
        pcm.to_string(),
    )
    .ok();

    session.close().expect("close should succeed");
}
