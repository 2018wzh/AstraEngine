use astra_package::{ContainerError, PackageReader, SectionPayload};

use crate::{
    CompiledStory, VnAdvancedPresentationManifest, VnCommercialBaselineManifest,
    VnExtensionManifest, VnPolicyBundleManifest, VnPolicyBundleSourceCache,
    VnPresentationProviderManifest, VnProfileManifest, VnStandardCommandManifest,
    VnSystemUiProfileManifest,
};

pub fn package_sections_for_story(
    compiled: &CompiledStory,
    profiles: &[String],
    target: &str,
) -> Result<Vec<SectionPayload>, ContainerError> {
    tracing::info!(
        event = "vn.package.build.start",
        profile_count = profiles.len(),
        state_count = compiled.states.len(),
        target,
        "AstraVN package section build started"
    );
    let manifest = VnProfileManifest {
        schema: "astra.vn.profile_manifest.v1".to_string(),
        target: target.to_string(),
        profiles: profiles.to_vec(),
    };
    let mut sections = vec![
        SectionPayload::postcard("vn.compiled_story", "astra.vn.compiled_story", compiled)?,
        SectionPayload::postcard(
            "vn.profile_manifest",
            "astra.vn.profile_manifest.v1",
            &manifest,
        )?,
        SectionPayload::postcard(
            "vn.policy_bundle_manifest",
            "astra.policy_bundle.v1",
            &VnPolicyBundleManifest::standard(),
        )?,
        SectionPayload::postcard(
            "vn.policy_bundle_source_cache",
            "astra.vn.policy_bundle_source_cache.v1",
            &VnPolicyBundleSourceCache::standard(),
        )?,
        SectionPayload::postcard(
            "vn.extension_manifest",
            "astra.vn.extension_manifest.v1",
            &VnExtensionManifest::standard(),
        )?,
        SectionPayload::postcard(
            "vn.standard_command_manifest",
            "astra.vn.standard_command_manifest.v1",
            &VnStandardCommandManifest::standard(),
        )?,
        SectionPayload::postcard(
            "vn.presentation_provider_manifest",
            "astra.vn.presentation_provider_manifest.v1",
            &VnPresentationProviderManifest::standard(),
        )?,
        SectionPayload::postcard(
            "vn.commercial_baseline_manifest",
            "astra.vn.commercial_baseline_manifest.v1",
            &VnCommercialBaselineManifest::from_compiled(compiled),
        )?,
        SectionPayload::postcard(
            "vn.system_story_manifest",
            "astra.vn.system_story_manifest.v1",
            &compiled.system_story_manifest,
        )?,
        SectionPayload::postcard(
            "vn.system_ui_profile_manifest",
            "astra.vn.system_ui_profile_manifest.v1",
            &VnSystemUiProfileManifest::from_compiled(compiled, vec!["zh-Hans".to_string()]),
        )?,
        SectionPayload::raw(
            "scenario.refs",
            "astra.scenario_refs.v1",
            serde_json::json!({
                "schema": "astra.scenario_refs.v1",
                "scenarios": []
            })
            .to_string()
            .into_bytes(),
        ),
    ];
    if profiles
        .iter()
        .any(|profile| VnAdvancedPresentationManifest::profile_requires_advanced(profile))
    {
        sections.push(SectionPayload::postcard(
            "vn.advanced_presentation_manifest",
            "astra.vn.advanced_presentation_manifest.v1",
            &VnAdvancedPresentationManifest::from_compiled(compiled, "advanced-vn"),
        )?);
    }
    tracing::info!(
        event = "vn.package.build.complete",
        section_count = sections.len(),
        target,
        "AstraVN package section build completed"
    );
    Ok(sections)
}

pub fn decode_compiled_story(package: &PackageReader) -> Result<CompiledStory, ContainerError> {
    let entry = package
        .container()
        .section_entry("vn.compiled_story")
        .ok_or_else(|| ContainerError::message("vn.compiled_story section is missing"))?;
    if entry.schema == "astra.vn.compiled_story.v1" {
        return Err(ContainerError::message(
            "ASTRA_VN_RECOOK_REQUIRED: legacy compiled story package must be re-cooked",
        ));
    }
    if entry.schema != "astra.vn.compiled_story" {
        return Err(ContainerError::message(format!(
            "ASTRA_VN_COMPILED_STORY_SCHEMA: unsupported compiled story schema {}",
            entry.schema
        )));
    }
    let compiled: CompiledStory = package
        .container()
        .decode_postcard("vn.compiled_story")
        .map_err(|_| {
            ContainerError::message(
                "ASTRA_VN_RECOOK_REQUIRED: compiled story payload does not use the current layout",
            )
        })?;
    if compiled.schema != "astra.vn.compiled_story" {
        return Err(ContainerError::message(
            "ASTRA_VN_COMPILED_STORY_SCHEMA: compiled story payload schema is not canonical",
        ));
    }
    Ok(compiled)
}
