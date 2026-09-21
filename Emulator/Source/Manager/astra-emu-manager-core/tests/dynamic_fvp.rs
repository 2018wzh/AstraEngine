use abi_stable::{std_types::ROption, type_level::downcasting::TD_Opaque};
use astra_emu_family_api::{
    AudioSink, AudioSink_TO, AudioWriteStatus, ConfigEntry, ConfigKind, ConfigValue,
    FamilyHostServices, FfiFamilyResult, OpenRequest, PcmChunk, PcmFormatSpec, WindowState,
};
use astra_emu_manager_core::{FamilyCapability, FamilyProviderRegistry};
use std::path::Path;

struct AcceptingSink;

impl AudioSink for AcceptingSink {
    fn configure(&self, _: PcmFormatSpec) -> FfiFamilyResult<()> {
        Ok(()).into()
    }

    fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        Ok(AudioWriteStatus::Accepted).into()
    }

    fn is_cancelled(&self) -> bool {
        false
    }

    fn cancel(&self) -> FfiFamilyResult<()> {
        Ok(()).into()
    }
}

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

fn request(game: &Path) -> OpenRequest {
    OpenRequest {
        game_path: game.to_string_lossy().to_string().into(),
        configuration: vec![ConfigEntry {
            id: "script_encoding".into(),
            value: ConfigValue::Enum("utf8".into()),
        }]
        .into(),
        initial_window: WindowState {
            width: 640,
            height: 480,
            focused: true,
            visible: true,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RSome(AudioSink_TO::from_value(AcceptingSink, TD_Opaque)),
            text_replacement: ROption::RNone,
        },
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
    assert!(descriptor.has_capability(FamilyCapability::PcmAudio));
    assert!(descriptor.has_capability(FamilyCapability::NativeSave));

    let schema = registry.configuration("astra.emu.fvp").unwrap();
    assert_eq!(schema.len(), 1);
    assert_eq!(schema[0].id.as_str(), "script_encoding");
    match &schema[0].kind {
        ConfigKind::Enum { choices } => {
            assert!(choices.iter().any(|choice| choice.as_str() == "utf8"));
        }
        _ => panic!("script_encoding must be an enum"),
    }

    let selection = registry
        .probe(
            &astra_emu_family_api::ProbeRequest {
                game_path: game.to_string_lossy().to_string().into(),
            },
            Some("astra.emu.fvp"),
            Some("fvp"),
        )
        .unwrap();
    let candidate = selection.candidates().first().cloned().unwrap();
    assert_eq!(candidate.report.game_id, "fixture");
    assert_eq!(candidate.report.format, "fvp.hcb");

    for _ in 0..2 {
        let opened = registry.open_selected(&candidate, request(&game)).unwrap();
        assert_eq!(opened.response.frame.width, 640);
        assert_eq!(opened.response.frame.height, 480);
        assert!(opened.response.audio_format.is_some());
        opened.session.close().unwrap();
    }
}
