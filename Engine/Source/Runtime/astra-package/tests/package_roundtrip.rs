use astra_core::SchemaVersion;
use astra_package::{
    section_aad_hash, AstraContainerBuilder, AstraContainerReader, ContainerKind,
    EncryptionDescriptor, ExternalKeyRef, MigrationPolicy, PackageBuildRequest, PackageBuilder,
    PackageReader, SectionCodec, SectionPayload,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FixturePayload {
    name: String,
    value: u32,
}

#[test]
fn package_roundtrip_verifies_hash_bounds_and_schema_registry() {
    let payload = FixturePayload {
        name: "stage2".to_string(),
        value: 42,
    };
    let encoded = postcard::to_allocvec(&payload).unwrap();
    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(SectionPayload::new(
            "schema.registry",
            "astra.schema_registry.v1",
            SchemaVersion::new(1, 0, 0),
            SectionCodec::Postcard,
            encoded,
            MigrationPolicy::current(),
        ))
        .write()
        .unwrap();

    let reader = AstraContainerReader::new(blob.as_bytes()).unwrap();
    let decoded: FixturePayload = reader.decode_postcard("schema.registry").unwrap();
    assert_eq!(decoded, payload);
    assert_eq!(reader.entries().len(), 1);

    let mut corrupted = blob.into_bytes();
    let middle = corrupted.len() / 2;
    corrupted[middle] ^= 0x01;
    assert!(AstraContainerReader::new(&corrupted).is_err());
}

#[test]
fn container_builder_rejects_duplicate_and_invalid_section_authority() {
    let section = || {
        SectionPayload::raw(
            "schema.registry",
            "astra.schema_registry.v1",
            b"registry".to_vec(),
        )
    };
    let duplicate = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(section())
        .add_section(section())
        .write()
        .unwrap_err();
    assert!(duplicate.to_string().contains("duplicate section id"));

    for invalid in [
        SectionPayload::raw("", "astra.schema_registry.v1", vec![]),
        SectionPayload::raw("schema.registry", "", vec![]),
        SectionPayload::raw("schema/registry", "astra.schema_registry.v1", vec![]),
    ] {
        assert!(AstraContainerBuilder::new(ContainerKind::Package)
            .add_section(invalid)
            .write()
            .is_err());
    }

    let invalid_migration = SectionPayload::new(
        "schema.registry",
        "astra.schema_registry.v1",
        SchemaVersion::new(1, 0, 0),
        SectionCodec::Raw,
        vec![],
        MigrationPolicy {
            minimum_supported_version: SchemaVersion::new(2, 0, 0),
            current_version: SchemaVersion::new(1, 0, 0),
        },
    );
    assert!(AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(invalid_migration)
        .write()
        .is_err());
}

#[test]
fn package_roundtrip_zstd_codec_roundtrips_and_encryption_descriptor_requires_provider() {
    let section = SectionPayload::new(
        "media.manifest",
        "astra.media_manifest.v1",
        SchemaVersion::new(1, 0, 0),
        SectionCodec::Zstd,
        b"{\"codecs\":[\"png\"]}".to_vec(),
        MigrationPolicy::current(),
    );
    let descriptor = EncryptionDescriptor {
        provider_id: "com.example.crypto".to_string(),
        method: "aes-256-gcm".to_string(),
        key_ref: ExternalKeyRef {
            uri: "keyref://release/test".to_string(),
        },
        aad_hash: section_aad_hash(ContainerKind::Package, &section).unwrap(),
    };
    let section = section.with_encryption_descriptor(descriptor);
    assert!(AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(section.clone())
        .write()
        .is_err());

    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(section)
        .write_with_crypto(&XorCryptoProvider)
        .unwrap();

    let reader = AstraContainerReader::new(blob.as_bytes()).unwrap();
    assert_eq!(
        reader.section_entry("media.manifest").unwrap().codec,
        SectionCodec::Zstd
    );
    assert!(reader.read_section("media.manifest").is_err());
    assert!(reader
        .read_section_with_crypto("media.manifest", &astra_package::NoCryptoProvider)
        .is_err());
    assert_eq!(
        reader
            .read_section_with_crypto("media.manifest", &XorCryptoProvider)
            .unwrap(),
        b"{\"codecs\":[\"png\"]}".to_vec()
    );
}

struct XorCryptoProvider;

impl astra_package::ContainerCryptoProvider for XorCryptoProvider {
    fn provider_id(&self) -> &str {
        "com.example.crypto"
    }

    fn decrypt(
        &self,
        _descriptor: &EncryptionDescriptor,
        stored_payload: &[u8],
    ) -> Result<Vec<u8>, astra_package::ContainerError> {
        Ok(stored_payload.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn encrypt(
        &self,
        _descriptor: &EncryptionDescriptor,
        plain_payload: &[u8],
    ) -> Result<Vec<u8>, astra_package::ContainerError> {
        Ok(plain_payload.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

#[test]
fn package_roundtrip_builder_writes_required_runtime_sections() {
    let request = PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    let blob = PackageBuilder::build(request).unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();

    for section in [
        "package.manifest",
        "schema.registry",
        "asset.vfs_manifest",
        "asset.catalog",
        "media.manifest",
        "provider.policy",
        "plugin.extension_registry",
        "plugin.dependency_graph",
        "module.fingerprint",
        "target.manifest",
        "release.summary",
        "scenario.refs",
        "platform.eligibility",
        "asset.characters.hero",
    ] {
        assert!(package.has_section(section), "missing {section}");
    }
    assert!(!package.has_section("asset.registry"));
    let policy = package.container().read_section("provider.policy").unwrap();
    let policy: serde_json::Value = serde_json::from_slice(&policy).unwrap();
    assert_eq!(policy["profile"], "desktop-release");

    let registry = package
        .container()
        .read_section("plugin.extension_registry")
        .unwrap();
    let registry: serde_json::Value = serde_json::from_slice(&registry).unwrap();
    assert_eq!(registry["schema"], "astra.plugin_extension_registry.v1");
    assert_eq!(registry["bindings"][0]["slot"], "presentation");

    let dependency_graph = package
        .container()
        .read_section("plugin.dependency_graph")
        .unwrap();
    let dependency_graph: serde_json::Value = serde_json::from_slice(&dependency_graph).unwrap();
    assert_eq!(
        dependency_graph["schema"],
        "astra.plugin_dependency_graph.v1"
    );
}
