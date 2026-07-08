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
    pub schema_registry: Vec<u8>,
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
    pub fn minimal(
        package_id: impl Into<String>,
        profile: impl Into<String>,
        cooked_assets: Vec<SectionPayload>,
    ) -> Self {
        let package_id = package_id.into();
        let profile = profile.into();
        Self {
            package_id: package_id.clone(),
            profile: profile.clone(),
            asset_vfs_manifest: default_asset_vfs_manifest(&cooked_assets),
            asset_catalog: default_asset_catalog(&cooked_assets, &profile),
            cooked_assets,
            schema_registry: json_bytes(serde_json::json!({
                "schema": "astra.schema_registry.v1",
                "schemas": []
            })),
            media_manifest: json_bytes(serde_json::json!({
                "schema": "astra.media_manifest.v1",
                "codecs": ["png", "jpeg", "webp", "wav", "ogg", "flac", "mp3"],
                "ffmpeg": "optional"
            })),
            provider_policy: json_bytes(serde_json::json!({
                "schema": "astra.provider_policy.v1",
                "profile": profile,
                "renderer": "headless",
                "decode_fallback": "profile_bound",
                "bindings": [{
                    "slot": "presentation",
                    "provider_id": "astra.fixture.headless_presentation"
                }]
            })),
            plugin_extension_registry: json_bytes(serde_json::json!({
                "schema": "astra.plugin_extension_registry.v1",
                "providers": [{
                    "slot": "presentation",
                    "provider_id": "astra.fixture.headless_presentation",
                    "capability": "presentation.headless",
                    "phase": "runtime",
                    "packaged": true
                }, {
                    "slot": "vfs_provider",
                    "provider_id": "astra.vfs.package",
                    "capability": "vfs.backend.package",
                    "phase": "runtime",
                    "packaged": true
                }],
                "bindings": [{
                    "slot": "presentation",
                    "provider_id": "astra.fixture.headless_presentation"
                }],
                "conflicts": []
            })),
            plugin_dependency_graph: json_bytes(serde_json::json!({
                "schema": "astra.plugin_dependency_graph.v1",
                "dependencies": []
            })),
            module_fingerprint: json_bytes(serde_json::json!({
                "schema": "astra.module_fingerprint.v1",
                "modules": []
            })),
            target_manifest: json_bytes(serde_json::json!({
                "schema": "astra.target_manifest.v1",
                "targets": [{
                    "id": "native-smoke-game",
                    "kind": "game",
                    "crate": "astra-runtime",
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
                "schema": "astra.scenario_refs.v1",
                "scenarios": ["scenarios/native_smoke.yaml"]
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

fn json_bytes(value: serde_json::Value) -> Vec<u8> {
    value.to_string().into_bytes()
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
            "capabilities": ["package.read"]
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
        let manifest = PackageManifest {
            schema: "astra.package_manifest.v1".to_string(),
            package_id: request.package_id,
            profile: request.profile,
            container_version: CURRENT_CONTAINER_VERSION,
        };
        let mut builder = AstraContainerBuilder::new(ContainerKind::Package)
            .add_section(SectionPayload::postcard(
                "package.manifest",
                "astra.package_manifest.v1",
                &manifest,
            )?)
            .add_section(SectionPayload::raw(
                "schema.registry",
                "astra.schema_registry.v1",
                request.schema_registry,
            ))
            .add_section(SectionPayload::raw(
                "asset.vfs_manifest",
                "astra.asset_vfs_manifest.v1",
                request.asset_vfs_manifest,
            ))
            .add_section(SectionPayload::raw(
                "asset.catalog",
                "astra.asset_catalog.v1",
                request.asset_catalog,
            ))
            .add_section(SectionPayload::raw(
                "media.manifest",
                "astra.media_manifest.v1",
                request.media_manifest,
            ))
            .add_section(SectionPayload::raw(
                "provider.policy",
                "astra.provider_policy.v1",
                request.provider_policy,
            ))
            .add_section(SectionPayload::raw(
                "plugin.extension_registry",
                "astra.plugin_extension_registry.v1",
                request.plugin_extension_registry,
            ))
            .add_section(SectionPayload::raw(
                "plugin.dependency_graph",
                "astra.plugin_dependency_graph.v1",
                request.plugin_dependency_graph,
            ))
            .add_section(SectionPayload::raw(
                "module.fingerprint",
                "astra.module_fingerprint.v1",
                request.module_fingerprint,
            ))
            .add_section(SectionPayload::raw(
                "target.manifest",
                "astra.target_manifest.v1",
                request.target_manifest,
            ))
            .add_section(SectionPayload::raw(
                "release.summary",
                "astra.release_summary.v1",
                request.release_summary,
            ))
            .add_section(SectionPayload::raw(
                "scenario.refs",
                "astra.scenario_refs.v1",
                request.scenario_refs,
            ))
            .add_section(SectionPayload::raw(
                "platform.eligibility",
                "astra.platform_eligibility.v1",
                request.platform_eligibility,
            ));
        for section in request.cooked_assets {
            builder = builder.add_section(section);
        }
        for section in request.extra_sections {
            builder = builder.add_section(section);
        }
        builder.write()
    }
}
