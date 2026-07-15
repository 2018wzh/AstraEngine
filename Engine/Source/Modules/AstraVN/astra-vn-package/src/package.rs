use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Hash128, Hash256, SourceRef};
use astra_package::{ContainerError, PackageReader, SectionPayload};
use astra_ui_core::{UiBindingManifest, UiBlueprintBundle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CompiledStory, CompiledVnProject, VnAdvancedPresentationManifest, VnCommercialBaselineManifest,
    VnExtensionManifest, VnPolicyBundleManifest, VnPolicyBundleSourceCache,
    VnPresentationProviderManifest, VnProfileManifest, VnStandardCommandManifest,
    VnSystemUiProfileManifest, VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnCompiledProjectRoot {
    pub schema: String,
    pub project_hash: Hash256,
    pub story_hash: Hash128,
    pub ui_blueprint_hash: Hash256,
    pub ui_binding_hash: Hash256,
    pub ui_provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnUiIdManifest {
    pub schema: String,
    pub ids: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnUiBackendManifest {
    pub schema: String,
    pub provider_id: String,
    pub input_protocol: String,
    pub render_protocol: String,
}

pub fn package_sections_for_project(
    project: &CompiledVnProject,
    profiles: &[String],
    target: &str,
) -> Result<Vec<SectionPayload>, ContainerError> {
    tracing::info!(
        event = "vn.package.build.start",
        profile_count = profiles.len(),
        state_count = project.story.states.len(),
        target,
        "AstraVN package section build started"
    );
    let manifest = VnProfileManifest {
        schema: "astra.vn.profile_manifest.v1".to_string(),
        target: target.to_string(),
        profiles: profiles.to_vec(),
    };
    let root = VnCompiledProjectRoot {
        schema: "astra.vn.compiled_project_root.v1".to_string(),
        project_hash: project.project_hash,
        story_hash: project.story.story_hash,
        ui_blueprint_hash: project.ui_blueprints.hash,
        ui_binding_hash: project.ui_bindings.hash,
        ui_provider: "astra.ui.yakui".to_string(),
    };
    let mut sections = vec![
        SectionPayload::postcard(
            "vn.compiled_project",
            "astra.vn.compiled_project_root.v1",
            &root,
        )?,
        SectionPayload::postcard("vn.story", "astra.vn.story", &project.story)?,
        SectionPayload::postcard(
            "vn.ui_blueprint_bundle",
            "astra.ui_blueprint_bundle.v1",
            &project.ui_blueprints,
        )?,
        SectionPayload::postcard(
            "vn.ui_binding_manifest",
            "astra.ui_binding_manifest.v1",
            &project.ui_bindings,
        )?,
        SectionPayload::postcard(
            "vn.ui_source_map",
            "astra.vn.ui_source_map.v1",
            &project.ui_source_map,
        )?,
        SectionPayload::postcard(
            "vn.ui_controller_manifest",
            "astra.vn.ui_controller_manifest.v1",
            &VnUiIdManifest {
                schema: "astra.vn.ui_controller_manifest.v1".to_string(),
                ids: project.controller_ids.clone(),
            },
        )?,
        SectionPayload::postcard(
            "vn.ui_theme_manifest",
            "astra.ui_theme_bundle.v1",
            &project.themes,
        )?,
        SectionPayload::postcard(
            "vn.ui_backend_manifest",
            "astra.vn.ui_backend_manifest.v1",
            &VnUiBackendManifest {
                schema: "astra.vn.ui_backend_manifest.v1".to_string(),
                provider_id: "astra.ui.yakui".to_string(),
                input_protocol: "astra.ui_input_frame.v1".to_string(),
                render_protocol: "astra.ui_render_frame.v1".to_string(),
            },
        )?,
        SectionPayload::postcard(
            "vn.ui_component_manifest",
            "astra.vn.ui_component_manifest.v1",
            &VnUiIdManifest {
                schema: "astra.vn.ui_component_manifest.v1".to_string(),
                ids: project.component_ids.clone(),
            },
        )?,
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
            "astra.vn.standard_command_manifest.v2",
            &VnStandardCommandManifest::standard(),
        )?,
        SectionPayload::postcard(
            "vn.presentation_provider_manifest",
            VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA,
            &VnPresentationProviderManifest::standard(),
        )?,
        SectionPayload::postcard(
            "vn.commercial_baseline_manifest",
            "astra.vn.commercial_baseline_manifest.v1",
            &VnCommercialBaselineManifest::from_compiled(&project.story),
        )?,
        SectionPayload::postcard(
            "vn.system_story_manifest",
            "astra.vn.system_story_manifest.v1",
            &project.story.system_story_manifest,
        )?,
        SectionPayload::postcard(
            "vn.system_ui_profile_manifest",
            "astra.vn.system_ui_profile_manifest.v1",
            &VnSystemUiProfileManifest::from_compiled(&project.story, vec!["zh-Hans".to_string()]),
        )?,
    ];
    if profiles
        .iter()
        .any(|profile| VnAdvancedPresentationManifest::profile_requires_advanced(profile))
    {
        sections.push(SectionPayload::postcard(
            "vn.advanced_presentation_manifest",
            "astra.vn.advanced_presentation_manifest.v1",
            &VnAdvancedPresentationManifest::from_compiled(&project.story, "advanced-vn"),
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

pub fn load_presentation_provider_manifest(
    package: &PackageReader,
    profile: &str,
) -> Result<VnPresentationProviderManifest, ContainerError> {
    let entry = package
        .container()
        .section_entry("vn.presentation_provider_manifest")
        .ok_or_else(|| {
            ContainerError::message(
                "ASTRA_VN_PRESENTATION_PROVIDER_MANIFEST: package section is missing",
            )
        })?;
    if entry.schema != VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA {
        return Err(ContainerError::message(format!(
            "ASTRA_VN_PRESENTATION_PROVIDER_SCHEMA: unsupported schema {}; package must be re-cooked",
            entry.schema
        )));
    }
    let manifest: VnPresentationProviderManifest = package
        .container()
        .decode_postcard("vn.presentation_provider_manifest")
        .map_err(|error| {
            ContainerError::message(format!("ASTRA_VN_PRESENTATION_PROVIDER_MANIFEST: {error}"))
        })?;
    let report = manifest.validate_standard();
    if let Some(diagnostic) = report.diagnostics.first() {
        return Err(ContainerError::message(format!(
            "{}: {}",
            diagnostic.code, diagnostic.message
        )));
    }
    manifest.profile(profile).map_err(|diagnostic| {
        ContainerError::message(format!("{}: {}", diagnostic.code, diagnostic.message))
    })?;
    Ok(manifest)
}

pub fn decode_compiled_project(
    package: &PackageReader,
) -> Result<CompiledVnProject, ContainerError> {
    if package.has_section("vn.compiled_story") {
        return Err(ContainerError::message(
            "ASTRA_VN_RECOOK_REQUIRED: vn.compiled_story is removed; project must be re-cooked",
        ));
    }
    let root: VnCompiledProjectRoot = package.container().decode_postcard("vn.compiled_project")?;
    if root.schema != "astra.vn.compiled_project_root.v1" || root.ui_provider != "astra.ui.yakui" {
        return Err(ContainerError::message(
            "ASTRA_VN_COMPILED_PROJECT_ROOT: invalid project root or UI provider",
        ));
    }
    let story: CompiledStory = package.container().decode_postcard("vn.story")?;
    let ui_blueprints: UiBlueprintBundle = package
        .container()
        .decode_postcard("vn.ui_blueprint_bundle")?;
    let ui_bindings: UiBindingManifest = package
        .container()
        .decode_postcard("vn.ui_binding_manifest")?;
    let ui_source_map: BTreeMap<String, SourceRef> =
        package.container().decode_postcard("vn.ui_source_map")?;
    let controllers: VnUiIdManifest = package
        .container()
        .decode_postcard("vn.ui_controller_manifest")?;
    let themes: BTreeMap<String, astra_ui_core::UiThemeManifest> = package
        .container()
        .decode_postcard("vn.ui_theme_manifest")?;
    let components: VnUiIdManifest = package
        .container()
        .decode_postcard("vn.ui_component_manifest")?;
    if story.story_hash != root.story_hash
        || ui_blueprints.hash != root.ui_blueprint_hash
        || ui_bindings.hash != root.ui_binding_hash
    {
        return Err(ContainerError::message(
            "ASTRA_VN_COMPILED_PROJECT_HASH: project section hashes do not match the root",
        ));
    }
    let project_hash = Hash256::from_sha256(
        &postcard::to_allocvec(&(
            story.story_hash,
            ui_blueprints.hash,
            ui_bindings.hash,
            &themes,
        ))
        .map_err(|error| {
            ContainerError::message(format!("ASTRA_VN_COMPILED_PROJECT_ENCODE: {error}"))
        })?,
    );
    if project_hash != root.project_hash {
        return Err(ContainerError::message(
            "ASTRA_VN_COMPILED_PROJECT_HASH: project root hash does not cover the packaged sections",
        ));
    }
    Ok(CompiledVnProject {
        schema: "astra.vn.compiled_project.v1".to_string(),
        project_hash: root.project_hash,
        story,
        ui_blueprints,
        ui_bindings,
        ui_source_map,
        controller_ids: controllers.ids,
        theme_ids: themes.keys().cloned().collect(),
        themes,
        component_ids: components.ids,
    })
}
