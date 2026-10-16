#[path = "texture_lifecycle_tests.rs"]
mod texture_lifecycle_tests;

use super::*;
use abi_stable::{std_types::ROption, type_level::downcasting::TD_Opaque};
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, FamilyHostServices, FfiFamilyResult, PcmChunk,
    PcmFormatSpec, WindowState,
};
use rfvp::{script::Variant, subsystem::global_savedata::try_decode_global_savedata_v1};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

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
        .open_session(request(root, cancelled))
        .map(|(_, session)| session)
}

fn await_frame(
    session: &mut FvpSession,
    after: Option<astra_emu_family_api::FrameIdentity>,
) -> astra_emu_family_api::FrameLease {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match session.acquire_frame(after).unwrap() {
            FramePoll::Ready(frame) => return frame,
            FramePoll::Pending(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            other => panic!("expected completed new native frame, got {other:?}"),
        }
    }
}

fn request(root: &Path, cancelled: Arc<AtomicBool>) -> OpenRequest {
    OpenRequest {
        session_epoch: astra_emu_family_api::allocate_session_epoch().unwrap(),
        configuration: Default::default(),
        game_path: root.to_str().unwrap().into(),
        initial_window: WindowState {
            width: 640,
            height: 480,
            focused: true,
            visible: true,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RSome(AudioSink_TO::from_value(TestSink(cancelled), TD_Opaque)),
            text_replacement: ROption::RNone,
            modal: ROption::RNone,
        },
    }
}

#[cfg(feature = "dynamic-plugin-export")]
#[test]
fn dynamic_module_reopens_on_windows_sized_stack() {
    use astra_emu_family_api::{FamilyModule, SessionRequest};
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let root = fixture();
            let module = astra_emu_family_api::ProviderModule::<crate::FvpProvider>::default();
            // Keep session storage boxed across repeated opens: inline session
            // values previously exhausted the desktop GUI thread stack.
            for _ in 0..3 {
                let cancelled = Arc::new(AtomicBool::new(false));
                let opened = module
                    .open(request(root.path(), cancelled.clone()))
                    .into_result()
                    .unwrap();
                module
                    .close(SessionRequest {
                        session_id: opened.session_id,
                    })
                    .into_result()
                    .unwrap();
                assert!(cancelled.load(Ordering::Acquire));
            }
        })
        .unwrap()
        .join()
        .unwrap();
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
fn persistence_failure_propagates_after_audio_worker_cleanup() {
    let root = fixture();
    let cancelled = Arc::new(AtomicBool::new(false));
    let session = open(root.path(), Arc::clone(&cancelled)).unwrap();
    std::fs::create_dir_all(root.path().join("save/rfvp_global.bin")).unwrap();
    Box::new(session).close().unwrap_err();
    assert!(cancelled.load(Ordering::Acquire));
    assert!(root.path().join("save/rfvp_global.bin").is_dir());
}

#[test]
fn unchanged_native_scene_keeps_published_generation_and_pixel_owner() {
    let root = fixture();
    let mut session = open(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    let baseline = await_frame(&mut session, None);
    for _ in 0..60 {
        session.advance(16_666_667, &[]).unwrap();
        assert!(
            matches!(session.acquire_frame(Some(baseline.identity)).unwrap(),
            FramePoll::Unchanged(identity) if identity == baseline.identity)
        );
    }
    Box::new(session).close().unwrap();
    assert!(baseline
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel == &[0, 0, 0, 255]));
}

#[test]
fn scripted_movie_reuses_frame_then_restores_native_game_after_eof() {
    use rfvp::script::opcode::Opcode;
    let root = tempfile::tempdir().unwrap();
    let mut hcb = vec![0; 4];
    hcb.extend_from_slice(&[Opcode::PushString as u8, 15]);
    hcb.extend_from_slice(b"movie/clip.wmv\0");
    hcb.extend_from_slice(&[Opcode::PushTrue as u8, Opcode::Syscall as u8, 1, 0]);
    let wait = hcb.len() as u32;
    hcb.extend_from_slice(&[Opcode::Syscall as u8, 0, 0, Opcode::Jmp as u8]);
    hcb.extend_from_slice(&wait.to_le_bytes());
    let descriptor = hcb.len() as u32;
    hcb[..4].copy_from_slice(&descriptor.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 1, 0]);
    hcb.extend_from_slice(&2_u16.to_le_bytes());
    for (args, name) in [(0, b"ThreadNext\0".as_slice()), (2, b"Movie\0".as_slice())] {
        hcb.extend_from_slice(&[args, name.len() as u8]);
        hcb.extend_from_slice(name);
    }
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    std::fs::write(root.path().join("fixture.hcb"), hcb).unwrap();
    let movie = include_bytes!("../../na_wmv_player/tests/fixtures/multi-payload.wmv");
    let mut pack = Vec::new();
    for value in [1_u32, 9, 0, 29, movie.len() as u32] {
        pack.extend_from_slice(&value.to_le_bytes());
    }
    pack.extend_from_slice(b"clip.wmv\0");
    pack.extend_from_slice(movie);
    std::fs::write(root.path().join("movie.bin"), pack).unwrap();
    let mut session = open(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
    let FramePoll::Ready(game) = session.acquire_frame(None).unwrap() else {
        panic!("initial native game frame missing");
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let first = loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        session.advance(10_000_000, &[]).unwrap();
        if let FramePoll::Ready(frame) = session.acquire_frame(Some(game.identity)).unwrap() {
            if frame.pixels() != game.pixels() {
                break frame;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native movie did not present"
        );
    };
    let mut latest = first.clone();
    let mut next = None;
    let restored = loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        session.advance(10_000_000, &[]).unwrap();
        if let FramePoll::Ready(frame) = session.acquire_frame(Some(latest.identity)).unwrap() {
            if frame.pixels() == game.pixels() {
                break frame;
            }
            if frame.pixels() != first.pixels() {
                next = Some(frame.clone());
            }
            latest = frame;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native movie did not reach EOF"
        );
    };
    let next = next.expect("native decoder must present another movie frame");
    Box::new(session).close().unwrap();
    assert!(first.pixels() != game.pixels());
    assert!(next.pixels() != game.pixels());
    assert_eq!(restored.pixels(), game.pixels());
}

#[test]
fn saved_thumbnail_matches_native_gpu_at_prepare_not_later_write() {
    use rfvp::script::opcode::Opcode;
    use rfvp::subsystem::resources::save_manager::{SaveItem, SaveManager};
    fn call(hcb: &mut Vec<u8>, id: u16, args: &[Option<i16>]) {
        for value in args {
            match value {
                Some(value) => {
                    hcb.push(Opcode::PushI16 as u8);
                    hcb.extend_from_slice(&value.to_le_bytes());
                }
                None => hcb.push(Opcode::PushNil as u8),
            }
        }
        hcb.push(Opcode::Syscall as u8);
        hcb.extend_from_slice(&id.to_le_bytes());
    }
    let root = tempfile::tempdir().unwrap();
    let mut hcb = vec![0; 4];
    call(
        &mut hcb,
        1,
        &[Some(1), Some(128), Some(0), Some(0), Some(255)],
    );
    call(
        &mut hcb,
        2,
        &[Some(1), Some(1), Some(0), Some(0), Some(640), Some(480)],
    );
    call(&mut hcb, 3, &[Some(1), Some(0)]);
    call(&mut hcb, 0, &[]);
    call(&mut hcb, 4, &[Some(3), None]);
    call(&mut hcb, 0, &[]);
    call(
        &mut hcb,
        1,
        &[Some(1), Some(0), Some(128), Some(0), Some(255)],
    );
    call(&mut hcb, 5, &[Some(20)]);
    let wait = hcb.len() as u32;
    call(&mut hcb, 0, &[]);
    hcb.push(Opcode::Jmp as u8);
    hcb.extend_from_slice(&wait.to_le_bytes());
    let descriptor = hcb.len() as u32;
    hcb[..4].copy_from_slice(&descriptor.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 1, 0]);
    hcb.extend_from_slice(&6_u16.to_le_bytes());
    for (args, name) in [
        (0, "ThreadNext"),
        (5, "ColorSet"),
        (6, "PrimSetTile"),
        (2, "PrimGroupIn"),
        (2, "SaveCreate"),
        (1, "SaveWrite"),
    ] {
        hcb.extend_from_slice(&[args, (name.len() + 1) as u8]);
        hcb.extend_from_slice(name.as_bytes());
        hcb.push(0);
    }
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    std::fs::write(root.path().join("fixture.hcb"), hcb).unwrap();
    let mut session = open(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    let prepared = await_frame(&mut session, None);
    let prepared_rgb = &prepared.pixels()[..3];
    assert!(prepared_rgb[0] > 0 && prepared_rgb[1] == 0 && prepared_rgb[2] == 0);
    session.advance(16_666_667, &[]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    let menu = await_frame(&mut session, Some(prepared.identity));
    assert!(menu.pixels()[0] == 0 && menu.pixels()[1] > 0 && menu.pixels()[2] == 0);
    let bytes = std::fs::read(root.path().join("save/rfvp_s020.bin")).unwrap();
    let native = SaveManager::new();
    let thumb = SaveItem::read_thumb_texture_from_mem(
        &bytes,
        native.get_thumb_width(),
        native.get_thumb_height(),
    )
    .unwrap();
    assert!(thumb
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[..3] == *prepared_rgb));
    Box::new(session).close().unwrap();
}
