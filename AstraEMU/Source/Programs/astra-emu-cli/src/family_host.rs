use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use astra_emu_family_api::{LegacyRuntimeProvider, LegacyVfsReader, LEGACY_FAMILY_ABI_FINGERPRINT};
use astra_emu_manager_core::{
    DynamicFamilyLoader, Ed25519FamilySignatureVerifier, FamilyPluginGate, FamilyPluginManifest,
};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

pub struct CliFamilyHostConfig {
    manifest_path: PathBuf,
    library_path: PathBuf,
}

impl CliFamilyHostConfig {
    pub fn installed_for_executable(executable: &Path) -> Result<Self, String> {
        let install_root = executable.parent().ok_or("ASTRA_EMU_INSTALL_ROOT")?;
        let family_root = install_root.join("families").join("fvp");
        Ok(Self {
            manifest_path: family_root.join("manifest.json"),
            library_path: family_root.join(platform_library_name()),
        })
    }

    pub fn with_paths(manifest_path: PathBuf, library_path: PathBuf) -> Self {
        Self {
            manifest_path,
            library_path,
        }
    }

    pub fn create_provider(
        &self,
        vfs: Arc<dyn LegacyVfsReader>,
    ) -> Result<Box<dyn LegacyRuntimeProvider>, String> {
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
                feature_fingerprint: env!("ASTRA_EMU_FVP_FEATURE_FINGERPRINT").into(),
                abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
                target: env!("ASTRA_EMU_TARGET").into(),
                allowed_signers: BTreeSet::from([signer.to_owned()]),
                require_native_manifest_binding: false,
                expected_native_manifest_hash: None,
            },
            Arc::new(verifier),
        );
        loader
            .load(
                &self.library_path,
                manifest,
                "astra.emu.cli.family.fvp".into(),
                vfs,
            )
            .map(|provider| Box::new(provider) as Box<dyn LegacyRuntimeProvider>)
            .map_err(|error| error.to_string())
    }
}

fn platform_library_name() -> &'static Path {
    if cfg!(target_os = "windows") {
        Path::new("astra_emu_fvp.dll")
    } else if cfg!(target_os = "macos") {
        Path::new("libastra_emu_fvp.dylib")
    } else {
        Path::new("libastra_emu_fvp.so")
    }
}
