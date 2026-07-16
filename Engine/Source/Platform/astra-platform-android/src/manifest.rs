use astra_platform::{PlatformError, PlatformErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_interpreter_only_features, ANDROID_BUNDLE_IDENTITY_MISMATCH};

pub const ANDROID_BUNDLE_MANIFEST_SCHEMA: &str = "astra.android_bundle_manifest.v1";
pub const ANDROID_MIN_SDK: u32 = 28;
pub const ANDROID_TARGET_SDK: u32 = 36;
pub const ANDROID_COMPILE_SDK: u32 = 36;
pub const ANDROID_BUILD_TOOLS: &str = "36.0.0";
pub const ANDROID_NDK_VERSION: &str = "30.0.15729638";
pub const ANDROID_AGP_VERSION: &str = "9.3.0";
pub const ANDROID_GRADLE_VERSION: &str = "9.5.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AndroidSigningMode {
    Debug,
    External,
    Unsigned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AndroidArtifactIdentity {
    pub kind: String,
    pub file_name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AndroidBundleManifest {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub package_id: String,
    pub package_hash: String,
    pub build_fingerprint: String,
    pub min_sdk: u32,
    pub compile_sdk: u32,
    pub target_sdk: u32,
    pub build_tools: String,
    pub ndk_version: String,
    pub agp_version: String,
    pub gradle_version: String,
    pub jdk_major: u32,
    pub jdk_version: String,
    pub toolchain: Vec<AndroidArtifactIdentity>,
    pub shipping_abis: Vec<String>,
    pub test_abis: Vec<String>,
    pub native_library: AndroidArtifactIdentity,
    pub artifacts: Vec<AndroidArtifactIdentity>,
    pub signing_mode: AndroidSigningMode,
    #[serde(default)]
    pub cargo_features: Vec<String>,
}

impl AndroidBundleManifest {
    pub fn validate(&self) -> Result<(), PlatformError> {
        let valid_hash = |value: &str| {
            value.len() == 71
                && value.starts_with("sha256:")
                && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        let fixed_versions = self.schema == ANDROID_BUNDLE_MANIFEST_SCHEMA
            && self.min_sdk == ANDROID_MIN_SDK
            && self.compile_sdk == ANDROID_COMPILE_SDK
            && self.target_sdk == ANDROID_TARGET_SDK
            && self.build_tools == ANDROID_BUILD_TOOLS
            && self.ndk_version == ANDROID_NDK_VERSION
            && self.agp_version == ANDROID_AGP_VERSION
            && self.gradle_version == ANDROID_GRADLE_VERSION
            && self.jdk_major == 17
            && self.jdk_version.starts_with("17.");
        let identities_complete = !self.target.is_empty()
            && !self.profile.is_empty()
            && !self.package_id.is_empty()
            && valid_hash(&self.package_hash)
            && valid_hash(&self.build_fingerprint)
            && self.native_library.kind == "cdylib"
            && self.native_library.file_name == "libastra_player_android.so"
            && valid_hash(&self.native_library.sha256)
            && [
                "jdk_runtime",
                "android_build_tools",
                "android_ndk",
                "gradle_wrapper",
            ]
            .into_iter()
            .all(|kind| {
                self.toolchain.iter().any(|artifact| {
                    artifact.kind == kind
                        && !artifact.file_name.is_empty()
                        && valid_hash(&artifact.sha256)
                })
            })
            && self.toolchain.len() == 4
            && self.artifacts.iter().all(|artifact| {
                matches!(artifact.kind.as_str(), "apk" | "aab")
                    && !artifact.file_name.is_empty()
                    && valid_hash(&artifact.sha256)
            });
        let abi_policy =
            self.shipping_abis == ["arm64-v8a"] && self.test_abis == ["arm64-v8a", "x86_64"];
        if !fixed_versions || !identities_complete || !abi_policy {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "android.bundle.validate",
                "Android bundle manifest does not match the pinned release identity",
            )
            .with_field("diagnostic_code", ANDROID_BUNDLE_IDENTITY_MISMATCH));
        }
        validate_interpreter_only_features(self.cargo_features.iter().map(String::as_str))
    }
}
