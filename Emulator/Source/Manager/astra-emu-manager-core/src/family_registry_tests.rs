use super::*;
use astra_emu_family_api::{
    FamilyCapability as AbiCapability, FamilyDescriptor, FamilyResult, OpenRequest, ProbeReport,
    ProbeRequest,
};
use std::path::Path;

struct ProbeProvider {
    descriptor: FamilyDescriptor,
    report: Option<ProbeReport>,
}

impl FamilyProvider for ProbeProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        Ok(self.descriptor.clone())
    }

    fn probe(&self, _request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        Ok(self.report.clone())
    }

    fn open(&mut self, _request: OpenRequest) -> FamilyResult<FamilyOpen> {
        unreachable!("probe registry test does not open sessions")
    }
}

fn descriptor(plugin_id: &str, family_id: &str) -> FamilyDescriptor {
    FamilyDescriptor {
        configuration: Default::default(),
        family_id: family_id.into(),
        plugin_id: plugin_id.into(),
        abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
        version: "1.0.0".into(),
        capabilities: vec![AbiCapability::CpuFrame].into(),
        supported_formats: vec!["fvp.hcb".into()].into(),
    }
}

fn report(family_id: &str, confidence_permyriad: u16) -> ProbeReport {
    ProbeReport {
        family_id: family_id.into(),
        game_id: "game".into(),
        format: "fvp.hcb".into(),
        confidence_permyriad,
    }
}

#[test]
fn static_providers_use_same_multi_probe_policy() {
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(ProbeProvider {
            descriptor: descriptor("a", "fvp"),
            report: Some(report("fvp", 8_000)),
        })
        .unwrap();
    registry
        .register_provider(ProbeProvider {
            descriptor: descriptor("b", "fvp"),
            report: Some(report("fvp", 9_000)),
        })
        .unwrap();
    let selection = registry
        .probe(
            &ProbeRequest {
                game_path: "game".into(),
            },
            None,
            None,
        )
        .unwrap();
    assert!(selection.requires_user_choice());
    assert_eq!(selection.candidates()[0].report.plugin_id, "b");
}

fn library_path(directory: &Path, name: &str) -> std::path::PathBuf {
    directory.join(format!("{name}.{}", std::env::consts::DLL_EXTENSION))
}

#[test]
fn core_directory_filters_dependency_libraries_by_family_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let family = library_path(directory.path(), "astra_emu_broken");
    let dependency = library_path(directory.path(), "avcodec-62");
    std::fs::write(&family, b"not a library").unwrap();
    std::fs::write(&dependency, b"not a library").unwrap();
    std::fs::write(directory.path().join("astra_emu_notes.txt"), b"ignored").unwrap();

    let mut registry = FamilyProviderRegistry::new();
    let report = registry.load_directory(directory.path()).unwrap();
    assert_eq!(report.loaded, 0);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].path, family);
    assert!(!is_family_library_path(&dependency));
}

#[test]
#[ignore = "requires two explicitly built Family plugin binaries"]
fn core_directory_rejects_duplicate_plugin_ids_as_a_group() {
    let first = std::env::var_os("ASTRA_EMU_TEST_PLUGIN_A")
        .expect("ASTRA_EMU_TEST_PLUGIN_A must identify a plugin");
    let second = std::env::var_os("ASTRA_EMU_TEST_PLUGIN_B")
        .expect("ASTRA_EMU_TEST_PLUGIN_B must identify a plugin");
    let directory = tempfile::tempdir().unwrap();
    let first_path = library_path(directory.path(), "astra_emu_first");
    let second_path = library_path(directory.path(), "astra_emu_second");
    std::fs::copy(first, &first_path).unwrap();
    std::fs::copy(second, &second_path).unwrap();

    let mut registry = FamilyProviderRegistry::new();
    let report = registry.load_directory(directory.path()).unwrap();
    assert_eq!(report.loaded, 0);
    assert_eq!(report.errors.len(), 2);
    assert!(report
        .errors
        .iter()
        .all(|entry| entry.error == FamilyLoadError::DuplicatePlugin));
    assert!(registry.descriptors().next().is_none());
}
