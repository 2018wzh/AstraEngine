use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RString, RVec},
    StableAbi,
};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    LegacyEphemeralText, LegacyOpenRequest, LegacyProbeRequest, LegacyProviderError,
    LegacyRuntimeHostCtx, LegacyRuntimeSessionId, LegacySnapshotEnvelope, LegacyStepInput,
};

pub const LEGACY_FAMILY_ABI_FINGERPRINT: &str = "astra.emu.family_abi.v4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyProviderInstanceRequest {
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyProbeCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub request: LegacyProbeRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyOpenCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub request: LegacyOpenRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyStepCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub session_id: LegacyRuntimeSessionId,
    pub input: LegacyStepInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacySessionCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub session_id: LegacyRuntimeSessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyRestoreCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub session_id: LegacyRuntimeSessionId,
    pub snapshot: LegacySnapshotEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyTextLeaseCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub session_id: LegacyRuntimeSessionId,
    pub lease_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyResourceReadCall {
    pub instance_id: String,
    pub ctx: LegacyRuntimeHostCtx,
    pub session_id: LegacyRuntimeSessionId,
    pub resource_uri: String,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsStatCall {
    pub mount_set_id: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsRangeCall {
    pub mount_set_id: String,
    pub uri: String,
    pub expected_revision: astra_byte_source::SourceRevision,
    pub range: astra_byte_source::ByteRange,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FfiLegacyEphemeralText {
    pub lease_id: String,
    pub text: String,
    pub speaker: Option<String>,
}

impl From<LegacyEphemeralText> for FfiLegacyEphemeralText {
    fn from(value: LegacyEphemeralText) -> Self {
        Self {
            lease_id: value.lease_id,
            text: value.text,
            speaker: value.speaker,
        }
    }
}

impl From<FfiLegacyEphemeralText> for LegacyEphemeralText {
    fn from(value: FfiLegacyEphemeralText) -> Self {
        Self {
            lease_id: value.lease_id,
            text: value.text,
            speaker: value.speaker,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiLegacyResult {
    pub ok: bool,
    pub payload: RVec<u8>,
    pub diagnostic_code: RString,
    pub diagnostic_message: RString,
}

impl FfiLegacyResult {
    pub fn success<T: Serialize>(value: &T) -> Self {
        match postcard::to_allocvec(value) {
            Ok(payload) => Self {
                ok: true,
                payload: payload.into(),
                diagnostic_code: RString::new(),
                diagnostic_message: RString::new(),
            },
            Err(error) => Self::failure(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_ENCODE",
                error.to_string(),
            )),
        }
    }

    pub fn failure(error: LegacyProviderError) -> Self {
        Self {
            ok: false,
            payload: RVec::new(),
            diagnostic_code: error.code().into(),
            diagnostic_message: error.message().into(),
        }
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, LegacyProviderError> {
        if !self.ok {
            return Err(LegacyProviderError::remote(
                self.diagnostic_code.to_string(),
                self.diagnostic_message.to_string(),
            ));
        }
        postcard::from_bytes(&self.payload).map_err(|error| {
            LegacyProviderError::invalid("ASTRA_EMU_FFI_DECODE", error.to_string())
        })
    }
}

pub type FfiLegacyInvoke = extern "C" fn(RVec<u8>) -> FfiLegacyResult;
pub type FfiLegacyVfsCall = extern "C" fn(RString, RVec<u8>) -> FfiLegacyResult;

#[repr(C)]
#[derive(Clone, StableAbi)]
pub struct FfiLegacyHostServices {
    pub host_token: RString,
    #[sabi(unsafe_opaque_field)]
    pub stat_vfs: FfiLegacyVfsCall,
    #[sabi(unsafe_opaque_field)]
    pub read_vfs_range: FfiLegacyVfsCall,
}

impl core::fmt::Debug for FfiLegacyHostServices {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("FfiLegacyHostServices")
            .field("host_token", &"redacted")
            .finish()
    }
}

pub type FfiLegacyCreateInstance =
    extern "C" fn(FfiLegacyHostServices, RVec<u8>) -> FfiLegacyResult;

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(
    prefix_ref = AstraLegacyFamilyModuleRef,
    prefix_fields = AstraLegacyFamilyModulePrefix
)))]
#[sabi(missing_field(panic))]
pub struct AstraLegacyFamilyModule {
    #[sabi(unsafe_opaque_field)]
    pub descriptor: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub create_instance: FfiLegacyCreateInstance,
    #[sabi(unsafe_opaque_field)]
    pub destroy_instance: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub probe: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub open: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub step: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub save: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub restore: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub take_ephemeral_text: FfiLegacyInvoke,
    #[sabi(unsafe_opaque_field)]
    pub read_session_resource: FfiLegacyInvoke,
    #[sabi(last_prefix_field)]
    #[sabi(unsafe_opaque_field)]
    pub shutdown: FfiLegacyInvoke,
}

impl RootModule for AstraLegacyFamilyModuleRef {
    abi_stable::declare_root_module_statics! {AstraLegacyFamilyModuleRef}

    const BASE_NAME: &'static str = "astra_legacy_family_module";
    const NAME: &'static str = "astra-legacy-family";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}

pub fn decode_ffi_request<T: DeserializeOwned>(bytes: RVec<u8>) -> Result<T, LegacyProviderError> {
    postcard::from_bytes(&bytes)
        .map_err(|error| LegacyProviderError::invalid("ASTRA_EMU_FFI_REQUEST", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FamilyId, LegacyFamilyPluginDescriptor};

    #[test]
    fn ffi_result_round_trips_descriptor_without_native_ownership() {
        let descriptor = LegacyFamilyPluginDescriptor {
            family_id: FamilyId("fvp".into()),
            plugin_id: "astra.emu.fvp".into(),
            provider_id: "astra.emu.fvp.runtime".into(),
            engine_version: "0.1.0".into(),
            rustc_fingerprint: "rustc.stable".into(),
            feature_fingerprint: "fvp.test".into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            supported_formats: vec!["fvp.hcb".into()],
            permissions: vec!["vfs.read".into()],
            report_redaction: "astra.emu.redaction.v1".into(),
            license: "MPL-2.0".into(),
        };
        let result = FfiLegacyResult::success(&descriptor);
        let decoded: LegacyFamilyPluginDescriptor = result.decode().unwrap();
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn ffi_failure_preserves_code_without_duplicating_it() {
        let result = FfiLegacyResult::failure(LegacyProviderError::invalid("TEST_CODE", "message"));
        let error = result.decode::<()>().unwrap_err();
        assert_eq!(error.code(), "TEST_CODE");
        assert_eq!(error.message(), "message");
        assert_eq!(error.to_string(), "TEST_CODE: message");
    }
}
