use super::*;
use abi_stable::{std_types::ROption, type_level::downcasting::TD_Opaque};
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FamilyHostServices, FfiFamilyResult, PcmChunk,
    PcmFormatSpec, WindowState,
};
use rfvp::{script::Variant, subsystem::global_savedata::try_decode_global_savedata_v1};
use std::sync::atomic::{AtomicBool, Ordering};

struct TestSink(Arc<AtomicBool>);
impl AudioSink for TestSink {
    fn configure(&self, _: PcmFormatSpec) -> FfiFamilyResult<()> {
        Ok(()).into()
    }
    fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        Ok(AudioWriteStatus::Accepted).into()
    }
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn cancel(&self) -> FfiFamilyResult<()> {
        self.0.store(true, Ordering::Release);
        Ok(()).into()
    }
}

fn fixture() -> tempfile::TempDir {
    use rfvp::script::opcode::Opcode;
    let root = tempfile::tempdir().unwrap();
    let mut hcb = vec![0; 4];
    // Set a slot-local variable and a system variable, then yield indefinitely.
    for (key, value) in [(0_u16, 99_u8), (2, 42)] {
        hcb.extend_from_slice(&[Opcode::PushI8 as u8, value, Opcode::PopGlobal as u8]);
        hcb.extend_from_slice(&key.to_le_bytes());
    }
    let wait = hcb.len() as u32;
    hcb.extend_from_slice(&[Opcode::Syscall as u8, 0, 0, Opcode::Jmp as u8]);
    hcb.extend_from_slice(&wait.to_le_bytes());
    let descriptor = hcb.len() as u32;
    hcb[..4].copy_from_slice(&descriptor.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&2_u16.to_le_bytes());
    hcb.extend_from_slice(&3_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 1, 0]); // mode, reserved, title length, empty title
    hcb.extend_from_slice(&1_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 11]); // args and name length
    hcb.extend_from_slice(b"ThreadNext\0");
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    std::fs::write(root.path().join("fixture.hcb"), hcb).unwrap();
    root
}

fn open(root: &Path, cancelled: Arc<AtomicBool>) -> FamilyResult<FvpSession> {
    FvpProvider::default()
        .open_session(OpenRequest {
            game_path: root.to_str().unwrap().into(),
            initial_window: WindowState {
                width: 640,
                height: 480,
                focused: true,
                visible: true,
            },
            host: FamilyHostServices {
                audio_sink: ROption::RSome(AudioSink_TO::from_value(
                    TestSink(cancelled),
                    TD_Opaque,
                )),
                text_replacement: ROption::RNone,
            },
        })
        .map(|(_, session)| session)
}

#[test]
fn session_close_persists_script_changes_and_cold_open_loads_before_first_tick() {
    let root = fixture();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut session = open(root.path(), Arc::clone(&cancelled)).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    Box::new(session).close().unwrap();
    assert!(cancelled.load(Ordering::Acquire));
    let path = root.path().join("save/rfvp_global.bin");
    let bytes = std::fs::read(&path).unwrap();
    let state = try_decode_global_savedata_v1(&bytes).unwrap().unwrap();
    assert!(matches!(state.volatile_globals[0], Variant::Int(42)));

    let cold = open(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
    Box::new(cold).close().unwrap();
    let reloaded = try_decode_global_savedata_v1(&std::fs::read(path).unwrap())
        .unwrap()
        .unwrap();
    assert!(matches!(reloaded.volatile_globals[0], Variant::Int(42)));
}

#[test]
fn invalid_global_file_blocks_open_without_replacing_it() {
    let root = fixture();
    std::fs::create_dir(root.path().join("save")).unwrap();
    let path = root.path().join("save/rfvp_global.bin");
    std::fs::write(&path, b"invalid saved data").unwrap();
    let error = open(root.path(), Arc::new(AtomicBool::new(false)))
        .err()
        .expect("invalid persistence must reject the session");
    assert!(error.message.as_str().contains("global save load"));
    assert_eq!(std::fs::read(path).unwrap(), b"invalid saved data");
}

#[test]
fn persistence_failure_propagates_after_audio_worker_cleanup() {
    let root = fixture();
    let cancelled = Arc::new(AtomicBool::new(false));
    let session = open(root.path(), Arc::clone(&cancelled)).unwrap();
    std::fs::create_dir_all(root.path().join("save/rfvp_global.bin")).unwrap();
    let error = Box::new(session).close().unwrap_err();
    assert!(error.message.as_str().contains("global save write"));
    assert!(cancelled.load(Ordering::Acquire));
    assert!(root.path().join("save/rfvp_global.bin").is_dir());
}
