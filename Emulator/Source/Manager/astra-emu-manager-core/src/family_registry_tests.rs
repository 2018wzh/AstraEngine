use super::*;
use astra_emu_family_api::{
    FamilyCapability as AbiCapability, FamilyDescriptor, FamilyResult, OpenRequest, ProbeReport,
    ProbeRequest,
};

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
