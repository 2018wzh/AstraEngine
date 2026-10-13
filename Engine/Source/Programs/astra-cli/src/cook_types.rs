use std::{
    fs,
    path::{Path, PathBuf},
};

use astra_core::Hash256;
use astra_media::UnicodeRange;
use astra_package::{
    AuthorizedSourceReader, ContainerError, CookSummaryManifest, SectionCodec,
    SourceFingerprintCryptoProvider, SourceUnlockPolicy, SourceVerificationManifest,
};
use astra_platform::PlatformHostProfile;
use astra_target::TargetManifest;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CookManifest {
    pub(crate) schema: String,
    pub(crate) package_id: String,
    pub(crate) profile: String,
    pub(crate) target: String,
    pub(crate) project_hash: String,
    pub(crate) target_manifest: TargetManifest,
    #[serde(default)]
    pub(crate) scenario_refs: Vec<String>,
    pub(crate) artifacts: Vec<CookedArtifactRef>,
    pub(crate) asset_cook: CookSummaryManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CookedArtifactRef {
    pub(crate) section_id: String,
    pub(crate) schema: String,
    pub(crate) path: String,
    pub(crate) hash: String,
    #[serde(default = "default_section_codec")]
    pub(crate) codec: SectionCodec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asset_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asset_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asset_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asset_byte_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asset_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) font: Option<CookedFontMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scenario_path: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PreparedAssetCook {
    pub(crate) asset_id: String,
    pub(crate) source_path: PathBuf,
    pub(crate) source_hash: Hash256,
    pub(crate) source_byte_size: u64,
    pub(crate) asset_role: String,
    pub(crate) asset_type: String,
    pub(crate) font: Option<CookedFontMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CookedFontMetadata {
    pub(crate) family: String,
    pub(crate) face_index: u32,
    pub(crate) license_id: String,
    pub(crate) subset: Option<String>,
    pub(crate) coverage: Vec<UnicodeRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ProjectPackageSection {
    pub(crate) id: String,
    pub(crate) schema: String,
    pub(crate) path: String,
    #[serde(default = "default_section_codec")]
    pub(crate) codec: SectionCodec,
    #[serde(default)]
    pub(crate) targets: Vec<String>,
    #[serde(default)]
    pub(crate) profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CookedPlatformProfiles {
    pub(crate) schema: String,
    pub(crate) profiles: Vec<PlatformHostProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct StandaloneBundleManifest {
    pub(crate) schema: String,
    pub(crate) target: String,
    pub(crate) profile: String,
    pub(crate) platform: String,
    pub(crate) entrypoint: String,
    pub(crate) package_hash: String,
    pub(crate) package: String,
    pub(crate) scenario_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mount_policy: Option<String>,
    pub(crate) observability: BundleObservabilityEvidence,
    pub(crate) checks: Vec<PlayerLaunchCheck>,
    pub(crate) files: Vec<StandaloneBundleFile>,
}

#[derive(Clone, Default)]
pub(crate) struct BundleArtifactInputs {
    pub(crate) windows_player: Option<PathBuf>,
    pub(crate) windows_runtime: Option<PathBuf>,
    pub(crate) linux_player: Option<PathBuf>,
    pub(crate) macos_player: Option<PathBuf>,
    pub(crate) crash_reporter: Option<PathBuf>,
    pub(crate) web_player_wasm: Option<PathBuf>,
    pub(crate) web_player_glue: Option<PathBuf>,
    pub(crate) android_arm64_library: Option<PathBuf>,
    pub(crate) android_x86_64_library: Option<PathBuf>,
    pub(crate) source_profile: Option<PathBuf>,
    pub(crate) source_root: Option<PathBuf>,
}

pub(crate) struct SourceLockBuildInputs {
    pub(crate) policy: SourceUnlockPolicy,
    pub(crate) crypto: SourceFingerprintCryptoProvider,
}

impl SourceLockBuildInputs {
    pub(crate) fn from_paths(
        policy_path: Option<&Path>,
        profile_path: Option<&Path>,
        source_root: Option<&Path>,
    ) -> Result<Option<Self>, CliError> {
        match (policy_path, profile_path, source_root) {
            (None, None, None) => Ok(None),
            (Some(policy_path), Some(profile_path), Some(source_root)) => {
                let policy: SourceUnlockPolicy = serde_json::from_slice(&fs::read(policy_path)?)?;
                let profile: SourceVerificationManifest =
                    serde_json::from_slice(&fs::read(profile_path)?)?;
                let mut reader = FilesystemSourceReader::open(source_root)?;
                let crypto =
                    SourceFingerprintCryptoProvider::unlock(&policy, &profile, &mut reader)?;
                Ok(Some(Self { policy, crypto }))
            }
            _ => {
                Err("source-locked package build requires policy, profile, and source root".into())
            }
        }
    }
}

pub(crate) struct FilesystemSourceReader {
    pub(crate) root: PathBuf,
}

impl FilesystemSourceReader {
    pub(crate) fn open(root: &Path) -> Result<Self, CliError> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err("authorized source root is not a directory".into());
        }
        Ok(Self { root })
    }

    pub(crate) fn resolve(&self, relative_path: &str) -> Result<PathBuf, ContainerError> {
        astra_platform::validate_safe_relative_path(relative_path)
            .map_err(|_| ContainerError::Crypto("authorized source path is unsafe".into()))?;
        let path = fs::canonicalize(self.root.join(relative_path))
            .map_err(|_| ContainerError::Crypto("authorized source entry is unavailable".into()))?;
        if !path.starts_with(&self.root) || !path.is_file() {
            return Err(ContainerError::Crypto(
                "authorized source entry escapes the selected root".into(),
            ));
        }
        Ok(path)
    }
}

impl AuthorizedSourceReader for FilesystemSourceReader {
    fn stat_relative(&mut self, relative_path: &str) -> Result<u64, ContainerError> {
        fs::metadata(self.resolve(relative_path)?)
            .map(|metadata| metadata.len())
            .map_err(|_| ContainerError::Crypto("authorized source stat failed".into()))
    }

    fn read_relative_range(
        &mut self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        let path = self.resolve(relative_path)?;
        let file_length = fs::metadata(&path)
            .map_err(|_| ContainerError::Crypto("authorized source stat failed".into()))?
            .len();
        let end = offset
            .checked_add(length)
            .ok_or_else(|| ContainerError::Crypto("authorized source range overflowed".into()))?;
        if length == 0 || length > max_bytes || end > file_length || length > usize::MAX as u64 {
            return Err(ContainerError::Crypto(
                "authorized source range exceeds its byte budget".into(),
            ));
        }
        let mut file = fs::File::open(path)
            .map_err(|_| ContainerError::Crypto("authorized source open failed".into()))?;
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| {
                let mut bytes = vec![0_u8; length as usize];
                file.read_exact(&mut bytes).map(|_| bytes)
            })
            .map_err(|_| ContainerError::Crypto("authorized source range read failed".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BundleObservabilityEvidence {
    pub(crate) log_schema: String,
    pub(crate) crash_reporting: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) crash_reporter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct StandaloneBundleFile {
    pub(crate) path: String,
    pub(crate) role: String,
    pub(crate) hash: String,
    pub(crate) byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PlayerLaunchCheck {
    pub(crate) id: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PlayerDisplayConfig {
    pub(crate) schema: String,
    pub(crate) original_resolution: PlayerResolution,
    pub(crate) scale_filter: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preview_layers: Vec<PlayerDisplayLayer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PlayerResolution {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PlayerDisplayLayer {
    pub(crate) vfs_uri: String,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

pub(crate) fn default_section_codec() -> SectionCodec {
    SectionCodec::Raw
}
