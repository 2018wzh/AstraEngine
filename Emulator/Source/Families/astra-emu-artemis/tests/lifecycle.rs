use abi_stable::{
    std_types::{ROption, RResult},
    type_level::downcasting::TD_Opaque,
};
use astra_emu_artemis::ArtemisProvider;
use astra_emu_family_api::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
struct Sink(Arc<AtomicBool>);
impl AudioSink for Sink {
    fn configure(&self, _: PcmFormatSpec) -> FfiFamilyResult<()> {
        RResult::ROk(())
    }
    fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        RResult::ROk(AudioWriteStatus::Accepted)
    }
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn cancel(&self) -> FfiFamilyResult<()> {
        self.0.store(true, Ordering::Release);
        RResult::ROk(())
    }
}
// Public-domain miniature archive generated from the documented PF6 layout.
fn archive(files: &[(&[u8], &[u8])]) -> Vec<u8> {
    let entries: usize = files.iter().map(|(n, _)| 16 + n.len()).sum();
    let index = 4 + entries + 4 + 8 * (files.len() + 1) + 4;
    let mut bytes = b"pf6".to_vec();
    bytes.extend((index as u32).to_le_bytes());
    bytes.extend((files.len() as u32).to_le_bytes());
    let mut offset = 7 + index;
    for (name, data) in files {
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(*name);
        bytes.extend([0; 4]);
        bytes.extend((offset as u32).to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        offset += data.len();
    }
    bytes.extend(((files.len() + 1) as u32).to_le_bytes());
    bytes.extend(vec![0; 8 * (files.len() + 1)]);
    bytes.extend(((11 + entries) as u32).to_le_bytes());
    for (_, data) in files {
        bytes.extend(*data);
    }
    bytes
}
fn request(root: &std::path::Path, cancelled: Arc<AtomicBool>) -> OpenRequest {
    OpenRequest {
        session_epoch: astra_emu_family_api::allocate_session_epoch().unwrap(),
        game_path: root.to_string_lossy().to_string().into(),
        configuration: Vec::new().into(),
        initial_window: WindowState {
            width: 64,
            height: 48,
            focused: true,
            visible: true,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RSome(AudioSink_TO::from_value(Sink(cancelled), TD_Opaque)),
            text_replacement: ROption::RNone,
            modal: ROption::RNone,
        },
    }
}
#[test]
fn malformed_archive_does_not_start_audio_or_leave_session_lease() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("data.pfs"), b"bad").unwrap();
    let mut provider = ArtemisProvider::default();
    for _ in 0..2 {
        let cancelled = Arc::new(AtomicBool::new(false));
        let failure = provider
            .open(request(root.path(), cancelled))
            .err()
            .unwrap();
        assert_eq!(failure.code.as_str(), "ASTRA_EMU_ARTEMIS_PROJECT_INI");
    }
}
#[test]
#[ignore = "requires a hardware native GPU adapter"]
fn native_gpu_family_opens_advances_closes_and_reopens() {
    let root = tempfile::tempdir().unwrap();
    let source = archive(&[
        (
            b"system.ini",
            b"[WINDOWS]\nWIDTH=64\nHEIGHT=48\nBOOT=boot.iet\nCHARSET=UTF-8\n",
        ),
        (b"boot.iet", b"[stop]\n"),
    ]);
    std::fs::write(root.path().join("data.pfs"), source).unwrap();
    let mut provider = ArtemisProvider::default();
    for _ in 0..3 {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut open = provider
            .open(request(root.path(), cancelled.clone()))
            .unwrap();
        assert_eq!(
            (open.response.frame.width, open.response.frame.height),
            (64, 48)
        );
        open.session.advance(16_666_667, &[]).unwrap();
        let FramePoll::Ready(baseline) = open.session.acquire_frame(None).unwrap() else {
            panic!("boot must publish a completed native frame");
        };
        for _ in 0..60 {
            open.session.advance(16_666_667, &[]).unwrap();
            assert!(matches!(
                open.session.acquire_frame(Some(baseline.identity)).unwrap(),
                FramePoll::Unchanged(identity) if identity == baseline.identity
            ));
        }
        open.session
            .advance(0, &[FamilyEvent::WindowSuspended { suspended: true }])
            .unwrap();
        open.session
            .advance(0, &[FamilyEvent::WindowSuspended { suspended: false }])
            .unwrap();
        open.session.close().unwrap();
        assert!(cancelled.load(Ordering::Acquire));
        assert!(baseline
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }
}

#[test]
#[ignore = "requires a hardware native GPU adapter"]
fn undersized_native_capture_is_an_error_and_preserves_the_completed_target() {
    use art3m1s_core::{backend::BackendSelection, runtime::CoreRuntime};
    #[cfg(target_os = "macos")]
    let backend = BackendSelection::NativeMetal;
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "android"))]
    let backend = BackendSelection::NativeVulkan;
    let mut runtime = CoreRuntime::create(64, 48, backend).unwrap();
    assert!(runtime.try_advance_frame(16).unwrap());
    let size = runtime.pixel_buffer_size();
    let mut baseline = vec![0; size];
    assert_eq!(runtime.try_capture_frame_into(&mut baseline).unwrap(), size);
    let mut too_small = vec![0x7b; size - 1];
    assert!(runtime.try_capture_frame_into(&mut too_small).is_err());
    assert!(too_small.iter().all(|byte| *byte == 0x7b));
    let mut recovered = vec![0xaa; size];
    assert_eq!(
        runtime.try_capture_frame_into(&mut recovered).unwrap(),
        size
    );
    assert_eq!(recovered, baseline);
    assert!(!runtime.try_advance_frame(16).unwrap());
}
