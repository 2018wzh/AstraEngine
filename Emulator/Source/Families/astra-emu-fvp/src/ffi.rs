use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{prefix_type::PrefixTypeTrait, std_types::RVec};
use astra_emu_family_api::{
    decode_ffi_request, validate_symbol, AstraLegacyFamilyModule, AstraLegacyFamilyModuleRef,
    FfiLegacyEphemeralText, FfiLegacyHostServices, FfiLegacyResult, LegacyOpenCall,
    LegacyProbeCall, LegacyProviderError, LegacyProviderInstanceRequest, LegacyResourceReadCall,
    LegacyRestoreCall, LegacyRuntimeProvider, LegacySessionCall, LegacyStepCall,
    LegacyTextLeaseCall, LegacyVfsReader,
};

use crate::FvpRuntimeProvider;

static PROVIDERS: OnceLock<Mutex<BTreeMap<String, FvpRuntimeProvider>>> = OnceLock::new();

#[derive(Clone)]
struct FfiVfsReader {
    services: FfiLegacyHostServices,
}

impl LegacyVfsReader for FfiVfsReader {
    fn read_file(
        &self,
        mount_set_id: &str,
        uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, LegacyProviderError> {
        let result = (self.services.read_vfs)(
            self.services.host_token.clone(),
            mount_set_id.into(),
            uri.into(),
            max_bytes,
        );
        let bytes: Vec<u8> = result.decode()?;
        if bytes.len() as u64 > max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_VFS_BOUNDS",
                "host VFS returned more bytes than requested",
            ));
        }
        Ok(bytes)
    }
}

fn providers() -> &'static Mutex<BTreeMap<String, FvpRuntimeProvider>> {
    PROVIDERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

extern "C" fn descriptor(_payload: RVec<u8>) -> FfiLegacyResult {
    FfiLegacyResult::success(&FvpRuntimeProvider::default().descriptor())
}

extern "C" fn create_instance(
    services: FfiLegacyHostServices,
    payload: RVec<u8>,
) -> FfiLegacyResult {
    let result = (|| {
        let request: LegacyProviderInstanceRequest = decode_ffi_request(payload)?;
        validate_symbol("instance_id", &request.instance_id)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        if providers.contains_key(&request.instance_id) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_FVP_INSTANCE_DUPLICATE",
                "provider instance id is already active",
            ));
        }
        providers.insert(
            request.instance_id,
            FvpRuntimeProvider::with_vfs(Arc::new(FfiVfsReader { services })),
        );
        Ok(())
    })();
    result_to_ffi(result)
}

extern "C" fn destroy_instance(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let request: LegacyProviderInstanceRequest = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get(&request.instance_id)
            .ok_or_else(instance_missing)?;
        if provider.has_active_sessions() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_FVP_INSTANCE_ACTIVE_SESSIONS",
                "provider instance still owns active sessions",
            ));
        }
        providers.remove(&request.instance_id);
        Ok(())
    })();
    result_to_ffi(result)
}

extern "C" fn probe(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyProbeCall = decode_ffi_request(payload)?;
        let providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.probe(&call.ctx, call.request)
    })();
    result_to_ffi(result)
}

extern "C" fn open(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyOpenCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.open(&call.ctx, call.request)
    })();
    result_to_ffi(result)
}

extern "C" fn step(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyStepCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.step(&call.ctx, &call.session_id, call.input)
    })();
    result_to_ffi(result)
}

extern "C" fn save(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacySessionCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.save(&call.ctx, &call.session_id)
    })();
    result_to_ffi(result)
}

extern "C" fn restore(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyRestoreCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.restore(&call.ctx, &call.session_id, &call.snapshot)
    })();
    result_to_ffi(result)
}

extern "C" fn shutdown(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacySessionCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.shutdown(&call.ctx, &call.session_id)
    })();
    result_to_ffi(result)
}

extern "C" fn take_ephemeral_text(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyTextLeaseCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider
            .take_ephemeral_text(&call.ctx, &call.session_id, &call.lease_id)
            .map(|value| value.map(FfiLegacyEphemeralText::from))
    })();
    result_to_ffi(result)
}

extern "C" fn read_session_resource(payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyResourceReadCall = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        provider.read_session_resource(
            &call.ctx,
            &call.session_id,
            &call.resource_uri,
            call.max_bytes,
        )
    })();
    result_to_ffi(result)
}

fn result_to_ffi<T: serde::Serialize>(result: Result<T, LegacyProviderError>) -> FfiLegacyResult {
    match result {
        Ok(value) => FfiLegacyResult::success(&value),
        Err(error) => FfiLegacyResult::failure(error),
    }
}

fn lock_error() -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_FVP_INSTANCE_LOCK_POISONED",
        "provider instance registry lock is poisoned",
    )
}

fn instance_missing() -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_FVP_INSTANCE_MISSING",
        "provider instance id is not active",
    )
}

#[abi_stable::export_root_module]
pub fn astra_legacy_family_root_module() -> AstraLegacyFamilyModuleRef {
    AstraLegacyFamilyModule {
        descriptor,
        create_instance,
        destroy_instance,
        probe,
        open,
        step,
        save,
        restore,
        take_ephemeral_text,
        read_session_resource,
        shutdown,
    }
    .leak_into_prefix()
}
