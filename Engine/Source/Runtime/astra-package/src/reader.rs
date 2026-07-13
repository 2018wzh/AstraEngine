use std::collections::BTreeMap;

use crate::{
    AstraContainerReader, ContainerError, ContainerKind, PackageManifest, SchemaRegistryManifest,
    CURRENT_CONTAINER_VERSION,
};

const REQUIRED_SECTIONS: &[(&str, &str)] = &[
    ("package.manifest", "astra.package_manifest.v1"),
    ("schema.registry", "astra.schema_registry.v2"),
    ("cook.summary", "astra.cook_batch_summary.v1"),
    ("asset.vfs_manifest", "astra.asset_vfs_manifest.v1"),
    ("asset.catalog", "astra.asset_catalog.v1"),
    ("media.manifest", "astra.media_manifest.v1"),
    ("provider.policy", "astra.provider_policy.v2"),
    (
        "plugin.extension_registry",
        "astra.plugin_extension_registry.v2",
    ),
    (
        "plugin.dependency_graph",
        "astra.plugin_dependency_graph.v1",
    ),
    ("module.fingerprint", "astra.module_fingerprint.v1"),
    ("target.manifest", "astra.target_manifest.v1"),
    ("release.summary", "astra.release_summary.v1"),
    ("scenario.refs", "astra.scenario_refs.v2"),
    ("platform.eligibility", "astra.platform_eligibility.v1"),
];

#[derive(Debug, Clone)]
pub struct PackageReader {
    container: AstraContainerReader,
}

impl PackageReader {
    pub fn open(bytes: &[u8]) -> Result<Self, ContainerError> {
        let container = AstraContainerReader::new(bytes)?;
        if container.kind() != ContainerKind::Package {
            return Err(ContainerError::message("container is not a package"));
        }
        for (id, schema) in REQUIRED_SECTIONS {
            let entry = container.section_entry(id).ok_or_else(|| {
                ContainerError::message(format!("package is missing required section {id}"))
            })?;
            if entry.schema != *schema {
                return Err(ContainerError::message(format!(
                    "required section {id} uses unsupported schema {}",
                    entry.schema
                )));
            }
        }
        let manifest: PackageManifest = container.decode_postcard("package.manifest")?;
        if manifest.schema != "astra.package_manifest.v1"
            || manifest.container_version != CURRENT_CONTAINER_VERSION
            || manifest.package_id.trim().is_empty()
            || manifest.profile.trim().is_empty()
        {
            return Err(ContainerError::message(
                "package manifest identity or version is invalid",
            ));
        }
        let policy_bytes = container.read_bounded("provider.policy", 256 * 1024)?;
        let extension_registry_bytes =
            container.read_bounded("plugin.extension_registry", 256 * 1024)?;
        let target_manifest_bytes = container.read_bounded("target.manifest", 256 * 1024)?;
        let vfs_manifest_bytes = container.read_bounded("asset.vfs_manifest", 16 * 1024 * 1024)?;
        crate::authority::validate_provider_authority(
            &manifest.package_id,
            &manifest.profile,
            &policy_bytes,
            &extension_registry_bytes,
            &target_manifest_bytes,
            &vfs_manifest_bytes,
        )?;
        let registry_bytes = container.read_bounded("schema.registry", 16 * 1024 * 1024)?;
        let registry: SchemaRegistryManifest =
            serde_json::from_slice(&registry_bytes).map_err(|error| {
                ContainerError::message(format!("schema registry decode failed: {error}"))
            })?;
        if registry.schema != "astra.schema_registry.v2" {
            return Err(ContainerError::message(
                "unsupported package schema registry version",
            ));
        }
        let mut registered = BTreeMap::new();
        for schema in registry.schemas {
            let section_id = schema.section_id;
            if registered
                .insert(section_id.clone(), (schema.schema, schema.version))
                .is_some()
            {
                return Err(ContainerError::message(format!(
                    "schema registry contains duplicate section {}",
                    section_id
                )));
            }
        }
        for entry in container
            .entries()
            .iter()
            .filter(|entry| entry.id != "schema.registry")
        {
            let Some((schema, version)) = registered.get(entry.id.as_str()) else {
                return Err(ContainerError::message(format!(
                    "section {} is not registered in schema.registry",
                    entry.id
                )));
            };
            if schema != &entry.schema || *version != entry.version {
                return Err(ContainerError::message(format!(
                    "section {} schema registry binding does not match its table entry",
                    entry.id
                )));
            }
        }
        if registered.len() + 1 != container.entries().len() {
            return Err(ContainerError::message(
                "schema registry contains entries without package sections",
            ));
        }
        let reader = Self { container };
        let cook_summary: crate::CookSummaryManifest =
            serde_json::from_slice(&reader.container.read_bounded("cook.summary", 1024 * 1024)?)
                .map_err(|error| {
                    ContainerError::message(format!("cook summary decode failed: {error}"))
                })?;
        cook_summary.validate()?;
        let cooked_asset_count = reader
            .container
            .entries()
            .iter()
            .filter(|entry| entry.schema == "astra.cooked_asset.v1")
            .count() as u64;
        if cooked_asset_count != cook_summary.artifact_count {
            return Err(ContainerError::message(
                "package cooked asset sections do not match cook.summary artifact_count",
            ));
        }
        let scenario_refs: crate::ScenarioRefsManifest = serde_json::from_slice(
            &reader
                .container
                .read_bounded("scenario.refs", 16 * 1024 * 1024)?,
        )
        .map_err(|error| {
            ContainerError::message(format!("scenario refs decode failed: {error}"))
        })?;
        scenario_refs.validate(&reader)?;
        Ok(reader)
    }

    pub fn has_section(&self, id: &str) -> bool {
        self.container.has_section(id)
    }

    pub fn container(&self) -> &AstraContainerReader {
        &self.container
    }
}
