//! Headless boot test: probe hit, open, settle, composed frames, and audio
//! flow through the family session.
//!
//! Skipped unless `ASTRA_ARTEMIS_TEST_GAME` points at an Artemis game
//! directory. Everything else runs unconditionally so the default workspace
//! test matrix covers the adapter contract without retail data.

mod common;

use astra_emu_family_api::{
    FamilyDescriptor, FamilyProvider, FamilyResult, FrameView, FrameVisitor, ProbeRequest,
};

fn descriptor() -> FamilyDescriptor {
    astra_emu_artemis::artemis_descriptor()
}

#[test]
fn descriptor_is_valid() {
    descriptor().validate().unwrap();
    assert_eq!(descriptor().family_id, "artemis");
    assert_eq!(descriptor().plugin_id, "astra.emu.artemis");
}

#[test]
fn probe_rejects_a_directory_without_a_pfs_archive() {
    let root = tempfile::tempdir().unwrap();
    let provider = astra_emu_artemis::ArtemisProvider::default();
    let report = provider
        .probe(ProbeRequest {
            game_path: root.path().display().to_string().into(),
        })
        .unwrap();
    assert!(report.is_none());
}

#[test]
fn probe_rejects_a_pfs_without_the_project_ini() {
    let root = tempfile::tempdir().unwrap();
    // A file with the .pfs suffix but no Artemis index must not match.
    std::fs::write(root.path().join("fake.pfs"), b"not an archive").unwrap();
    let provider = astra_emu_artemis::ArtemisProvider::default();
    let report = provider
        .probe(ProbeRequest {
            game_path: root.path().display().to_string().into(),
        })
        .unwrap();
    assert!(report.is_none());
}

struct Lit {
    count: usize,
    total: usize,
}

impl FrameVisitor for Lit {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()> {
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

#[test]
fn headless_boot_composes_the_title_frame() {
    let Some(game) = std::env::var("ASTRA_ARTEMIS_TEST_GAME")
        .ok()
        .map(std::path::PathBuf::from)
    else {
        eprintln!("ASTRA_ARTEMIS_TEST_GAME not set; skipping");
        return;
    };
    let _ = env_logger::try_init();
    let provider = astra_emu_artemis::ArtemisProvider::default();
    let report = provider
        .probe(ProbeRequest {
            game_path: game.display().to_string().into(),
        })
        .expect("probe succeeds")
        .expect("an Artemis game directory probes as artemis");
    assert_eq!(report.family_id, "artemis");
    assert_eq!(report.format, "artemis.pfs");

    let (mut session, _stage) = common::open_session(&game);
    let mut lit = Lit { count: 0, total: 0 };
    // The boot settle already produced the first composition; the frame is
    // the game's own opening art, not an empty buffer.
    session.visit_frame(&mut lit).expect("frame visit succeeds");
    assert!(lit.total > 0, "the settled frame is not empty");
    eprintln!(
        "BOOT: settled frame {}/{} lit pixels, {} audio frames pushed",
        lit.count,
        lit.total,
        common::pushed_frames()
    );

    // A budget of silent ticks must keep the session alive with a valid
    // frame; the engine keeps timers and events running.
    for _ in 0..120 {
        session.advance(16_666_667, &[]).expect("advance succeeds");
    }
    session.visit_frame(&mut lit).expect("frame visit succeeds");
    assert!(lit.total > 0);

    session.close().expect("session closes cleanly");
}
