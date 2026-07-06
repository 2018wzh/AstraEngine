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
    pub asset_registry: Vec<u8>,
    pub media_manifest: Vec<u8>,
    pub provider_policy: Vec<u8>,
    pub module_fingerprint: Vec<u8>,
    pub target_manifest: Vec<u8>,
    pub release_summary: Vec<u8>,
    pub scenario_refs: Vec<u8>,
    pub platform_eligibility: Vec<u8>,
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
            cooked_assets,
            schema_registry: json_bytes(serde_json::json!({
                "schema": "astra.schema_registry.v1",
                "schemas": []
            })),
            asset_registry: json_bytes(serde_json::json!({
                "schema": "astra.asset_registry.v1",
                "package_id": package_id,
                "assets": []
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
                "decode_fallback": "profile_bound"
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
        }
    }
}

fn json_bytes(value: serde_json::Value) -> Vec<u8> {
    value.to_string().into_bytes()
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
                "asset.registry",
                "astra.asset_registry.v1",
                request.asset_registry,
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
        builder.write()
    }
}
