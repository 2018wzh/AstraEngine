use astra_plugin_abi::{
    LoadPhase, PluginExtensionRegistrySnapshot, ProductRuntimeDescriptor, ProviderBinding,
    ProviderBindingContext, ProviderExtensionRecord, ProviderPolicy, RuntimeOutputCodec,
    RuntimeOutputDomain, RuntimeOutputSchemaDescriptor, PLUGIN_EXTENSION_REGISTRY_SCHEMA,
    PROVIDER_POLICY_SCHEMA,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AstraContainerBuilder, ContainerBlob, ContainerError, ContainerKind, SectionPayload,
    CURRENT_CONTAINER_VERSION,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PackageManifest {
    pub schema: String,
    pub package_id: String,
    pub profile: String,
    pub container_version: astra_core::SchemaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageBuildRequest {
    pub package_id: String,
    pub profile: String,
    pub cooked_assets: Vec<SectionPayload>,
    pub cook_summary: Vec<u8>,
    pub asset_vfs_manifest: Vec<u8>,
    pub asset_catalog: Vec<u8>,
    pub media_manifest: Vec<u8>,
    pub provider_policy: Vec<u8>,
    pub plugin_extension_registry: Vec<u8>,
    pub plugin_dependency_graph: Vec<u8>,
    pub module_fingerprint: Vec<u8>,
    pub target_manifest: Vec<u8>,
    pub release_summary: Vec<u8>,
    pub scenario_refs: Vec<u8>,
    pub platform_eligibility: Vec<u8>,
    pub extra_sections: Vec<SectionPayload>,
}

impl PackageBuildRequest {
    pub fn fixture(
        package_id: impl Into<String>,
        profile: impl Into<String>,
        cooked_assets: Vec<SectionPayload>,
    ) -> Self {
        let package_id = package_id.into();
        let profile = profile.into();
        let cook_summary = CookSummaryManifest::from_sections(&cooked_assets);
        let (provider_policy, plugin_extension_registry) =
            default_fixture_provider_metadata(&package_id, &profile);
        Self {
            package_id: package_id.clone(),
            profile: profile.clone(),
            asset_vfs_manifest: default_asset_vfs_manifest(&cooked_assets),
            asset_catalog: default_asset_catalog(&cooked_assets, &profile),
            cooked_assets,
            cook_summary: json_bytes(
                serde_json::to_value(cook_summary)
                    .expect("fixture cook summary serialization must succeed"),
            ),
            media_manifest: json_bytes(serde_json::json!({
                "schema": "astra.media_manifest.v1",
                "codecs": ["png", "jpeg", "webp", "wav", "ogg", "flac", "mp3"],
                "ffmpeg": "optional"
            })),
            provider_policy,
            plugin_extension_registry,
            plugin_dependency_graph: json_bytes(serde_json::json!({
                "schema": "astra.plugin_dependency_graph.v1",
                "dependencies": []
            })),
            module_fingerprint: json_bytes(serde_json::json!({
                "schema": "astra.module_fingerprint.v1",
                "modules": []
            })),
            target_manifest: json_bytes(serde_json::json!({
                "schema": "astra.target_manifest.v2",
                "targets": [{
                    "id": "native-smoke-game",
                    "kind": "game",
                    "crate": "astra-runtime",
                    "runtime_provider": "native_vn",
                    "ui_provider": "astra.ui.yakui",
                    "default_profile": "desktop-release",
                    "platforms": ["windows", "linux", "macos", "ios", "android", "web"],
                    "packaged": true
                }]
            })),
            release_summary: json_bytes(serde_json::json!({
                "schema": "astra.release_summary.v1",
                "status": "unchecked"
            })),
            scenario_refs: json_bytes(serde_json::json!({
                "schema": "astra.scenario_refs.v2",
                "scenarios": []
            })),
            platform_eligibility: json_bytes(serde_json::json!({
                "schema": "astra.platform_eligibility.v1",
                "target": "native-smoke-game",
                "profiles": ["desktop-release", "headless"],
                "platforms": ["windows", "linux", "macos", "ios", "android", "web"]
            })),
            extra_sections: Vec::new(),
        }
    }
}

fn json_bytes(value: impl Serialize) -> Vec<u8> {
    serde_json::to_vec(&value).expect("fixture metadata serialization must succeed")
}

fn default_fixture_provider_metadata(package_id: &str, profile: &str) -> (Vec<u8>, Vec<u8>) {
    let provider_specs = [
        (
            "presentation",
            "astra.fixture.headless_presentation",
            "presentation.headless",
        ),
        ("vfs_provider", "astra.vfs.package", "vfs.backend.package"),
        (
            "game_runtime_provider",
            "astra.runtime.native_vn",
            "runtime.native_vn",
        ),
    ];
    let bindings = provider_specs
        .iter()
        .map(|(slot, provider_id, capability)| {
            ProviderBinding::new(
                *slot,
                *provider_id,
                ProviderBindingContext {
                    package_id: package_id.to_string(),
                    target: "native-smoke-game".to_string(),
                    profile: profile.to_string(),
                    required_capability: capability.to_string(),
                    engine_version: env!("CARGO_PKG_VERSION").to_string(),
                    rustc_fingerprint: "rustc-stable".to_string(),
                    feature_fingerprint: "runtime-envelope-v2".to_string(),
                    abi_fingerprint: "astra-plugin-abi-v2".to_string(),
                },
            )
            .expect("fixture provider binding must be valid")
        })
        .collect::<Vec<_>>();
    let providers = provider_specs
        .iter()
        .map(|(slot, provider_id, capability)| ProviderExtensionRecord {
            slot: slot.to_string(),
            provider_id: provider_id.to_string(),
            capability: capability.to_string(),
            phase: LoadPhase::Runtime,
            packaged: true,
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "runtime-envelope-v2".to_string(),
            abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        })
        .collect();
    let policy = ProviderPolicy {
        schema: PROVIDER_POLICY_SCHEMA.to_string(),
        profile: profile.to_string(),
        renderer: "astra.fixture.headless_presentation".to_string(),
        decode_fallback: "profile_bound".to_string(),
        runtime_provider: ProductRuntimeDescriptor {
            runtime_id: "native_vn".to_string(),
            product_kind: "visual_novel".to_string(),
            provider_id: "astra.runtime.native_vn".to_string(),
            supported_targets: vec!["game".to_string()],
            capabilities: vec!["runtime.native_vn".to_string()],
            package_sections: [
                "vn.story",
                "vn.profile_manifest",
                "vn.policy_bundle_manifest",
                "vn.extension_manifest",
                "vn.standard_command_manifest",
                "vn.presentation_provider_manifest",
                "vn.commercial_baseline_manifest",
                "vn.system_story_manifest",
                "vn.system_ui_profile_manifest",
                "vn.advanced_presentation_manifest",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            release_checks: [
                "runtime_provider.native_vn",
                "vn.commercial_baseline",
                "vn.system_ui_profile",
                "vn.advanced_presentation",
                "player.full_playable",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            output_schemas: [
                (
                    RuntimeOutputDomain::Effect,
                    "astra.vn.runtime_step_effect.v2",
                    2,
                ),
                (
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v2",
                    2,
                ),
                (RuntimeOutputDomain::Audio, "astra.vn.audio_command.v2", 2),
                (RuntimeOutputDomain::Await, "astra.runtime.await_id.v1", 1),
                (
                    RuntimeOutputDomain::Observation,
                    "astra.product.observation.v1",
                    1,
                ),
                (
                    RuntimeOutputDomain::Trace,
                    "astra.vn.runtime_step_trace.v1",
                    1,
                ),
                (RuntimeOutputDomain::Trace, "astra.vn.runtime_state.v1", 1),
                (
                    RuntimeOutputDomain::DirtySaveSection,
                    "astra.runtime.dirty_save_section.v1",
                    1,
                ),
            ]
            .into_iter()
            .map(|(domain, schema, major)| RuntimeOutputSchemaDescriptor {
                domain,
                schema: schema.to_string(),
                version: astra_core::SchemaVersion::new(major, 0, 0),
                codec: RuntimeOutputCodec::Postcard,
            })
            .collect(),
        },
        bindings: bindings.clone(),
    };
    let registry = PluginExtensionRegistrySnapshot {
        schema: PLUGIN_EXTENSION_REGISTRY_SCHEMA.to_string(),
        providers,
        bindings,
        conflicts: vec![],
    };
    (json_bytes(policy), json_bytes(registry))
}

fn default_asset_vfs_manifest(cooked_assets: &[SectionPayload]) -> Vec<u8> {
    json_bytes(serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": {
                "kind": "package_section",
                "section_id": "package.manifest"
            },
            "targets": [],
            "profiles": []
        }],
        "entries": cooked_assets.iter().map(|section| serde_json::json!({
            "vfs_uri": vfs_uri_for_section_id(&section.id),
            "layer_id": "package.base",
            "source": {
                "kind": "package_section",
                "section_id": section.id
            },
            "offset": 0,
            "size": section.payload.len() as u64,
            "hash": astra_core::Hash256::from_sha256(&section.payload).to_string(),
            "codec": codec_name(&section.codec),
            "media_kind": media_kind_for_section(&section.id),
            "diagnostics": []
        })).collect::<Vec<_>>(),
        "whiteouts": []
    }))
}

fn default_asset_catalog(cooked_assets: &[SectionPayload], profile: &str) -> Vec<u8> {
    json_bytes(serde_json::json!({
        "schema": "astra.asset_catalog.v1",
        "assets": cooked_assets.iter().filter(|section| section.id.starts_with("asset.")).map(|section| serde_json::json!({
            "asset_id": asset_id_for_section_id(&section.id),
            "vfs_uri": vfs_uri_for_section_id(&section.id),
            "media_kind": media_kind_for_section(&section.id),
            "tags": [],
            "bundle_id": profile,
            "chunk_id": "base",
            "profiles": [profile]
        })).collect::<Vec<_>>()
    }))
}

fn vfs_uri_for_section_id(section_id: &str) -> String {
    format!("package:/{}", section_id.replace('.', "/"))
}

fn asset_id_for_section_id(section_id: &str) -> String {
    format!(
        "asset:/{}",
        section_id.trim_start_matches("asset.").replace('.', "/")
    )
}

fn codec_name(codec: &crate::SectionCodec) -> &'static str {
    match codec {
        crate::SectionCodec::Postcard => "postcard",
        crate::SectionCodec::Raw => "raw",
        crate::SectionCodec::Zstd => "zstd",
    }
}

fn media_kind_for_section(section_id: &str) -> &'static str {
    if section_id.contains(".voice.") {
        "voice"
    } else if section_id.contains(".audio.") || section_id.contains(".bgm.") {
        "audio"
    } else if section_id.contains(".movie.") || section_id.contains(".video.") {
        "video"
    } else if section_id.starts_with("asset.") {
        "asset"
    } else {
        "data"
    }
}

pub struct PackageBuilder;

impl PackageBuilder {
    pub fn build(request: PackageBuildRequest) -> Result<ContainerBlob, ContainerError> {
        tracing::info!(
            event = "package.build.start",
            profile = %request.profile,
            cooked_asset_count = request.cooked_assets.len(),
            extra_section_count = request.extra_sections.len(),
            "package build started"
        );
        crate::authority::validate_provider_authority(
            &request.package_id,
            &request.profile,
            &request.provider_policy,
            &request.plugin_extension_registry,
            &request.target_manifest,
            &request.asset_vfs_manifest,
        )?;
        let manifest = PackageManifest {
            schema: "astra.package_manifest.v1".to_string(),
            package_id: request.package_id,
            profile: request.profile,
            container_version: CURRENT_CONTAINER_VERSION,
        };
        let manifest_section =
            SectionPayload::postcard("package.manifest", "astra.package_manifest.v1", &manifest)?;
        let mut sections = vec![
            SectionPayload::raw(
                "cook.summary",
                "astra.cook_batch_summary.v1",
                request.cook_summary,
            ),
            SectionPayload::raw(
                "asset.vfs_manifest",
                "astra.asset_vfs_manifest.v1",
                request.asset_vfs_manifest,
            ),
            SectionPayload::raw(
                "asset.catalog",
                "astra.asset_catalog.v1",
                request.asset_catalog,
            ),
            SectionPayload::raw(
                "media.manifest",
                "astra.media_manifest.v1",
                request.media_manifest,
            ),
            SectionPayload::raw(
                "provider.policy",
                PROVIDER_POLICY_SCHEMA,
                request.provider_policy,
            ),
            SectionPayload::raw(
                "plugin.extension_registry",
                PLUGIN_EXTENSION_REGISTRY_SCHEMA,
                request.plugin_extension_registry,
            ),
            SectionPayload::raw(
                "plugin.dependency_graph",
                "astra.plugin_dependency_graph.v1",
                request.plugin_dependency_graph,
            ),
            SectionPayload::raw(
                "module.fingerprint",
                "astra.module_fingerprint.v1",
                request.module_fingerprint,
            ),
            SectionPayload::raw(
                "target.manifest",
                "astra.target_manifest.v2",
                request.target_manifest,
            ),
            SectionPayload::raw(
                "release.summary",
                "astra.release_summary.v1",
                request.release_summary,
            ),
            SectionPayload::raw(
                "scenario.refs",
                "astra.scenario_refs.v2",
                request.scenario_refs,
            ),
            SectionPayload::raw(
                "platform.eligibility",
                "astra.platform_eligibility.v1",
                request.platform_eligibility,
            ),
        ];
        sections.extend(request.cooked_assets);
        sections.extend(request.extra_sections);
        let registry = SchemaRegistryManifest {
            schema: "astra.schema_registry.v2".to_string(),
            schemas: std::iter::once(&manifest_section)
                .chain(sections.iter())
                .map(|section| SchemaRegistryEntry {
                    section_id: section.id.clone(),
                    schema: section.schema.clone(),
                    version: section.version,
                })
                .collect(),
        };
        let registry_section = SectionPayload::raw(
            "schema.registry",
            "astra.schema_registry.v2",
            serde_json::to_vec(&registry)
                .map_err(|error| ContainerError::message(error.to_string()))?,
        );
        let mut builder = AstraContainerBuilder::new(ContainerKind::Package)
            .add_section(manifest_section)
            .add_section(registry_section);
        for section in sections {
            builder = builder.add_section(section);
        }
        match builder.write() {
            Ok(blob) => {
                tracing::info!(
                    event = "package.build.complete",
                    byte_size = blob.as_bytes().len(),
                    "package build completed"
                );
                Ok(blob)
            }
            Err(error) => {
                tracing::error!(
                    event = "package.build.failed",
                    error_kind = "container_write",
                    "package build failed"
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SchemaRegistryManifest {
    pub schema: String,
    pub schemas: Vec<SchemaRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SchemaRegistryEntry {
    pub section_id: String,
    pub schema: String,
    pub version: astra_core::SchemaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CookSummaryManifest {
    pub schema: String,
    pub graph_hash: astra_core::Hash256,
    pub artifact_count: u64,
    pub cache_hit_count: u64,
    pub cooked_count: u64,
    pub max_concurrency: u64,
}

impl CookSummaryManifest {
    pub fn empty() -> Self {
        Self {
            schema: "astra.cook_batch_summary.v1".to_string(),
            graph_hash: astra_core::Hash256::from_sha256(b"astra.cook_graph.v1|empty"),
            artifact_count: 0,
            cache_hit_count: 0,
            cooked_count: 0,
            max_concurrency: 1,
        }
    }

    pub fn validate(&self) -> Result<(), ContainerError> {
        if self.schema != "astra.cook_batch_summary.v1"
            || self.max_concurrency == 0
            || self.cache_hit_count.checked_add(self.cooked_count) != Some(self.artifact_count)
        {
            return Err(ContainerError::message(
                "package cook summary identity is invalid",
            ));
        }
        Ok(())
    }

    fn from_sections(sections: &[SectionPayload]) -> Self {
        let cooked = sections
            .iter()
            .filter(|section| section.schema == "astra.cooked_asset.v1")
            .collect::<Vec<_>>();
        let identity = cooked
            .iter()
            .map(|section| {
                format!(
                    "{}|{}|{}",
                    section.id,
                    section.schema,
                    astra_core::Hash256::from_sha256(&section.payload)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let artifact_count = cooked.len() as u64;
        Self {
            schema: "astra.cook_batch_summary.v1".to_string(),
            graph_hash: if cooked.is_empty() {
                astra_core::Hash256::from_sha256(b"astra.cook_graph.v1|empty")
            } else {
                astra_core::Hash256::from_sha256(identity.as_bytes())
            },
            artifact_count,
            cache_hit_count: 0,
            cooked_count: artifact_count,
            max_concurrency: 1,
        }
    }
}
