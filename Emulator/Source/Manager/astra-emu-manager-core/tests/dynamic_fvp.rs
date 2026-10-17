use astra_emu_manager_core::model::{
    allocate_session_epoch, ConfigEntry, ConfigKind, ConfigValue, FramePoll, ProbeRequest,
    WindowState,
};
use astra_emu_manager_core::{
    no_host_services, FamilyCapability, FamilyProviderRegistry, NativeCancellation,
    NativeOpenRequest,
};
use std::path::Path;

fn fixture(root: &Path) {
    let mut hcb = vec![0; 4];
    for (key, value) in [(0_u16, 99_u8), (2, 42)] {
        hcb.extend_from_slice(&[12, value, 21]);
        hcb.extend_from_slice(&key.to_le_bytes());
    }
    let wait = hcb.len() as u32;
    hcb.extend_from_slice(&[3, 0, 0, 6]);
    hcb.extend_from_slice(&wait.to_le_bytes());
    let descriptor = hcb.len() as u32;
    hcb[..4].copy_from_slice(&descriptor.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&2_u16.to_le_bytes());
    hcb.extend_from_slice(&3_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 1, 0]);
    hcb.extend_from_slice(&1_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 11]);
    hcb.extend_from_slice(b"ThreadNext\0");
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    std::fs::write(root.join("fixture.hcb"), hcb).unwrap();
}

fn request(game: &Path) -> NativeOpenRequest {
    let epoch = allocate_session_epoch().unwrap();
    let cancellation = NativeCancellation::new(epoch);
    NativeOpenRequest {
        session_epoch: epoch,
        game_path: game.to_string_lossy().into_owned(),
        configuration: vec![ConfigEntry {
            id: "script_encoding".into(),
            value: ConfigValue::Enum("utf8".into()),
        }],
        initial_window: WindowState {
            width: 640,
            height: 480,
            scale_factor: 1.0,
            focused: true,
            visible: true,
        },
        host: no_host_services(&cancellation),
        cancellation,
        d3d12: None,
    }
}

#[test]
#[ignore = "requires an explicitly built FVP dynamic plugin binary"]
fn fvp_dynamic_core_scans_probes_configures_and_reopens() {
    let plugin = std::env::var_os("ASTRA_EMU_TEST_PLUGIN")
        .expect("ASTRA_EMU_TEST_PLUGIN must identify the plugin under test");
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let game = root.path().join("fixture");
    std::fs::create_dir_all(&cores).unwrap();
    std::fs::create_dir_all(&game).unwrap();
    std::fs::copy(
        plugin,
        cores.join(format!("astra_emu_fvp.{}", std::env::consts::DLL_EXTENSION)),
    )
    .unwrap();
    fixture(&game);

    let mut registry = FamilyProviderRegistry::new();
    let load = registry.load_directory(&cores).unwrap();
    assert_eq!(load.loaded, 1);
    assert!(load.errors.is_empty());
    let descriptor = registry.descriptor("astra.emu.fvp").unwrap();
    assert_eq!(descriptor.family_id, "fvp");
    assert!(descriptor.has_capability(FamilyCapability::CpuFrame));

    let schema = registry.configuration("astra.emu.fvp").unwrap();
    let encoding = schema
        .iter()
        .find(|field| field.id == "script_encoding")
        .expect("native script encoding schema");
    match &encoding.kind {
        ConfigKind::Enum { choices } => {
            assert!(choices.iter().any(|choice| choice.as_str() == "utf8"));
        }
        _ => panic!("script_encoding must be an enum"),
    }

    let selection = registry
        .probe(
            &ProbeRequest {
                game_path: game.to_string_lossy().into_owned(),
            },
            Some("astra.emu.fvp"),
            Some("fvp"),
        )
        .unwrap();
    let candidate = selection.candidates().first().cloned().unwrap();
    assert_eq!(candidate.report.game_id, "fixture");
    assert_eq!(candidate.report.format, "fvp.hcb");

    for _ in 0..2 {
        let mut session = registry.open_selected(&candidate, request(&game)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let lease = loop {
            if let FramePoll::Ready(frame) = session.acquire_frame(None).unwrap() {
                break frame;
            }
            assert!(std::time::Instant::now() < deadline, "native frame timeout");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(lease.info.width != 0 && lease.info.height != 0);
        session.close().unwrap();
        assert!(
            !lease.pixels().unwrap().is_empty(),
            "CPU lease remains valid after native close"
        );
    }
}
