use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RResult, RString, RVec},
    StableAbi,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

impl FfiExtensionDescriptor {
    pub fn validate(&self) -> Result<(), FfiExtensionError> {
        validate_identity("extension_id", &self.extension_id)?;
        validate_identity("provider_id", &self.provider_id)?;
        if self.abi_fingerprint != ASTRA_EMU_EXTENSION_ABI_FINGERPRINT {
            return Err(extension_error(
                "ASTRA_EMU_EXTENSION_ABI_FINGERPRINT",
                "extension ABI fingerprint does not match v1",
            ));
        }
        if self.hook_ids.is_empty() {
            return Err(extension_error(
                "ASTRA_EMU_EXTENSION_HOOK_SET",
                "extension descriptor must expose at least one hook",
            ));
        }
        let mut hooks = BTreeSet::new();
        for hook_id in &self.hook_ids {
            validate_identity("hook_id", hook_id)?;
            if !hooks.insert(hook_id.as_str()) {
                return Err(extension_error(
                    "ASTRA_EMU_EXTENSION_HOOK_DUPLICATE",
                    "extension descriptor contains a duplicate hook identity",
                ));
            }
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiExtensionInstanceRequest {
    pub instance_id: RString,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiExtensionSessionRequest {
    pub instance_id: RString,
    pub session_id: RString,
    pub family_id: RString,
    pub family_game_id: RString,
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

impl FfiHookInvocation {
    pub fn validate(&self) -> Result<(), FfiExtensionError> {
        for (field, value) in [
            ("instance_id", &self.instance_id),
            ("session_id", &self.session_id),
            ("invocation_id", &self.invocation_id),
            ("family_id", &self.family_id),
            ("family_game_id", &self.family_game_id),
            ("hook_id", &self.hook_id),
        ] {
            validate_identity(field, value)?;
        }
        Ok(())
    }
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

fn validate_identity(field: &str, value: &str) -> Result<(), FfiExtensionError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(extension_error(
            "ASTRA_EMU_EXTENSION_IDENTITY",
            format!("{field} is not a bounded portable identity"),
        ));
    }
    Ok(())
}

fn extension_error(code: &str, message: impl Into<String>) -> FfiExtensionError {
    FfiExtensionError {
        code: code.into(),
        message: message.into().into(),
    }
}

pub type FfiExtensionResult<T> = RResult<T, FfiExtensionError>;
pub type FfiDescriptor = extern "C" fn() -> FfiExtensionResult<FfiExtensionDescriptor>;
pub type FfiCreateInstance = extern "C" fn(FfiExtensionInstanceRequest) -> FfiExtensionResult<()>;
pub type FfiDestroyInstance = extern "C" fn(FfiExtensionInstanceRequest) -> FfiExtensionResult<()>;
pub type FfiOpenSession = extern "C" fn(FfiExtensionSessionRequest) -> FfiExtensionResult<()>;
pub type FfiCloseSession = extern "C" fn(FfiExtensionSessionRequest) -> FfiExtensionResult<()>;
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
    #[sabi(unsafe_opaque_field)]
    pub open_session: FfiOpenSession,
    #[sabi(unsafe_opaque_field)]
    pub close_session: FfiCloseSession,
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

    #[test]
    fn descriptor_requires_v1_and_unique_hooks() {
        let mut descriptor = FfiExtensionDescriptor {
            extension_id: "astra.emu.translation".into(),
            provider_id: "translation.fixture".into(),
            abi_fingerprint: ASTRA_EMU_EXTENSION_ABI_FINGERPRINT.into(),
            hook_ids: vec![TRANSLATION_TEXT_HOOK_ID.into()].into(),
        };
        descriptor.validate().unwrap();
        descriptor.hook_ids.push(TRANSLATION_TEXT_HOOK_ID.into());
        assert_eq!(
            descriptor.validate().unwrap_err().code,
            "ASTRA_EMU_EXTENSION_HOOK_DUPLICATE"
        );
    }

    #[test]
    fn invocation_identity_is_validated_without_interpreting_payload() {
        let mut invocation = FfiHookInvocation {
            instance_id: "extension.1".into(),
            session_id: "session.1".into(),
            invocation_id: "invocation.1".into(),
            family_id: "fvp".into(),
            family_game_id: "game.1".into(),
            hook_id: TRANSLATION_TEXT_HOOK_ID.into(),
            timeout_ms: 0,
            payload: vec![0xff].into(),
        };
        invocation.validate().unwrap();
        invocation.session_id = "../escape".into();
        assert_eq!(
            invocation.validate().unwrap_err().code,
            "ASTRA_EMU_EXTENSION_IDENTITY"
        );
    }
}
