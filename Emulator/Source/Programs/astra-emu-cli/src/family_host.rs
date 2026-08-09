use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use astra_core::Hash256;
use astra_emu_family_api::{
    LegacyPrivateMaterialHostV8, LegacyPrivateMaterialRequestV8, LegacyProviderError,
    LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacySecretBufferV8, LegacyVfsReader,
    LEGACY_FAMILY_ABI_FINGERPRINT,
};
use astra_emu_manager_core::{
    DynamicFamilyHostServicesV8, DynamicFamilyLoader, Ed25519FamilySignatureVerifier,
    FamilyPluginGate, FamilyPluginManifest,
};
use serde::Deserialize;
use zeroize::Zeroizing;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PRIVATE_MATERIAL_FILE_BYTES: u64 = 64 * 1024;

pub struct CliPrivateMaterialHost {
    secret_id: String,
    bytes: Zeroizing<Vec<u8>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SiglusKeyFile {
    key: Vec<u8>,
}

impl CliPrivateMaterialHost {
    pub fn load_siglus(
        game_root: &Path,
        path: &Path,
        secret_id: &str,
    ) -> Result<Arc<dyn LegacyPrivateMaterialHostV8>, String> {
        let path = fs::canonicalize(path).map_err(|_| "ASTRA_EMU_PRIVATE_MATERIAL_READ")?;
        if !path.is_file() || !path.starts_with(game_root) {
            return Err("ASTRA_EMU_PRIVATE_MATERIAL_SCOPE".into());
        }
        let metadata = fs::metadata(&path).map_err(|_| "ASTRA_EMU_PRIVATE_MATERIAL_READ")?;
        if metadata.len() == 0 || metadata.len() > MAX_PRIVATE_MATERIAL_FILE_BYTES {
            return Err("ASTRA_EMU_PRIVATE_MATERIAL_BOUNDS".into());
        }
        let encoded = fs::read(path).map_err(|_| "ASTRA_EMU_PRIVATE_MATERIAL_READ")?;
        let encoded =
            std::str::from_utf8(&encoded).map_err(|_| "ASTRA_EMU_PRIVATE_MATERIAL_ENCODING")?;
        let parsed: SiglusKeyFile =
            toml::from_str(encoded).map_err(|_| "ASTRA_EMU_PRIVATE_MATERIAL_PARSE")?;
        if parsed.key.len() != 16 {
            return Err("ASTRA_EMU_PRIVATE_MATERIAL_LENGTH".into());
        }
        Ok(Arc::new(Self {
            secret_id: secret_id.into(),
            bytes: Zeroizing::new(parsed.key),
        }))
    }
}

impl LegacyPrivateMaterialHostV8 for CliPrivateMaterialHost {
    fn read_private_material(
        &self,
        _session: &LegacyRuntimeSessionId,
        request: LegacyPrivateMaterialRequestV8,
    ) -> Result<LegacySecretBufferV8, LegacyProviderError> {
        if request.secret_id != self.secret_id || request.exact_len as usize != self.bytes.len() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_PRIVATE_MATERIAL_REQUEST",
                "private material request does not match the authorized binding",
            ));
        }
        Ok(LegacySecretBufferV8::new(self.bytes.as_slice().to_vec()))
    }
}

pub struct CliFamilyHostConfig {
    family_id: String,
    manifest_path: PathBuf,
    library_path: PathBuf,
}

impl CliFamilyHostConfig {
    pub fn installed_for_executable(executable: &Path, family_id: &str) -> Result<Self, String> {
        validate_family_id(family_id)?;
        let install_root = executable.parent().ok_or("ASTRA_EMU_INSTALL_ROOT")?;
        let family_root = install_root.join("families").join(family_id);
        Ok(Self {
            family_id: family_id.into(),
            manifest_path: family_root.join("manifest.json"),
            library_path: family_root.join(platform_library_name(family_id)?),
        })
    }

    pub fn with_paths(
        family_id: &str,
        manifest_path: PathBuf,
        library_path: PathBuf,
    ) -> Result<Self, String> {
        validate_family_id(family_id)?;
        Ok(Self {
            family_id: family_id.into(),
            manifest_path,
            library_path,
        })
    }

    pub fn create_provider(
        &self,
        vfs: Arc<dyn LegacyVfsReader>,
    ) -> Result<Box<dyn LegacyRuntimeProvider>, String> {
        self.create_provider_with_identity(vfs)
            .map(|(provider, _)| provider)
    }

    pub fn create_provider_with_identity(
        &self,
        vfs: Arc<dyn LegacyVfsReader>,
    ) -> Result<(Box<dyn LegacyRuntimeProvider>, Hash256), String> {
        self.create_provider_with_services(vfs, None)
    }

    pub fn create_provider_with_services(
        &self,
        vfs: Arc<dyn LegacyVfsReader>,
        private_material: Option<Arc<dyn LegacyPrivateMaterialHostV8>>,
    ) -> Result<(Box<dyn LegacyRuntimeProvider>, Hash256), String> {
        let metadata =
            fs::metadata(&self.manifest_path).map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_READ")?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
            return Err("ASTRA_EMU_FAMILY_MANIFEST_BOUNDS".into());
        }
        let manifest: FamilyPluginManifest = serde_json::from_slice(
            &fs::read(&self.manifest_path).map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_READ")?,
        )
        .map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_PARSE")?;
        let signer = env!("ASTRA_EMU_FAMILY_SIGNER_ID");
        let public_key = env!("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX");
        if signer.is_empty() || public_key.is_empty() {
            return Err("ASTRA_EMU_FAMILY_TRUST_ROOT_NOT_PROVISIONED".into());
        }
        let public_key: [u8; 32] = hex::decode(public_key)
            .map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_ENCODING")?
            .try_into()
            .map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_LENGTH")?;
        let verifier = Ed25519FamilySignatureVerifier::new([(signer.to_owned(), public_key)])
            .map_err(|error| error.to_string())?;
        let loader = DynamicFamilyLoader::new(
            FamilyPluginGate {
                engine_version: env!("CARGO_PKG_VERSION").into(),
                rustc_fingerprint: env!("ASTRA_EMU_CLI_RUSTC_FINGERPRINT").into(),
                feature_fingerprint: expected_feature_fingerprint(&self.family_id)?.into(),
                abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
                target: env!("ASTRA_EMU_TARGET").into(),
                allowed_signers: BTreeSet::from([signer.to_owned()]),
                require_native_manifest_binding: false,
                expected_native_manifest_hash: None,
            },
            Arc::new(verifier),
        );
        let binary_hash = manifest.binary_hash;
        loader
            .load(
                &self.library_path,
                manifest,
                format!("astra.emu.cli.family.{}", self.family_id),
                DynamicFamilyHostServicesV8 {
                    vfs,
                    text_layout: None,
                    private_material,
                    save_store: None,
                },
            )
            .map(|provider| {
                (
                    Box::new(provider) as Box<dyn LegacyRuntimeProvider>,
                    binary_hash,
                )
            })
            .map_err(|error| error.to_string())
    }
}

fn validate_family_id(family_id: &str) -> Result<(), String> {
    match family_id {
        "fvp" | "minori" | "siglus" => Ok(()),
        _ => Err("ASTRA_EMU_CLI_FAMILY_UNSUPPORTED".into()),
    }
}

fn expected_feature_fingerprint(family_id: &str) -> Result<&'static str, String> {
    match family_id {
        "fvp" => Ok(env!("ASTRA_EMU_FVP_FEATURE_FINGERPRINT")),
        "minori" => Ok(env!("ASTRA_EMU_MINORI_FEATURE_FINGERPRINT")),
        "siglus" => Ok(env!("ASTRA_EMU_SIGLUS_FEATURE_FINGERPRINT")),
        _ => Err("ASTRA_EMU_CLI_FAMILY_UNSUPPORTED".into()),
    }
}

fn platform_library_name(family_id: &str) -> Result<&'static Path, String> {
    Ok(
        match (
            family_id,
            cfg!(target_os = "windows"),
            cfg!(target_os = "macos"),
        ) {
            ("fvp", true, _) => Path::new("astra_emu_fvp.dll"),
            ("fvp", false, true) => Path::new("libastra_emu_fvp.dylib"),
            ("fvp", false, false) => Path::new("libastra_emu_fvp.so"),
            ("minori", true, _) => Path::new("astra_emu_minori.dll"),
            ("minori", false, true) => Path::new("libastra_emu_minori.dylib"),
            ("minori", false, false) => Path::new("libastra_emu_minori.so"),
            ("siglus", true, _) => Path::new("astra_emu_siglus.dll"),
            ("siglus", false, true) => Path::new("libastra_emu_siglus.dylib"),
            ("siglus", false, false) => Path::new("libastra_emu_siglus.so"),
            _ => return Err("ASTRA_EMU_CLI_FAMILY_UNSUPPORTED".into()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siglus_private_material_is_root_scoped_and_exact_length() {
        let root = tempfile::tempdir().unwrap();
        let key_path = root.path().join("key.toml");
        fs::write(
            &key_path,
            "key = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]\n",
        )
        .unwrap();
        let host = CliPrivateMaterialHost::load_siglus(
            &fs::canonicalize(root.path()).unwrap(),
            &key_path,
            "siglus.scene-key",
        )
        .unwrap();
        let secret = host
            .read_private_material(
                &LegacyRuntimeSessionId("session".into()),
                LegacyPrivateMaterialRequestV8 {
                    secret_id: "siglus.scene-key".into(),
                    exact_len: 16,
                },
            )
            .unwrap();
        assert_eq!(secret.len(), 16);
        assert!(host
            .read_private_material(
                &LegacyRuntimeSessionId("session".into()),
                LegacyPrivateMaterialRequestV8 {
                    secret_id: "siglus.scene-key".into(),
                    exact_len: 15,
                },
            )
            .is_err());
    }

    #[test]
    fn siglus_private_material_rejects_paths_outside_authorized_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let key_path = outside.path().join("key.toml");
        fs::write(
            &key_path,
            "key = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]\n",
        )
        .unwrap();
        let error = CliPrivateMaterialHost::load_siglus(
            &fs::canonicalize(root.path()).unwrap(),
            &key_path,
            "siglus.scene-key",
        )
        .err()
        .unwrap();
        assert_eq!(error, "ASTRA_EMU_PRIVATE_MATERIAL_SCOPE");
    }
}
