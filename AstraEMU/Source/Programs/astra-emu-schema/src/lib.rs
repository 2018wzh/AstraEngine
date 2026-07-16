use std::{collections::BTreeMap, fs, path::Path};

use astra_emu_cli::HeadlessRunReportV1;
use astra_emu_family_api::{
    LegacyFamilyPluginDescriptor, LegacyProbeReport, LegacySnapshotEnvelope, LegacyStepInput,
    LegacyStepOutput,
};
use astra_emu_manager_core::{
    AndroidNativePluginManifest, EmuCaseProfile, EmuPlatformRunEvidenceV1,
    EmuProviderBindingEvidenceV1, EmuReleaseManifestV1, FamilyPluginManifest, FvpParityEvidence,
    FvpSyscallCoverageEvidence, LibraryMigrationEvidence, ProbeEvidence, TranslationConsent,
    TranslationEvidence, TrustedLuauEvidenceV1, UiHostIdentityEvidence,
};
use astra_emu_translation_openai_compatible::TranslationProfile;
use schemars::{schema::RootSchema, schema_for};

pub fn schemas() -> BTreeMap<&'static str, RootSchema> {
    BTreeMap::from([
        (
            "astra-emu-headless-run-report.schema.json",
            schema_for!(HeadlessRunReportV1),
        ),
        (
            "android-native-plugin-manifest.schema.json",
            schema_for!(AndroidNativePluginManifest),
        ),
        ("case-profile.schema.json", schema_for!(EmuCaseProfile)),
        (
            "emu-platform-run-evidence.schema.json",
            schema_for!(EmuPlatformRunEvidenceV1),
        ),
        (
            "emu-provider-binding.schema.json",
            schema_for!(EmuProviderBindingEvidenceV1),
        ),
        (
            "emu-release-manifest.schema.json",
            schema_for!(EmuReleaseManifestV1),
        ),
        (
            "family-descriptor.schema.json",
            schema_for!(LegacyFamilyPluginDescriptor),
        ),
        (
            "family-plugin-manifest.schema.json",
            schema_for!(FamilyPluginManifest),
        ),
        (
            "fvp-coverage.schema.json",
            schema_for!(FvpSyscallCoverageEvidence),
        ),
        ("fvp-parity.schema.json", schema_for!(FvpParityEvidence)),
        ("legacy-probe.schema.json", schema_for!(LegacyProbeReport)),
        (
            "legacy-snapshot.schema.json",
            schema_for!(LegacySnapshotEnvelope),
        ),
        (
            "legacy-step-input.schema.json",
            schema_for!(LegacyStepInput),
        ),
        (
            "legacy-step-output.schema.json",
            schema_for!(LegacyStepOutput),
        ),
        (
            "library-migration.schema.json",
            schema_for!(LibraryMigrationEvidence),
        ),
        ("probe-evidence.schema.json", schema_for!(ProbeEvidence)),
        (
            "translation-consent.schema.json",
            schema_for!(TranslationConsent),
        ),
        (
            "translation-evidence.schema.json",
            schema_for!(TranslationEvidence),
        ),
        (
            "translation-profile.schema.json",
            schema_for!(TranslationProfile),
        ),
        (
            "trusted-luau-evidence.schema.json",
            schema_for!(TrustedLuauEvidenceV1),
        ),
        (
            "ui-host-identity.schema.json",
            schema_for!(UiHostIdentityEvidence),
        ),
    ])
}

pub fn generate(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(output)?;
    for (name, schema) in schemas() {
        let destination = output.join(name);
        let temporary = output.join(format!(".{name}.tmp"));
        fs::write(&temporary, schema_bytes(&schema)?)?;
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(temporary, destination)?;
    }
    Ok(())
}

fn schema_bytes(schema: &RootSchema) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(schema)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_schemas_match_rust_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Schemas");
        for (name, schema) in schemas() {
            let committed = fs::read(root.join(name)).expect("committed schema is missing");
            assert_eq!(
                committed,
                schema_bytes(&schema).unwrap(),
                "generated schema drifted: {name}"
            );
        }
    }
}
