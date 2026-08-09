use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{ROption, RResult, RString, RVec},
    StableAbi,
};
use astra_byte_source::FfiOwnedByteBuffer;
use zeroize::Zeroize;

use crate::{
    FfiByteRange, FfiByteSourceStat, FfiEphemeralText, FfiFamilyPluginDescriptor, FfiOpenRequest,
    FfiProbeReport, FfiProbeRequest, FfiRangeReadResult, FfiRestoreReport, FfiRuntimeHostCtx,
    FfiShutdownReport, FfiSnapshotEnvelope, FfiStepInput, FfiStepOutput, FfiVfsListedFile,
    LegacyProviderError,
};

/// The v8 wire contract makes bulk ownership and its scalar kind explicit.
/// v7/v6/v5 modules are intentionally rejected by the loader; there is no shim.
pub const LEGACY_FAMILY_ABI_FINGERPRINT: &str = "astra.emu.family_abi.v8";

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
    pub expected_revision: u64,
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

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiTextWrapV8 {
    None,
    Word,
    Glyph,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiTextOverflowV8 {
    Clip,
    Ellipsis,
}

#[repr(C)]
#[derive(Clone, StableAbi)]
pub struct FfiTextLayoutRequestV8 {
    pub lease_id: RString,
    pub text: RString,
    pub language: RString,
    pub font_families: RVec<RString>,
    pub font_size: f32,
    pub line_height: f32,
    pub width: u32,
    pub height: u32,
    pub max_lines: u32,
    pub wrap: FfiTextWrapV8,
    pub overflow: FfiTextOverflowV8,
}

impl core::fmt::Debug for FfiTextLayoutRequestV8 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("FfiTextLayoutRequestV8")
            .field("lease_id", &self.lease_id)
            .field("text_bytes", &self.text.len())
            .field("language", &self.language)
            .field("font_families", &self.font_families)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("max_lines", &self.max_lines)
            .finish_non_exhaustive()
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiTextLayoutResultV8 {
    pub layout_token: RString,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    pub line_count: u32,
    pub glyph_count: u32,
    pub clipped: bool,
    pub cache_revision: u64,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiTextLayoutCallV8 {
    pub session_id: RString,
    pub request: FfiTextLayoutRequestV8,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiTextLayoutReleaseCallV8 {
    pub session_id: RString,
    pub layout_token: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiPrivateMaterialCallV8 {
    pub session_id: RString,
    pub secret_id: RString,
    pub exact_len: u32,
}

#[repr(C)]
#[derive(StableAbi)]
pub struct FfiSecretBufferV8 {
    pub bytes: RVec<u8>,
}

impl core::fmt::Debug for FfiSecretBufferV8 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("FfiSecretBufferV8")
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl Drop for FfiSecretBufferV8 {
    fn drop(&mut self) {
        self.bytes.as_mut_slice().zeroize();
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSaveSlotV8 {
    pub slot_id: RString,
    pub revision: u64,
    pub byte_len: u64,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSaveListCallV8 {
    pub session_id: RString,
    pub max_slots: u32,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSaveReadCallV8 {
    pub session_id: RString,
    pub slot_id: RString,
    pub expected_revision: ROption<u64>,
    pub max_bytes: u64,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiSaveReadResultV8 {
    pub slot_id: RString,
    pub revision: u64,
    pub payload: FfiOwnedByteBuffer,
}

#[repr(C)]
#[derive(StableAbi)]
pub struct FfiSaveWriteCallV8 {
    pub session_id: RString,
    pub slot_id: RString,
    pub expected_revision: ROption<u64>,
    pub max_bytes: u64,
    pub payload: FfiOwnedByteBuffer,
}

impl core::fmt::Debug for FfiSaveWriteCallV8 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("FfiSaveWriteCallV8")
            .field("session_id", &self.session_id)
            .field("slot_id", &self.slot_id)
            .field("expected_revision", &self.expected_revision)
            .field("max_bytes", &self.max_bytes)
            .field("payload_bytes", &self.payload.len())
            .finish()
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiSaveWriteResultV8 {
    pub slot_id: RString,
    pub revision: u64,
    pub byte_len: u64,
    pub verified: bool,
}

impl From<crate::LegacyTextLayoutRequestV8> for FfiTextLayoutRequestV8 {
    fn from(value: crate::LegacyTextLayoutRequestV8) -> Self {
        Self {
            lease_id: value.lease_id.into(),
            text: value.text.into(),
            language: value.language.into(),
            font_families: value
                .font_families
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            font_size: value.font_size,
            line_height: value.line_height,
            width: value.width,
            height: value.height,
            max_lines: value.max_lines,
            wrap: match value.wrap {
                crate::LegacyTextWrapV8::None => FfiTextWrapV8::None,
                crate::LegacyTextWrapV8::Word => FfiTextWrapV8::Word,
                crate::LegacyTextWrapV8::Glyph => FfiTextWrapV8::Glyph,
            },
            overflow: match value.overflow {
                crate::LegacyTextOverflowV8::Clip => FfiTextOverflowV8::Clip,
                crate::LegacyTextOverflowV8::Ellipsis => FfiTextOverflowV8::Ellipsis,
            },
        }
    }
}

impl From<FfiTextLayoutRequestV8> for crate::LegacyTextLayoutRequestV8 {
    fn from(value: FfiTextLayoutRequestV8) -> Self {
        Self {
            lease_id: value.lease_id.to_string(),
            text: value.text.to_string(),
            language: value.language.to_string(),
            font_families: value
                .font_families
                .into_iter()
                .map(|value| value.to_string())
                .collect(),
            font_size: value.font_size,
            line_height: value.line_height,
            width: value.width,
            height: value.height,
            max_lines: value.max_lines,
            wrap: match value.wrap {
                FfiTextWrapV8::None => crate::LegacyTextWrapV8::None,
                FfiTextWrapV8::Word => crate::LegacyTextWrapV8::Word,
                FfiTextWrapV8::Glyph => crate::LegacyTextWrapV8::Glyph,
            },
            overflow: match value.overflow {
                FfiTextOverflowV8::Clip => crate::LegacyTextOverflowV8::Clip,
                FfiTextOverflowV8::Ellipsis => crate::LegacyTextOverflowV8::Ellipsis,
            },
        }
    }
}

impl From<crate::LegacyTextLayoutResultV8> for FfiTextLayoutResultV8 {
    fn from(value: crate::LegacyTextLayoutResultV8) -> Self {
        Self {
            layout_token: value.layout_token.into(),
            width: value.width,
            height: value.height,
            baseline: value.baseline,
            line_count: value.line_count,
            glyph_count: value.glyph_count,
            clipped: value.clipped,
            cache_revision: value.cache_revision,
        }
    }
}

impl From<FfiTextLayoutResultV8> for crate::LegacyTextLayoutResultV8 {
    fn from(value: FfiTextLayoutResultV8) -> Self {
        Self {
            layout_token: value.layout_token.to_string(),
            width: value.width,
            height: value.height,
            baseline: value.baseline,
            line_count: value.line_count,
            glyph_count: value.glyph_count,
            clipped: value.clipped,
            cache_revision: value.cache_revision,
        }
    }
}

impl FfiSecretBufferV8 {
    pub fn into_native(mut self) -> crate::LegacySecretBufferV8 {
        let bytes = core::mem::take(&mut self.bytes)
            .into_iter()
            .collect::<Vec<_>>();
        crate::LegacySecretBufferV8::new(bytes)
    }
}

impl From<crate::LegacySaveSlotV8> for FfiSaveSlotV8 {
    fn from(value: crate::LegacySaveSlotV8) -> Self {
        Self {
            slot_id: value.slot_id.into(),
            revision: value.revision,
            byte_len: value.byte_len,
        }
    }
}

impl From<FfiSaveSlotV8> for crate::LegacySaveSlotV8 {
    fn from(value: FfiSaveSlotV8) -> Self {
        Self {
            slot_id: value.slot_id.to_string(),
            revision: value.revision,
            byte_len: value.byte_len,
        }
    }
}

impl From<crate::LegacySaveWriteResultV8> for FfiSaveWriteResultV8 {
    fn from(value: crate::LegacySaveWriteResultV8) -> Self {
        Self {
            slot_id: value.slot_id.into(),
            revision: value.revision,
            byte_len: value.byte_len,
            verified: value.verified,
        }
    }
}

impl From<FfiSaveWriteResultV8> for crate::LegacySaveWriteResultV8 {
    fn from(value: FfiSaveWriteResultV8) -> Self {
        Self {
            slot_id: value.slot_id.to_string(),
            revision: value.revision,
            byte_len: value.byte_len,
            verified: value.verified,
        }
    }
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
    extern "C" fn(FfiResourceReadCall) -> FfiLegacyResult<FfiOwnedByteBuffer>;
pub type FfiShutdown = extern "C" fn(FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport>;

pub type FfiVfsStat = extern "C" fn(RString, FfiVfsStatCall) -> FfiLegacyResult<FfiByteSourceStat>;
pub type FfiVfsReadRange =
    extern "C" fn(RString, FfiVfsRangeCall) -> FfiLegacyResult<FfiRangeReadResult>;
pub type FfiVfsEnumerate =
    extern "C" fn(
        RString,
        FfiVfsEnumerateCall,
    ) -> FfiLegacyResult<abi_stable::std_types::RVec<FfiVfsListedFile>>;
pub type FfiLayoutTextV8 =
    extern "C" fn(RString, FfiTextLayoutCallV8) -> FfiLegacyResult<FfiTextLayoutResultV8>;
pub type FfiReleaseTextLayoutV8 =
    extern "C" fn(RString, FfiTextLayoutReleaseCallV8) -> FfiLegacyResult<()>;
pub type FfiReadPrivateMaterialV8 =
    extern "C" fn(RString, FfiPrivateMaterialCallV8) -> FfiLegacyResult<FfiSecretBufferV8>;
pub type FfiListSaveSlotsV8 =
    extern "C" fn(RString, FfiSaveListCallV8) -> FfiLegacyResult<RVec<FfiSaveSlotV8>>;
pub type FfiReadSaveSlotV8 =
    extern "C" fn(RString, FfiSaveReadCallV8) -> FfiLegacyResult<FfiSaveReadResultV8>;
pub type FfiAtomicWriteSaveSlotV8 =
    extern "C" fn(RString, FfiSaveWriteCallV8) -> FfiLegacyResult<FfiSaveWriteResultV8>;

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
    #[sabi(unsafe_opaque_field)]
    pub layout_text: FfiLayoutTextV8,
    #[sabi(unsafe_opaque_field)]
    pub release_text_layout: FfiReleaseTextLayoutV8,
    #[sabi(unsafe_opaque_field)]
    pub read_private_material: FfiReadPrivateMaterialV8,
    #[sabi(unsafe_opaque_field)]
    pub list_save_slots: FfiListSaveSlotsV8,
    #[sabi(unsafe_opaque_field)]
    pub read_save_slot: FfiReadSaveSlotV8,
    #[sabi(unsafe_opaque_field)]
    pub atomic_write_save_slot: FfiAtomicWriteSaveSlotV8,
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
    use crate::{FamilyId, FfiOwnedBytes, LegacyFamilyPluginDescriptor};

    #[test]
    fn v8_descriptor_round_trips_through_typed_wire() {
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
    fn v8_error_preserves_code_without_serialization() {
        let ffi = FfiLegacyError::from(LegacyProviderError::invalid("TEST_CODE", "message"));
        let error = LegacyProviderError::from(ffi);
        assert_eq!(error.code(), "TEST_CODE");
        assert_eq!(error.message(), "message");
    }

    #[test]
    fn owned_resource_bytes_cross_ffi_without_reallocation() {
        let bytes = vec![1_u8, 2, 3, 4, 5];
        let allocation = bytes.as_ptr();
        let bytes = FfiOwnedBytes::new(bytes).into_bytes();
        assert_eq!(bytes.as_ptr(), allocation);
    }
}
