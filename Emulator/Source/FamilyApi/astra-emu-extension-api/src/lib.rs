use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RResult, RString, RVec},
    StableAbi,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const ASTRA_EMU_EXTENSION_ABI_FINGERPRINT: &str = "astra.emu.extension_abi.v1";
pub const TRANSLATION_TEXT_HOOK_ID: &str = "astra.emu.translation.text.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TranslationTextRequestV1 {
    pub utf8: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TranslationTextResponseV1 {
    pub utf8: Vec<u8>,
}

impl TranslationTextRequestV1 {
    pub fn validate(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.utf8)
    }
}

impl TranslationTextResponseV1 {
    pub fn validate(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.utf8)
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiExtensionDescriptor {
    pub extension_id: RString,
    pub provider_id: RString,
    pub abi_fingerprint: RString,
    pub hook_ids: RVec<RString>,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiExtensionInstanceRequest {
    pub instance_id: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiHookInvocation {
    pub instance_id: RString,
    pub session_id: RString,
    pub invocation_id: RString,
    pub family_id: RString,
    pub family_game_id: RString,
    pub hook_id: RString,
    pub timeout_ms: u32,
    pub payload: RVec<u8>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiHookStatus {
    Completed,
    Unbound,
    TimedOut,
    Failed,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiHookDiagnostic {
    pub code: RString,
    pub message: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiHookResult {
    pub status: FfiHookStatus,
    pub payload: RVec<u8>,
    pub diagnostics: RVec<FfiHookDiagnostic>,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiExtensionError {
    pub code: RString,
    pub message: RString,
}

pub type FfiExtensionResult<T> = RResult<T, FfiExtensionError>;
pub type FfiDescriptor = extern "C" fn() -> FfiExtensionResult<FfiExtensionDescriptor>;
pub type FfiCreateInstance = extern "C" fn(FfiExtensionInstanceRequest) -> FfiExtensionResult<()>;
pub type FfiDestroyInstance = extern "C" fn(FfiExtensionInstanceRequest) -> FfiExtensionResult<()>;
pub type FfiInvokeHook = extern "C" fn(FfiHookInvocation) -> FfiExtensionResult<FfiHookResult>;

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(
    prefix_ref = AstraEmuExtensionModuleRef,
    prefix_fields = AstraEmuExtensionModulePrefix
)))]
#[sabi(missing_field(panic))]
pub struct AstraEmuExtensionModule {
    #[sabi(unsafe_opaque_field)]
    pub descriptor: FfiDescriptor,
    #[sabi(unsafe_opaque_field)]
    pub create_instance: FfiCreateInstance,
    #[sabi(unsafe_opaque_field)]
    pub destroy_instance: FfiDestroyInstance,
    #[sabi(last_prefix_field)]
    #[sabi(unsafe_opaque_field)]
    pub invoke_hook: FfiInvokeHook,
}

impl RootModule for AstraEmuExtensionModuleRef {
    abi_stable::declare_root_module_statics! {AstraEmuExtensionModuleRef}

    const BASE_NAME: &'static str = "astra_emu_extension_module";
    const NAME: &'static str = "astra-emu-extension";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}

pub fn translation_request(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

pub fn translation_response(bytes: &[u8]) -> Result<&str, std::str::Utf8Error> {
    std::str::from_utf8(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_companion_is_utf8_only() {
        assert_eq!(
            translation_response(&translation_request("翻译")).unwrap(),
            "翻译"
        );
        assert!(translation_response(&[0xff]).is_err());
        assert!(TranslationTextRequestV1 { utf8: vec![0xff] }
            .validate()
            .is_err());
    }
}
