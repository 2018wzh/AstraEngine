use super::*;
use abi_stable::{std_types::RResult, type_level::downcasting::TD_Opaque};
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FfiFamilyResult, PcmChunk, PcmFormatSpec,
};

struct Sink;
impl AudioSink for Sink {
    fn configure(&self, _: PcmFormatSpec) -> FfiFamilyResult<()> {
        RResult::ROk(())
    }
    fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        RResult::ROk(AudioWriteStatus::Accepted)
    }
    fn is_cancelled(&self) -> bool {
        false
    }
    fn cancel(&self) -> FfiFamilyResult<()> {
        RResult::ROk(())
    }
}

#[test]
#[ignore = "requires a hardware native GPU adapter"]
fn successful_script_load_clears_submillisecond_remainder_once() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("boot.iet"),
        b"[save file=clock.dat]\n[wait input=1]\n[load file=clock.dat]\n[stop]\n",
    )
    .unwrap();
    let resources = HostResources::new();
    resources.mount_directory(root.path()).unwrap();
    resources.set_save_dir(Some(root.path())).unwrap();
    let host_events = HostEvents::new();
    host_events.set_enabled(true);
    let ini = |boot: &str| {
        format!("[WINDOWS]\nWIDTH=8\nHEIGHT=8\nBOOT={boot}\nSAVEPATH=save\nCHARSET=UTF-8\n")
    };
    let opened = boot_engine(
        &resources,
        &host_events,
        ini("boot.iet").as_bytes(),
        WindowState {
            width: 8,
            height: 8,
            focused: true,
            visible: true,
        },
        AudioSink_TO::from_value(Sink, TD_Opaque),
        provider::SessionLease::acquire().unwrap(),
        FramePublisher::new(astra_emu_family_api::allocate_session_epoch().unwrap()).unwrap(),
    )
    .unwrap();
    let mut session = Box::new(opened.session);
    assert!(resources.read_file("save/clock.dat").is_ok());
    assert!(!session.advance(0, &[]).unwrap().reset_clock);
    session.elapsed_remainder_ns = 999_999;
    let loaded = session
        .advance(
            1,
            &[FamilyEvent::PointerButton {
                button: astra_emu_family_api::PointerButton::Primary,
                state: astra_emu_family_api::KeyState::Pressed,
            }],
        )
        .unwrap();
    assert!(loaded.reset_clock);
    assert_eq!(session.elapsed_remainder_ns, 0);
    assert!(!session.advance(499_999, &[]).unwrap().reset_clock);
    assert_eq!(session.elapsed_remainder_ns, 499_999);
    session.close().unwrap();
}
