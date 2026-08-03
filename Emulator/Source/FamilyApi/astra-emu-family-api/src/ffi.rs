use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RResult, RString},
    StableAbi,
};

use crate::{
    FfiBulkBytes, FfiByteRange, FfiByteSourceStat, FfiEphemeralText, FfiFamilyPluginDescriptor,
    FfiHash256, FfiOpenRequest, FfiProbeReport, FfiProbeRequest, FfiRangeReadResult,
    FfiRestoreReport, FfiRuntimeHostCtx, FfiShutdownReport, FfiSnapshotEnvelope, FfiStepInput,
    FfiStepOutput, FfiVfsListedFile, LegacyProviderError,
};

pub const LEGACY_FAMILY_ABI_FINGERPRINT: &str = "astra.emu.family_abi.v6";

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiLegacyError {
    pub code: RString,
    pub message: RString,
}

impl From<LegacyProviderError> for FfiLegacyError {
    fn from(value: LegacyProviderError) -> Self {
        Self {
            code: value.code().into(),
            message: value.message().into(),
        }
    }
}

impl From<FfiLegacyError> for LegacyProviderError {
    fn from(value: FfiLegacyError) -> Self {
        Self::remote(value.code.to_string(), value.message.to_string())
    }
}

pub type FfiLegacyResult<T> = RResult<T, FfiLegacyError>;

pub fn ffi_result<T, U>(result: Result<T, LegacyProviderError>) -> FfiLegacyResult<U>
where
    T: Into<U>,
{
    match result {
        Ok(value) => RResult::ROk(value.into()),
        Err(error) => RResult::RErr(error.into()),
    }
}

pub fn native_result<T, U>(result: FfiLegacyResult<T>) -> Result<U, LegacyProviderError>
where
    T: Into<U>,
{
    match result {
        RResult::ROk(value) => Ok(value.into()),
        RResult::RErr(error) => Err(error.into()),
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiProviderInstanceRequest {
    pub instance_id: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiProbeCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub request: FfiProbeRequest,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiOpenCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub request: FfiOpenRequest,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiStepCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub session_id: RString,
    pub input: FfiStepInput,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSessionCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub session_id: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiRestoreCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub session_id: RString,
    pub snapshot: FfiSnapshotEnvelope,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiTextLeaseCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub session_id: RString,
    pub lease_id: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiResourceReadCall {
    pub instance_id: RString,
    pub ctx: FfiRuntimeHostCtx,
    pub session_id: RString,
    pub resource_uri: RString,
    pub max_bytes: u64,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiVfsStatCall {
    pub mount_set_id: RString,
    pub uri: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiVfsRangeCall {
    pub mount_set_id: RString,
    pub uri: RString,
    pub expected_revision: FfiHash256,
    pub range: FfiByteRange,
    pub max_bytes: u64,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiVfsEnumerateCall {
    pub mount_set_id: RString,
    pub root: RString,
    pub extension_without_dot: RString,
    pub max_entries: u32,
}

pub type FfiDescriptor = extern "C" fn() -> FfiLegacyResult<FfiFamilyPluginDescriptor>;
pub type FfiCreateInstance =
    extern "C" fn(FfiLegacyHostServices, FfiProviderInstanceRequest) -> FfiLegacyResult<()>;
pub type FfiDestroyInstance = extern "C" fn(FfiProviderInstanceRequest) -> FfiLegacyResult<()>;
pub type FfiProbe = extern "C" fn(FfiProbeCall) -> FfiLegacyResult<FfiProbeReport>;
pub type FfiOpen = extern "C" fn(FfiOpenCall) -> FfiLegacyResult<RString>;
pub type FfiStep = extern "C" fn(FfiStepCall) -> FfiLegacyResult<FfiStepOutput>;
pub type FfiSave = extern "C" fn(FfiSessionCall) -> FfiLegacyResult<FfiSnapshotEnvelope>;
pub type FfiRestore = extern "C" fn(FfiRestoreCall) -> FfiLegacyResult<FfiRestoreReport>;
pub type FfiTakeEphemeralText =
    extern "C" fn(
        FfiTextLeaseCall,
    ) -> FfiLegacyResult<abi_stable::std_types::ROption<FfiEphemeralText>>;
pub type FfiReadSessionResource =
    extern "C" fn(FfiResourceReadCall) -> FfiLegacyResult<FfiBulkBytes>;
pub type FfiShutdown = extern "C" fn(FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport>;

pub type FfiVfsStat = extern "C" fn(RString, FfiVfsStatCall) -> FfiLegacyResult<FfiByteSourceStat>;
pub type FfiVfsReadRange =
    extern "C" fn(RString, FfiVfsRangeCall) -> FfiLegacyResult<FfiRangeReadResult>;
pub type FfiVfsEnumerate =
    extern "C" fn(
        RString,
        FfiVfsEnumerateCall,
    ) -> FfiLegacyResult<abi_stable::std_types::RVec<FfiVfsListedFile>>;

#[repr(C)]
#[derive(Clone, StableAbi)]
pub struct FfiLegacyHostServices {
    pub host_token: RString,
    #[sabi(unsafe_opaque_field)]
    pub stat_vfs: FfiVfsStat,
    #[sabi(unsafe_opaque_field)]
    pub read_vfs_range: FfiVfsReadRange,
    #[sabi(unsafe_opaque_field)]
    pub enumerate_vfs: FfiVfsEnumerate,
}

impl core::fmt::Debug for FfiLegacyHostServices {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("FfiLegacyHostServices")
            .field("host_token", &"redacted")
            .finish()
    }
}

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(
    prefix_ref = AstraLegacyFamilyModuleRef,
    prefix_fields = AstraLegacyFamilyModulePrefix
)))]
#[sabi(missing_field(panic))]
pub struct AstraLegacyFamilyModule {
    #[sabi(unsafe_opaque_field)]
    pub descriptor: FfiDescriptor,
    #[sabi(unsafe_opaque_field)]
    pub create_instance: FfiCreateInstance,
    #[sabi(unsafe_opaque_field)]
    pub destroy_instance: FfiDestroyInstance,
    #[sabi(unsafe_opaque_field)]
    pub probe: FfiProbe,
    #[sabi(unsafe_opaque_field)]
    pub open: FfiOpen,
    #[sabi(unsafe_opaque_field)]
    pub step: FfiStep,
    #[sabi(unsafe_opaque_field)]
    pub save: FfiSave,
    #[sabi(unsafe_opaque_field)]
    pub restore: FfiRestore,
    #[sabi(unsafe_opaque_field)]
    pub take_ephemeral_text: FfiTakeEphemeralText,
    #[sabi(unsafe_opaque_field)]
    pub read_session_resource: FfiReadSessionResource,
    #[sabi(last_prefix_field)]
    #[sabi(unsafe_opaque_field)]
    pub shutdown: FfiShutdown,
}

impl RootModule for AstraLegacyFamilyModuleRef {
    abi_stable::declare_root_module_statics! {AstraLegacyFamilyModuleRef}

    const BASE_NAME: &'static str = "astra_legacy_family_module";
    const NAME: &'static str = "astra-legacy-family";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FamilyId, LegacyFamilyPluginDescriptor};

    #[test]
    fn v6_descriptor_round_trips_through_typed_wire() {
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
        let decoded: LegacyFamilyPluginDescriptor =
            FfiFamilyPluginDescriptor::from(descriptor.clone()).into();
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn v6_error_preserves_code_without_serialization() {
        let ffi = FfiLegacyError::from(LegacyProviderError::invalid("TEST_CODE", "message"));
        let error = LegacyProviderError::from(ffi);
        assert_eq!(error.code(), "TEST_CODE");
        assert_eq!(error.message(), "message");
    }

    #[test]
    fn v6_bulk_buffer_preserves_the_owned_allocation_across_clones() {
        let bytes = vec![1_u8, 2, 3, 4, 5];
        let allocation = bytes.as_ptr();
        let bulk = crate::bulk_bytes_from_vec(bytes);
        assert_eq!(bulk.as_slice().as_ptr(), allocation);
        let clone = bulk.clone();
        drop(bulk);
        assert_eq!(clone.as_slice().as_ptr(), allocation);
        assert_eq!(clone.as_slice(), &[1, 2, 3, 4, 5]);
    }
}
