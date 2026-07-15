use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Hash128, Hash256, SourceRef};
use astra_package::{ContainerError, PackageReader, SectionPayload};
use astra_ui_core::{UiBindingManifest, UiBlueprintBundle};
use astra_ui_plugin_abi::UiComponentManifest;
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
pub struct VnUiComponentBundleManifest {
    pub schema: String,
    pub ids: BTreeSet<String>,
    pub bindings: Vec<VnUiComponentBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnUiComponentBinding {
    pub component_id: String,
    pub windows_manifest_section: String,
    pub windows_artifact_section: String,
    pub web_manifest_section: String,
    pub web_artifact_section: String,
    pub signer_id: String,
    pub signer_public_key: [u8; 32],
    pub signer_key_fingerprint: Hash256,
}

#[derive(Debug, Clone)]
pub struct VnUiComponentArtifactInput {
    pub target: VnUiComponentTarget,
    pub manifest: UiComponentManifest,
    pub artifact: Vec<u8>,
    pub signer_public_key: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VnUiComponentTarget {
    Windows,
    Web,
}

impl VnUiComponentTarget {
    fn id(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Web => "web",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedVnUiComponentArtifact {
    pub manifest: UiComponentManifest,
    pub artifact: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnUiControllerBundle {
    pub schema: String,
    pub sources: BTreeMap<String, String>,
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
    package_sections_for_project_with_components(project, profiles, target, &[])
}

pub fn package_sections_for_project_with_components(
    project: &CompiledVnProject,
    profiles: &[String],
    target: &str,
    component_artifacts: &[VnUiComponentArtifactInput],
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
    let (component_bundle, mut component_sections) =
        build_component_sections(&project.component_ids, component_artifacts)?;
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
            &VnUiControllerBundle {
                schema: "astra.vn.ui_controller_manifest.v1".to_string(),
                sources: project.controller_sources.clone(),
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
            "astra.vn.ui_component_bundle.v1",
            &component_bundle,
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
    sections.append(&mut component_sections);
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

fn build_component_sections(
    declared_ids: &BTreeSet<String>,
    inputs: &[VnUiComponentArtifactInput],
) -> Result<(VnUiComponentBundleManifest, Vec<SectionPayload>), ContainerError> {
    let mut grouped =
        BTreeMap::<String, BTreeMap<VnUiComponentTarget, &VnUiComponentArtifactInput>>::new();
    for input in inputs {
        if input.artifact.is_empty() || input.artifact.len() > 64 * 1024 * 1024 {
            return Err(ContainerError::message(
                "ASTRA_VN_UI_COMPONENT_ARTIFACT_SIZE: component artifact must contain 1..=64 MiB",
            ));
        }
        input
            .manifest
            .verify(
                &input.artifact,
                &BTreeMap::from([(input.manifest.signer_id.clone(), input.signer_public_key)]),
            )
            .map_err(|error| {
                ContainerError::message(format!("ASTRA_VN_UI_COMPONENT_TRUST: {error}"))
            })?;
        if !declared_ids.contains(&input.manifest.component_id) {
            return Err(ContainerError::message(
                "ASTRA_VN_UI_COMPONENT_UNDECLARED: packaged component is not declared by UI source",
            ));
        }
        let targets = grouped
            .entry(input.manifest.component_id.clone())
            .or_default();
        if targets.insert(input.target, input).is_some() {
            return Err(ContainerError::message(
                "ASTRA_VN_UI_COMPONENT_TARGET_DUPLICATE: component target artifact is duplicated",
            ));
        }
    }
    if grouped.keys().cloned().collect::<BTreeSet<_>>() != *declared_ids {
        return Err(ContainerError::message(
            "ASTRA_VN_UI_COMPONENT_ARTIFACT_SET: every declared component requires trusted Windows and Web artifacts",
        ));
    }
    let mut bindings = Vec::new();
    let mut sections = Vec::new();
    for component_id in declared_ids {
        let targets = grouped.get(component_id).ok_or_else(|| {
            ContainerError::message("ASTRA_VN_UI_COMPONENT_ARTIFACT_SET: component is missing")
        })?;
        let windows = targets.get(&VnUiComponentTarget::Windows).ok_or_else(|| {
            ContainerError::message(
                "ASTRA_VN_UI_COMPONENT_WINDOWS_MISSING: Windows artifact is required",
            )
        })?;
        let web = targets.get(&VnUiComponentTarget::Web).ok_or_else(|| {
            ContainerError::message("ASTRA_VN_UI_COMPONENT_WEB_MISSING: Web artifact is required")
        })?;
        if windows.manifest.signer_id != web.manifest.signer_id
            || windows.signer_public_key != web.signer_public_key
        {
            return Err(ContainerError::message(
                "ASTRA_VN_UI_COMPONENT_SIGNER_MISMATCH: Windows and Web artifacts require one signer identity",
            ));
        }
        let section_prefix = format!("vn.ui_component.{component_id}");
        let windows_manifest = format!("{section_prefix}.windows.manifest");
        let windows_artifact = format!("{section_prefix}.windows.artifact");
        let web_manifest = format!("{section_prefix}.web.manifest");
        let web_artifact = format!("{section_prefix}.web.artifact");
        for (input, manifest_section, artifact_section) in [
            (*windows, &windows_manifest, &windows_artifact),
            (*web, &web_manifest, &web_artifact),
        ] {
            sections.push(SectionPayload::postcard(
                manifest_section,
                "astra.ui_component_manifest.v1",
                &input.manifest,
            )?);
            sections.push(SectionPayload::raw(
                artifact_section,
                "astra.ui_component_artifact.v1",
                input.artifact.clone(),
            ));
        }
        bindings.push(VnUiComponentBinding {
            component_id: component_id.clone(),
            windows_manifest_section: windows_manifest,
            windows_artifact_section: windows_artifact,
            web_manifest_section: web_manifest,
            web_artifact_section: web_artifact,
            signer_id: windows.manifest.signer_id.clone(),
            signer_public_key: windows.signer_public_key,
            signer_key_fingerprint: Hash256::from_sha256(&windows.signer_public_key),
        });
    }
    Ok((
        VnUiComponentBundleManifest {
            schema: "astra.vn.ui_component_bundle.v1".to_string(),
            ids: declared_ids.clone(),
            bindings,
        },
        sections,
    ))
}

pub fn load_ui_component_artifact(
    package: &PackageReader,
    component_id: &str,
    target: VnUiComponentTarget,
    signer_allowlist: &BTreeMap<String, [u8; 32]>,
) -> Result<LoadedVnUiComponentArtifact, ContainerError> {
    let bundle: VnUiComponentBundleManifest = package
        .container()
        .decode_postcard("vn.ui_component_manifest")?;
    if bundle.schema != "astra.vn.ui_component_bundle.v1" {
        return Err(ContainerError::message(
            "ASTRA_VN_UI_COMPONENT_BUNDLE_SCHEMA: package must be re-cooked",
        ));
    }
    let binding = bundle
        .bindings
        .iter()
        .find(|binding| binding.component_id == component_id)
        .ok_or_else(|| {
            ContainerError::message(
                "ASTRA_VN_UI_COMPONENT_BINDING_MISSING: component is not package-bound",
            )
        })?;
    let (manifest_section, artifact_section) = match target {
        VnUiComponentTarget::Windows => (
            &binding.windows_manifest_section,
            &binding.windows_artifact_section,
        ),
        VnUiComponentTarget::Web => (&binding.web_manifest_section, &binding.web_artifact_section),
    };
    let manifest_entry = package
        .container()
        .section_entry(manifest_section)
        .ok_or_else(|| ContainerError::message("ASTRA_VN_UI_COMPONENT_MANIFEST_MISSING"))?;
    let artifact_entry = package
        .container()
        .section_entry(artifact_section)
        .ok_or_else(|| ContainerError::message("ASTRA_VN_UI_COMPONENT_ARTIFACT_MISSING"))?;
    if manifest_entry.schema != "astra.ui_component_manifest.v1"
        || artifact_entry.schema != "astra.ui_component_artifact.v1"
    {
        return Err(ContainerError::message(
            "ASTRA_VN_UI_COMPONENT_SECTION_SCHEMA: component section schema is invalid",
        ));
    }
    let manifest: UiComponentManifest = package.container().decode_postcard(manifest_section)?;
    let artifact = package.container().read_section(artifact_section)?;
    if manifest.component_id != component_id
        || manifest.signer_id != binding.signer_id
        || signer_allowlist
            .get(&binding.signer_id)
            .map(|key| Hash256::from_sha256(key))
            != Some(binding.signer_key_fingerprint)
        || signer_allowlist.get(&binding.signer_id) != Some(&binding.signer_public_key)
    {
        return Err(ContainerError::message(
            "ASTRA_VN_UI_COMPONENT_BINDING_IDENTITY: component signer or identity differs from package binding",
        ));
    }
    manifest
        .verify(&artifact, signer_allowlist)
        .map_err(|error| {
            ContainerError::message(format!("ASTRA_VN_UI_COMPONENT_TRUST: {error}"))
        })?;
    tracing::info!(
        event = "vn.ui_component.load",
        component_id,
        target = target.id(),
        artifact_hash = %manifest.artifact_hash,
        "loaded trusted UI component artifact"
    );
    Ok(LoadedVnUiComponentArtifact { manifest, artifact })
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
    let controllers: VnUiControllerBundle = package
        .container()
        .decode_postcard("vn.ui_controller_manifest")?;
    let themes: BTreeMap<String, astra_ui_core::UiThemeManifest> = package
        .container()
        .decode_postcard("vn.ui_theme_manifest")?;
    let components: VnUiComponentBundleManifest = package
        .container()
        .decode_postcard("vn.ui_component_manifest")?;
    if components.schema != "astra.vn.ui_component_bundle.v1"
        || components.ids.len() != components.bindings.len()
        || components
            .bindings
            .iter()
            .map(|binding| binding.component_id.clone())
            .collect::<BTreeSet<_>>()
            != components.ids
    {
        return Err(ContainerError::message(
            "ASTRA_VN_UI_COMPONENT_BUNDLE: component binding manifest is invalid",
        ));
    }
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
            &controllers.sources,
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
        controller_ids: controllers.sources.keys().cloned().collect(),
        controller_sources: controllers.sources,
        theme_ids: themes.keys().cloned().collect(),
        themes,
        component_ids: components.ids,
    })
}
