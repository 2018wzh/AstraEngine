use astra_core::Hash256;
use astra_package::{
    section_aad_hash, validate_source_locked_container, AstraContainerBuilder,
    AstraContainerReader, AuthorizedSourceReader, ContainerCryptoProvider, ContainerError,
    ContainerKind, EncryptionDescriptor, ExternalKeyRef, PackageBuildRequest, PackageBuilder,
    PackageReader, SectionPayload, SourceFingerprintCryptoProvider, SourceUnlockPolicy,
    SourceVerificationEntry, SourceVerificationManifest, SOURCE_FINGERPRINT_METHOD,
    SOURCE_FINGERPRINT_PROVIDER_ID,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

struct MemorySource(BTreeMap<String, Vec<u8>>);

impl AuthorizedSourceReader for MemorySource {
    fn stat_relative(&mut self, relative_path: &str) -> Result<u64, ContainerError> {
        self.0
            .get(relative_path)
            .map(|bytes| bytes.len() as u64)
            .ok_or_else(|| ContainerError::Crypto("source file is missing".into()))
    }

    fn read_relative(
        &mut self,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        let bytes = self
            .0
            .get(relative_path)
            .ok_or_else(|| ContainerError::Crypto("source file is missing".into()))?;
        if bytes.len() as u64 > max_bytes {
            return Err(ContainerError::Crypto("source read exceeds bound".into()));
        }
        Ok(bytes.clone())
    }
}

fn fixture() -> (SourceUnlockPolicy, SourceVerificationManifest, MemorySource) {
    let bytes = b"verified original bytes".to_vec();
    let mut manifest = SourceVerificationManifest {
        schema: "astra.source_verification_manifest.v1".into(),
        profile_id: "fixture.original.v1".into(),
        manifest_hash: Hash256::from_bytes([0; 32]),
        entries: vec![SourceVerificationEntry {
            relative_path: "GAME.DAT".into(),
            byte_length: bytes.len() as u64,
            sha256: Hash256::from_sha256(&bytes),
        }],
    };
    manifest.manifest_hash = manifest.computed_hash();
    (
        SourceUnlockPolicy {
            schema: "astra.source_unlock_policy.v1".into(),
            source_profile: "fixture.original.v1".into(),
            verification_manifest_hash: manifest.manifest_hash,
            crypto_provider: SOURCE_FINGERPRINT_PROVIDER_ID.into(),
            protected_sections: BTreeSet::from(["story.main".into()]),
            max_files: 1,
            max_file_bytes: 1024,
            max_total_bytes: 1024,
        },
        manifest,
        MemorySource(BTreeMap::from([("GAME.DAT".into(), bytes)])),
    )
}

#[test]
fn source_fingerprint_provider_roundtrips_and_authenticates() {
    let (policy, manifest, mut source) = fixture();
    let provider = SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source)
        .expect("unlock verified source");
    let descriptor = EncryptionDescriptor {
        provider_id: SOURCE_FINGERPRINT_PROVIDER_ID.into(),
        method: SOURCE_FINGERPRINT_METHOD.into(),
        key_ref: ExternalKeyRef {
            uri: "source-fingerprint://fixture.original.v1".into(),
        },
        aad_hash: Hash256::from_sha256(b"section metadata"),
    };
    let stored = provider
        .encrypt(&descriptor, b"commercial payload")
        .unwrap();
    assert!(!stored
        .windows(18)
        .any(|window| window == b"commercial payload"));
    assert_eq!(
        provider.decrypt(&descriptor, &stored).unwrap(),
        b"commercial payload"
    );
    let mut tampered = stored;
    *tampered.last_mut().unwrap() ^= 1;
    assert!(provider.decrypt(&descriptor, &tampered).is_err());
}

#[test]
fn modified_or_unsafe_source_fails_before_key_creation() {
    let (policy, mut manifest, mut source) = fixture();
    source.0.get_mut("GAME.DAT").unwrap()[0] ^= 1;
    assert!(SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).is_err());

    let (_, _, mut source) = fixture();
    manifest.entries[0].relative_path = "../GAME.DAT".into();
    assert!(SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).is_err());
}

#[test]
fn source_locked_container_requires_plain_bootstrap_and_encrypted_payload() {
    let (policy, manifest, mut source) = fixture();
    let provider =
        SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).unwrap();
    let bootstrap =
        SectionPayload::postcard("source.unlock", "astra.source_unlock_policy.v1", &policy)
            .unwrap();
    let mut story = SectionPayload::raw("story.main", "fixture.story", b"secret".to_vec());
    let aad_hash = section_aad_hash(ContainerKind::Package, &story).unwrap();
    story = story.with_encryption_descriptor(EncryptionDescriptor {
        provider_id: SOURCE_FINGERPRINT_PROVIDER_ID.into(),
        method: SOURCE_FINGERPRINT_METHOD.into(),
        key_ref: ExternalKeyRef {
            uri: "source-fingerprint://fixture.original.v1".into(),
        },
        aad_hash,
    });
    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(bootstrap)
        .add_section(story)
        .write_with_crypto(&provider)
        .unwrap();
    let reader = AstraContainerReader::new(blob.as_bytes()).unwrap();
    validate_source_locked_container(&reader, &policy, "source.unlock").unwrap();
    assert_eq!(
        reader
            .read_section_with_crypto("story.main", &provider)
            .unwrap(),
        b"secret"
    );
    let reader = AstraContainerReader::new(blob.as_bytes())
        .unwrap()
        .with_crypto_provider(Arc::new(provider));
    assert_eq!(reader.read_section("story.main").unwrap(), b"secret");
}

#[test]
fn package_builder_and_reader_use_source_crypto_on_product_sections() {
    let (policy, manifest, mut source) = fixture();
    let provider =
        SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).unwrap();
    let mut story = SectionPayload::raw("story.main", "fixture.story", b"secret".to_vec());
    let aad_hash = section_aad_hash(ContainerKind::Package, &story).unwrap();
    story = story.with_encryption_descriptor(EncryptionDescriptor {
        provider_id: SOURCE_FINGERPRINT_PROVIDER_ID.into(),
        method: SOURCE_FINGERPRINT_METHOD.into(),
        key_ref: ExternalKeyRef {
            uri: "source-fingerprint://fixture.original.v1".into(),
        },
        aad_hash,
    });
    let bootstrap =
        SectionPayload::postcard("source.unlock", "astra.source_unlock_policy.v1", &policy)
            .unwrap();
    let request = PackageBuildRequest::fixture(
        "com.example.source-locked",
        "classic",
        vec![bootstrap, story],
    );
    let blob = PackageBuilder::build_with_crypto(request, &provider).unwrap();
    assert!(!blob.as_bytes().windows(6).any(|window| window == b"secret"));
    let package = PackageReader::open_source_locked(
        blob.as_bytes(),
        &policy,
        "source.unlock",
        Arc::new(provider),
    )
    .unwrap();
    assert_eq!(
        package.container().read_section("story.main").unwrap(),
        b"secret"
    );
}

#[test]
fn package_builder_applies_source_policy_to_named_sections() {
    let (policy, manifest, mut source) = fixture();
    let provider =
        SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).unwrap();
    let mut request = PackageBuildRequest::fixture(
        "com.example.policy-owned",
        "classic",
        vec![SectionPayload::raw(
            "story.main",
            "fixture.story",
            b"commercial payload marker".to_vec(),
        )],
    );
    request.source_unlock_policy = Some(policy.clone());
    let blob = PackageBuilder::build_with_crypto(request, &provider).unwrap();
    assert!(!blob
        .as_bytes()
        .windows(b"commercial payload marker".len())
        .any(|window| window == b"commercial payload marker"));
    let raw = AstraContainerReader::new(blob.as_bytes()).unwrap();
    validate_source_locked_container(&raw, &policy, "source.unlock").unwrap();
    let package = PackageReader::open_source_locked(
        blob.as_bytes(),
        &policy,
        "source.unlock",
        Arc::new(provider),
    )
    .unwrap();
    assert_eq!(
        package.container().read_section("story.main").unwrap(),
        b"commercial payload marker"
    );
}

#[test]
fn source_locked_package_does_not_expose_common_payload_signatures() {
    let (mut policy, manifest, mut source) = fixture();
    policy.protected_sections = BTreeSet::from([
        "story.main".into(),
        "asset.image".into(),
        "asset.audio".into(),
    ]);
    let provider =
        SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).unwrap();
    let mut request = PackageBuildRequest::fixture(
        "com.example.signature-scan",
        "classic",
        vec![
            SectionPayload::raw("story.main", "fixture.story", b"known dialogue".to_vec()),
            SectionPayload::raw(
                "asset.image",
                "fixture.image",
                b"\x89PNG\r\n\x1a\nprivate-image".to_vec(),
            ),
            SectionPayload::raw(
                "asset.audio",
                "fixture.audio",
                b"RIFFprivate-waveWAVE".to_vec(),
            ),
        ],
    );
    request.source_unlock_policy = Some(policy);
    let blob = PackageBuilder::build_with_crypto(request, &provider).unwrap();
    for marker in [
        b"known dialogue".as_slice(),
        b"\x89PNG\r\n\x1a\n".as_slice(),
        b"RIFFprivate-waveWAVE".as_slice(),
    ] {
        assert!(!blob
            .as_bytes()
            .windows(marker.len())
            .any(|window| window == marker));
    }
}

#[test]
fn package_builder_rejects_missing_protected_section() {
    let (mut policy, manifest, mut source) = fixture();
    policy.protected_sections = BTreeSet::from(["story.missing".into()]);
    let provider =
        SourceFingerprintCryptoProvider::unlock(&policy, &manifest, &mut source).unwrap();
    let mut request = PackageBuildRequest::fixture("com.example.missing", "classic", vec![]);
    request.source_unlock_policy = Some(policy);
    let error = PackageBuilder::build_with_crypto(request, &provider).unwrap_err();
    assert!(error.to_string().contains("missing protected sections"));
}
