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
    LegacyTextLeaseCall, LegacyVfsRangeCall, LegacyVfsReader, LegacyVfsStatCall,
};

use crate::MinoriRuntimeProvider;

static PROVIDERS: OnceLock<Mutex<BTreeMap<String, MinoriRuntimeProvider>>> = OnceLock::new();

#[derive(Clone)]
struct FfiVfsReader {
    services: FfiLegacyHostServices,
}

impl LegacyVfsReader for FfiVfsReader {
    fn stat_file(
        &self,
        mount_set_id: &str,
        uri: &str,
    ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
        let payload = postcard::to_allocvec(&LegacyVfsStatCall {
            mount_set_id: mount_set_id.into(),
            uri: uri.into(),
        })
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_FFI_ENCODE",
                "VFS stat call encoding failed",
            )
        })?;
        (self.services.stat_vfs)(self.services.host_token.clone(), payload.into()).decode()
    }

    fn read_file_range(
        &self,
        mount_set_id: &str,
        uri: &str,
        expected_revision: astra_byte_source::SourceRevision,
        range: astra_byte_source::ByteRange,
        max_bytes: u64,
    ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError> {
        let payload = postcard::to_allocvec(&LegacyVfsRangeCall {
            mount_set_id: mount_set_id.into(),
            uri: uri.into(),
            expected_revision,
            range,
            max_bytes,
        })
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_FFI_ENCODE",
                "VFS range call encoding failed",
            )
        })?;
        let result: astra_byte_source::RangeReadResult =
            (self.services.read_vfs_range)(self.services.host_token.clone(), payload.into())
                .decode()?;
        if result.bytes.len() as u64 != range.len || result.bytes.len() as u64 > max_bytes {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FFI_VFS_BOUNDS",
                "host VFS returned an invalid range",
            ));
        }
        Ok(result)
    }
}

fn providers() -> &'static Mutex<BTreeMap<String, MinoriRuntimeProvider>> {
    PROVIDERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

extern "C" fn descriptor(_: RVec<u8>) -> FfiLegacyResult {
    FfiLegacyResult::success(&MinoriRuntimeProvider::default().descriptor())
}

extern "C" fn create_instance(
    services: FfiLegacyHostServices,
    payload: RVec<u8>,
) -> FfiLegacyResult {
    result_to_ffi((|| {
        let request: LegacyProviderInstanceRequest = decode_ffi_request(payload)?;
        validate_symbol("instance_id", &request.instance_id)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        if providers.contains_key(&request.instance_id) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_INSTANCE_DUPLICATE",
                "provider instance is already active",
            ));
        }
        providers.insert(
            request.instance_id,
            MinoriRuntimeProvider::with_vfs(Arc::new(FfiVfsReader { services })),
        );
        Ok(())
    })())
}

extern "C" fn destroy_instance(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        let request: LegacyProviderInstanceRequest = decode_ffi_request(payload)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get(&request.instance_id)
            .ok_or_else(instance_missing)?;
        if provider.has_active_sessions() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_INSTANCE_ACTIVE_SESSIONS",
                "provider instance still owns active sessions",
            ));
        }
        providers.remove(&request.instance_id);
        Ok(())
    })())
}

macro_rules! invoke {
    ($payload:ident, $call:ty, $body:expr) => {{
        let call: $call = decode_ffi_request($payload)?;
        let mut guard = providers().lock().map_err(|_| lock_error())?;
        let provider = guard
            .get_mut(&call.instance_id)
            .ok_or_else(instance_missing)?;
        $body(provider, call)
    }};
}

extern "C" fn probe(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacyProbeCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacyProbeCall| provider
                .probe(&call.ctx, call.request)
        )
    })())
}
extern "C" fn open(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacyOpenCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacyOpenCall| provider
                .open(&call.ctx, call.request)
        )
    })())
}
extern "C" fn step(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacyStepCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacyStepCall| provider.step(
                &call.ctx,
                &call.session_id,
                call.input
            )
        )
    })())
}
extern "C" fn save(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacySessionCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacySessionCall| provider
                .save(&call.ctx, &call.session_id)
        )
    })())
}
extern "C" fn restore(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacyRestoreCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacyRestoreCall| provider.restore(
                &call.ctx,
                &call.session_id,
                &call.snapshot
            )
        )
    })())
}
extern "C" fn take_ephemeral_text(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacyTextLeaseCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacyTextLeaseCall| provider
                .take_ephemeral_text(&call.ctx, &call.session_id, &call.lease_id)
                .map(|value| value.map(FfiLegacyEphemeralText::from))
        )
    })())
}
extern "C" fn read_session_resource(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacyResourceReadCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacyResourceReadCall| provider
                .read_session_resource(
                    &call.ctx,
                    &call.session_id,
                    &call.resource_uri,
                    call.max_bytes
                )
        )
    })())
}
extern "C" fn shutdown(payload: RVec<u8>) -> FfiLegacyResult {
    result_to_ffi((|| {
        invoke!(
            payload,
            LegacySessionCall,
            |provider: &mut MinoriRuntimeProvider, call: LegacySessionCall| provider
                .shutdown(&call.ctx, &call.session_id)
        )
    })())
}

fn result_to_ffi<T: serde::Serialize>(result: Result<T, LegacyProviderError>) -> FfiLegacyResult {
    match result {
        Ok(value) => FfiLegacyResult::success(&value),
        Err(error) => FfiLegacyResult::failure(error),
    }
}
fn lock_error() -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_INSTANCE_LOCK_POISONED",
        "provider registry lock is poisoned",
    )
}
fn instance_missing() -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_INSTANCE_MISSING",
        "provider instance is not active",
    )
}
fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
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
