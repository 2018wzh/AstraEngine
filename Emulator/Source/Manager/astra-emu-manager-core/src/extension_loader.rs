use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use abi_stable::{
    library::{AbiHeaderRef, ROOT_MODULE_LOADER_NAME_WITH_NUL},
    std_types::{RResult, RVec},
};
use astra_byte_source::OwnedByteBuffer;
use astra_emu_extension_api::{
    AstraEmuExtensionModuleRef, FfiExtensionDescriptor, FfiExtensionError,
    FfiExtensionInstanceRequest, FfiExtensionSessionRequest, FfiHookInvocation, FfiHookStatus,
};
use astra_emu_family_api::{
    LegacyDiagnostic, LegacyHookInvocationV1, LegacyHookResultV1, LegacyHookStatusV1,
    LegacyProviderError,
};
use libloading::Library;

use crate::SynchronousFamilyHookProvider;

/// Loaded Extension ABI v1 hook provider. The dynamic library remains resident
/// until its session and instance have been closed in order.
pub struct LoadedExtensionHookProvider {
    module: AstraEmuExtensionModuleRef,
    descriptor: FfiExtensionDescriptor,
    instance_id: String,
    family_id: String,
    family_game_id: String,
    session: Mutex<Option<FfiExtensionSessionRequest>>,
    _library: Arc<Library>,
}

impl LoadedExtensionHookProvider {
    pub fn load(
        path: impl AsRef<Path>,
        instance_id: impl Into<String>,
        family_id: impl Into<String>,
        family_game_id: impl Into<String>,
    ) -> Result<Self, LegacyProviderError> {
        let library = unsafe { Library::new(path.as_ref()) }.map_err(|_| {
            invalid(
                "ASTRA_EMU_EXTENSION_LIBRARY_LOAD",
                "extension library could not be loaded",
            )
        })?;
        let module = unsafe { root_module(&library)? };
        let descriptor = extension_result((module.descriptor())())?;
        descriptor
            .validate()
            .map_err(|error| extension_error("descriptor", error))?;
        let instance_id = instance_id.into();
        let instance = FfiExtensionInstanceRequest {
            instance_id: instance_id.clone().into(),
        };
        extension_result((module.create_instance())(instance.clone()))?;
        Ok(Self {
            module,
            descriptor,
            instance_id,
            family_id: family_id.into(),
            family_game_id: family_game_id.into(),
            session: Mutex::new(None),
            _library: Arc::new(library),
        })
    }
}

impl SynchronousFamilyHookProvider for LoadedExtensionHookProvider {
    fn provider_id(&self) -> &str {
        self.descriptor.provider_id.as_str()
    }

    fn invoke(
        &self,
        invocation: &LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        if self.family_id != invocation.family_id
            || self.family_game_id != invocation.family_game_id
            || !self
                .descriptor
                .hook_ids
                .iter()
                .any(|hook| hook.as_str() == invocation.hook_id)
        {
            return Err(invalid(
                "ASTRA_EMU_EXTENSION_INVOCATION_BINDING",
                "hook invocation does not match the opened extension session",
            ));
        }
        let mut session = self.session.lock().map_err(|_| {
            invalid(
                "ASTRA_EMU_EXTENSION_SESSION_LOCK",
                "extension session lock is poisoned",
            )
        })?;
        if let Some(opened) = session.as_ref() {
            if opened.session_id != invocation.session_id {
                return Err(invalid(
                    "ASTRA_EMU_EXTENSION_SESSION_IDENTITY",
                    "extension invocation changed session identity",
                ));
            }
        } else {
            let opened = FfiExtensionSessionRequest {
                instance_id: self.instance_id.clone().into(),
                session_id: invocation.session_id.clone().into(),
                family_id: invocation.family_id.clone().into(),
                family_game_id: invocation.family_game_id.clone().into(),
            };
            extension_result((self.module.open_session())(opened.clone()))?;
            *session = Some(opened);
        }
        let ffi = FfiHookInvocation {
            instance_id: self.instance_id.clone().into(),
            session_id: invocation.session_id.clone().into(),
            invocation_id: invocation.invocation_id.clone().into(),
            family_id: invocation.family_id.clone().into(),
            family_game_id: invocation.family_game_id.clone().into(),
            hook_id: invocation.hook_id.clone().into(),
            timeout_ms: invocation.timeout_ms,
            payload: RVec::from(invocation.payload.as_slice().to_vec()),
        };
        ffi.validate()
            .map_err(|error| extension_error("invoke", error))?;
        let result = extension_result((self.module.invoke_hook())(ffi))?;
        let diagnostics = result
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let code = diagnostic.code.into_string();
                if !safe_diagnostic_code(&code) {
                    return Err(invalid(
                        "ASTRA_EMU_EXTENSION_DIAGNOSTIC_CODE",
                        "extension returned an invalid diagnostic code",
                    ));
                }
                Ok(LegacyDiagnostic {
                    code,
                    severity: "warn".into(),
                    message: "extension hook returned a typed diagnostic".into(),
                    subject: "extension".into(),
                })
            })
            .collect::<Result<Vec<_>, LegacyProviderError>>()?;
        Ok(LegacyHookResultV1 {
            status: match result.status {
                FfiHookStatus::Completed => LegacyHookStatusV1::Completed,
                FfiHookStatus::Unbound => LegacyHookStatusV1::Unbound,
                FfiHookStatus::TimedOut => LegacyHookStatusV1::TimedOut,
                FfiHookStatus::Failed => LegacyHookStatusV1::Failed,
            },
            payload: OwnedByteBuffer::from_vec(result.payload.into_vec()),
            diagnostics,
        })
    }
}

impl Drop for LoadedExtensionHookProvider {
    fn drop(&mut self) {
        if let Ok(session) = self.session.get_mut() {
            if let Some(session) = session.take() {
                if let Err(error) = extension_result((self.module.close_session())(session)) {
                    tracing::error!(
                        event = "astra.emu.extension.close_failed",
                        diagnostic_code = error.code()
                    );
                }
            }
        }
        let instance = FfiExtensionInstanceRequest {
            instance_id: self.instance_id.clone().into(),
        };
        if let Err(error) = extension_result((self.module.destroy_instance())(instance)) {
            tracing::error!(
                event = "astra.emu.extension.destroy_failed",
                diagnostic_code = error.code()
            );
        }
    }
}

unsafe fn root_module(
    library: &Library,
) -> Result<AstraEmuExtensionModuleRef, LegacyProviderError> {
    let header = library
        .get::<AbiHeaderRef>(ROOT_MODULE_LOADER_NAME_WITH_NUL.as_bytes())
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_EXTENSION_ROOT_SYMBOL",
                "extension root symbol is missing",
            )
        })?;
    let header = (*header).upgrade().map_err(|_| {
        invalid(
            "ASTRA_EMU_EXTENSION_ABI_HEADER",
            "extension ABI header is invalid",
        )
    })?;
    header
        .init_root_module::<AstraEmuExtensionModuleRef>()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_EXTENSION_ABI_REJECTED",
                "extension module is not compatible with Extension ABI v1",
            )
        })
}

fn extension_result<T>(result: RResult<T, FfiExtensionError>) -> Result<T, LegacyProviderError> {
    match result {
        RResult::ROk(value) => Ok(value),
        RResult::RErr(error) => Err(extension_error("provider", error)),
    }
}

fn extension_error(operation: &str, _error: FfiExtensionError) -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_EMU_EXTENSION_PROVIDER_FAILED",
        format!("extension {operation} failed"),
    )
}

fn invalid(code: &'static str, message: &str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

fn safe_diagnostic_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_extension_library_fails_fast() {
        let error = LoadedExtensionHookProvider::load(
            "definitely-missing-astra-emu-extension",
            "extension.instance",
            "fvp",
            "game.1",
        )
        .err()
        .expect("missing extension must fail");
        assert_eq!(error.code(), "ASTRA_EMU_EXTENSION_LIBRARY_LOAD");
    }

    #[test]
    fn diagnostic_codes_are_bounded_machine_identities() {
        assert!(safe_diagnostic_code("ASTRA_EMU_TRANSLATION_TIMEOUT"));
        assert!(!safe_diagnostic_code("contains text"));
        assert!(!safe_diagnostic_code(""));
    }
}
