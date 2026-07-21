use crate::{AstraContainerReader, ContainerCryptoProvider, ContainerError, EncryptionDescriptor};
use aes_gcm_siv::{
    aead::{Aead, KeyInit, Payload},
    Aes256GcmSiv, Nonce,
};
use astra_core::Hash256;
use hkdf::Hkdf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::collections::BTreeSet;
use zeroize::{Zeroize, Zeroizing};

pub const SOURCE_FINGERPRINT_PROVIDER_ID: &str = "astra.crypto.source_fingerprint.v1";
pub const SOURCE_FINGERPRINT_METHOD: &str = "aes-256-gcm-siv+hkdf-sha256";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceUnlockPolicy {
    pub schema: String,
    pub source_profile: String,
    pub verification_manifest_hash: Hash256,
    pub crypto_provider: String,
    pub protected_sections: BTreeSet<String>,
    pub max_files: u32,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceVerificationEntry {
    pub relative_path: String,
    pub byte_length: u64,
    pub sha256: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceVerificationManifest {
    pub schema: String,
    pub profile_id: String,
    pub manifest_hash: Hash256,
    pub entries: Vec<SourceVerificationEntry>,
}

pub trait AuthorizedSourceReader {
    fn stat_relative(&mut self, relative_path: &str) -> Result<u64, ContainerError>;

    fn read_relative_range(
        &mut self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError>;
}

const SOURCE_FINGERPRINT_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

pub struct SourceFingerprintCryptoProvider {
    key: Zeroizing<[u8; 32]>,
    key_uri: String,
}

impl SourceUnlockPolicy {
    pub fn validate(&self) -> Result<(), ContainerError> {
        if self.schema != "astra.source_unlock_policy.v1"
            || self.crypto_provider != SOURCE_FINGERPRINT_PROVIDER_ID
            || !safe_symbol(&self.source_profile)
            || self.protected_sections.is_empty()
            || self.max_files == 0
            || self.max_file_bytes == 0
            || self.max_total_bytes < self.max_file_bytes
        {
            return Err(ContainerError::Crypto(
                "source unlock policy is invalid or unsupported".into(),
            ));
        }
        if self.protected_sections.iter().any(|id| !safe_symbol(id)) {
            return Err(ContainerError::Crypto(
                "source unlock policy contains an unsafe section id".into(),
            ));
        }
        Ok(())
    }
}

impl SourceVerificationManifest {
    pub fn computed_hash(&self) -> Hash256 {
        let mut hasher = Sha256::new();
        hasher.update(b"astra.source_verification_manifest.v1\0");
        hasher.update((self.profile_id.len() as u64).to_le_bytes());
        hasher.update(self.profile_id.as_bytes());
        for entry in &self.entries {
            hasher.update((entry.relative_path.len() as u64).to_le_bytes());
            hasher.update(entry.relative_path.as_bytes());
            hasher.update(entry.byte_length.to_le_bytes());
            hasher.update(entry.sha256.as_bytes());
        }
        Hash256::from_bytes(hasher.finalize().into())
    }
}

pub fn validate_source_locked_container(
    reader: &AstraContainerReader,
    policy: &SourceUnlockPolicy,
    bootstrap_section_id: &str,
) -> Result<(), ContainerError> {
    policy.validate()?;
    let mut protected = BTreeSet::new();
    let mut bootstrap_found = false;
    for entry in reader.entries() {
        if entry.id == bootstrap_section_id {
            bootstrap_found = true;
            if entry.encryption.is_some() {
                return Err(ContainerError::Crypto(
                    "source unlock bootstrap section must remain plaintext".into(),
                ));
            }
        }
        if policy.protected_sections.contains(&entry.id) {
            let descriptor = entry.encryption.as_ref().ok_or_else(|| {
                ContainerError::Crypto(format!("protected section {} is plaintext", entry.id))
            })?;
            if descriptor.provider_id != SOURCE_FINGERPRINT_PROVIDER_ID
                || descriptor.method != SOURCE_FINGERPRINT_METHOD
            {
                return Err(ContainerError::Crypto(format!(
                    "protected section {} uses the wrong crypto provider",
                    entry.id
                )));
            }
            protected.insert(entry.id.as_str());
        }
    }
    if !bootstrap_found || protected.len() != policy.protected_sections.len() {
        return Err(ContainerError::Crypto(
            "source locked container is missing bootstrap or protected sections".into(),
        ));
    }
    Ok(())
}

impl SourceFingerprintCryptoProvider {
    pub fn unlock(
        policy: &SourceUnlockPolicy,
        manifest: &SourceVerificationManifest,
        reader: &mut dyn AuthorizedSourceReader,
    ) -> Result<Self, ContainerError> {
        policy.validate()?;
        if manifest.schema != "astra.source_verification_manifest.v1"
            || manifest.profile_id != policy.source_profile
            || manifest.manifest_hash != policy.verification_manifest_hash
            || manifest.computed_hash() != manifest.manifest_hash
            || manifest.entries.is_empty()
            || manifest.entries.len() > policy.max_files as usize
        {
            return Err(ContainerError::Crypto(
                "source verification manifest does not match unlock policy".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut total = 0_u64;
        let mut fingerprint = Sha512::new();
        fingerprint.update(b"astra.source_fingerprint.v1\0");
        fingerprint.update(manifest.profile_id.as_bytes());
        for entry in &manifest.entries {
            if !safe_relative_path(&entry.relative_path)
                || !seen.insert(entry.relative_path.as_str())
                || entry.byte_length == 0
                || entry.byte_length > policy.max_file_bytes
            {
                return Err(ContainerError::Crypto(
                    "source verification entry is invalid".into(),
                ));
            }
            total = total.checked_add(entry.byte_length).ok_or_else(|| {
                ContainerError::Crypto("source verification byte budget overflow".into())
            })?;
            if total > policy.max_total_bytes {
                return Err(ContainerError::Crypto(
                    "source verification byte budget exceeded".into(),
                ));
            }
            if reader.stat_relative(&entry.relative_path)? != entry.byte_length {
                return Err(ContainerError::Crypto("source file length mismatch".into()));
            }
            let mut file_hash = Sha256::new();
            fingerprint.update((entry.relative_path.len() as u64).to_le_bytes());
            fingerprint.update(entry.relative_path.as_bytes());
            fingerprint.update(entry.byte_length.to_le_bytes());
            let mut offset = 0_u64;
            while offset < entry.byte_length {
                let length = (entry.byte_length - offset).min(SOURCE_FINGERPRINT_CHUNK_BYTES);
                let mut bytes = Zeroizing::new(reader.read_relative_range(
                    &entry.relative_path,
                    offset,
                    length,
                    SOURCE_FINGERPRINT_CHUNK_BYTES,
                )?);
                if bytes.len() as u64 != length {
                    bytes.zeroize();
                    return Err(ContainerError::Crypto(
                        "source file range length mismatch".into(),
                    ));
                }
                file_hash.update(bytes.as_slice());
                fingerprint.update(bytes.as_slice());
                bytes.zeroize();
                offset = offset.checked_add(length).ok_or_else(|| {
                    ContainerError::Crypto("source fingerprint offset overflow".into())
                })?;
            }
            if Hash256::from_bytes(file_hash.finalize().into()) != entry.sha256 {
                return Err(ContainerError::Crypto(
                    "source file fingerprint mismatch".into(),
                ));
            }
            if reader.stat_relative(&entry.relative_path)? != entry.byte_length {
                return Err(ContainerError::Crypto(
                    "source file changed during fingerprinting".into(),
                ));
            }
        }
        let digest = fingerprint.finalize();
        let mut source_material = Zeroizing::new([0_u8; 64]);
        source_material.copy_from_slice(&digest);
        let hkdf = Hkdf::<Sha256>::new(
            Some(policy.verification_manifest_hash.as_bytes()),
            source_material.as_slice(),
        );
        let mut key = Zeroizing::new([0_u8; 32]);
        hkdf.expand(
            format!("astra.section-key.v1\0{}", policy.source_profile).as_bytes(),
            key.as_mut(),
        )
        .map_err(|_| ContainerError::Crypto("source key derivation failed".into()))?;
        Ok(Self {
            key,
            key_uri: format!("source-fingerprint://{}", policy.source_profile),
        })
    }

    fn validate_descriptor(&self, descriptor: &EncryptionDescriptor) -> Result<(), ContainerError> {
        if descriptor.provider_id != SOURCE_FINGERPRINT_PROVIDER_ID
            || descriptor.method != SOURCE_FINGERPRINT_METHOD
            || descriptor.key_ref.uri != self.key_uri
        {
            return Err(ContainerError::Crypto(
                "source fingerprint encryption descriptor mismatch".into(),
            ));
        }
        Ok(())
    }
}

impl ContainerCryptoProvider for SourceFingerprintCryptoProvider {
    fn provider_id(&self) -> &str {
        SOURCE_FINGERPRINT_PROVIDER_ID
    }

    fn decrypt(
        &self,
        descriptor: &EncryptionDescriptor,
        stored_payload: &[u8],
    ) -> Result<Vec<u8>, ContainerError> {
        self.validate_descriptor(descriptor)?;
        let (nonce, ciphertext) = stored_payload.split_at_checked(12).ok_or_else(|| {
            ContainerError::Crypto("encrypted section payload is truncated".into())
        })?;
        let cipher = Aes256GcmSiv::new_from_slice(self.key.as_slice())
            .map_err(|_| ContainerError::Crypto("invalid source section key".into()))?;
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: descriptor.aad_hash.as_bytes(),
                },
            )
            .map_err(|_| ContainerError::Crypto("encrypted section authentication failed".into()))
    }

    fn encrypt(
        &self,
        descriptor: &EncryptionDescriptor,
        plain_payload: &[u8],
    ) -> Result<Vec<u8>, ContainerError> {
        self.validate_descriptor(descriptor)?;
        let mut nonce_hasher = Sha256::new();
        nonce_hasher.update(b"astra.gcm-siv.nonce.v1\0");
        nonce_hasher.update(self.key.as_slice());
        nonce_hasher.update(descriptor.aad_hash.as_bytes());
        nonce_hasher.update(Hash256::from_sha256(plain_payload).as_bytes());
        let nonce_digest = nonce_hasher.finalize();
        let nonce = &nonce_digest[..12];
        let cipher = Aes256GcmSiv::new_from_slice(self.key.as_slice())
            .map_err(|_| ContainerError::Crypto("invalid source section key".into()))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: plain_payload,
                    aad: descriptor.aad_hash.as_bytes(),
                },
            )
            .map_err(|_| ContainerError::Crypto("section encryption failed".into()))?;
        let mut stored = Vec::with_capacity(12 + ciphertext.len());
        stored.extend_from_slice(nonce);
        stored.extend_from_slice(&ciphertext);
        Ok(stored)
    }
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains(':')
        && value
            .split(['/', '\\'])
            .all(|part| safe_symbol(part) && part != "." && part != "..")
}
