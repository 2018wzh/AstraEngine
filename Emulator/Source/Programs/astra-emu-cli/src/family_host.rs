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
        loader
            .load(
                &self.library_path,
                manifest,
                format!("astra.emu.cli.family.{}", self.family_id),
                vfs,
            )
            .map(|provider| Box::new(provider) as Box<dyn LegacyRuntimeProvider>)
            .map_err(|error| error.to_string())
    }
}

fn validate_family_id(family_id: &str) -> Result<(), String> {
    match family_id {
        "fvp" | "minori" => Ok(()),
        _ => Err("ASTRA_EMU_CLI_FAMILY_UNSUPPORTED".into()),
    }
}

fn expected_feature_fingerprint(family_id: &str) -> Result<&'static str, String> {
    match family_id {
        "fvp" => Ok(env!("ASTRA_EMU_FVP_FEATURE_FINGERPRINT")),
        "minori" => Ok(env!("ASTRA_EMU_MINORI_FEATURE_FINGERPRINT")),
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
            _ => return Err("ASTRA_EMU_CLI_FAMILY_UNSUPPORTED".into()),
        },
    )
}
