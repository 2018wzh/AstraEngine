use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{ROption, RResult, RString},
};
use astra_emu_family_api::{
    bulk_bytes_from_vec, ffi_result, native_result, validate_symbol, AstraLegacyFamilyModule,
    AstraLegacyFamilyModuleRef, FfiBulkBytes, FfiEphemeralText, FfiFamilyPluginDescriptor,
    FfiLegacyHostServices, FfiLegacyResult, FfiOpenCall, FfiProbeCall, FfiProbeReport,
    FfiProviderInstanceRequest, FfiResourceReadCall, FfiRestoreCall, FfiRestoreReport,
    FfiSessionCall, FfiShutdownReport, FfiSnapshotEnvelope, FfiStepCall, FfiStepOutput,
    FfiTextLeaseCall, FfiVfsEnumerateCall, FfiVfsRangeCall, FfiVfsStatCall, LegacyProviderError,
    LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacyVfsListedFile, LegacyVfsReader,
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
        native_result((self.services.stat_vfs)(
            self.services.host_token.clone(),
            FfiVfsStatCall {
                mount_set_id: mount_set_id.into(),
                uri: uri.into(),
            },
        ))
    }

    fn read_file_range(
        &self,
        mount_set_id: &str,
        uri: &str,
        expected_revision: astra_byte_source::SourceRevision,
        range: astra_byte_source::ByteRange,
        max_bytes: u64,
    ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError> {
        let result: astra_byte_source::RangeReadResult =
            native_result((self.services.read_vfs_range)(
                self.services.host_token.clone(),
                FfiVfsRangeCall {
                    mount_set_id: mount_set_id.into(),
                    uri: uri.into(),
                    expected_revision: expected_revision.0.into(),
                    range: range.into(),
                    max_bytes,
                },
            ))?;
        if result.bytes.len() as u64 != range.len || result.bytes.len() as u64 > max_bytes {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FFI_VFS_BOUNDS",
                "host VFS range length is invalid",
            ));
        }
        Ok(result)
    }

    fn enumerate_by_extension(
        &self,
        mount_set_id: &str,
        root: &str,
        extension_without_dot: &str,
        max_entries: u32,
    ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
        let entries = match (self.services.enumerate_vfs)(
            self.services.host_token.clone(),
            FfiVfsEnumerateCall {
                mount_set_id: mount_set_id.into(),
                root: root.into(),
                extension_without_dot: extension_without_dot.into(),
                max_entries,
            },
        ) {
            RResult::ROk(entries) => entries.iter().cloned().map(Into::into).collect::<Vec<_>>(),
            RResult::RErr(error) => return Err(error.into()),
        };
        if entries.len() > max_entries as usize {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FFI_VFS_ENUM_BOUNDS",
                "host VFS enumeration exceeded the requested bound",
            ));
        }
        Ok(entries)
    }
}

fn providers() -> &'static Mutex<BTreeMap<String, MinoriRuntimeProvider>> {
    PROVIDERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

extern "C" fn descriptor() -> FfiLegacyResult<FfiFamilyPluginDescriptor> {
    RResult::ROk(MinoriRuntimeProvider::default().descriptor().into())
}

extern "C" fn create_instance(
    services: FfiLegacyHostServices,
    request: FfiProviderInstanceRequest,
) -> FfiLegacyResult<()> {
    ffi_result((|| {
        let instance_id = request.instance_id.to_string();
        validate_symbol("instance_id", &instance_id)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        if providers.contains_key(&instance_id) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_INSTANCE_DUPLICATE",
                "provider instance id is already active",
            ));
        }
        providers.insert(
            instance_id,
            MinoriRuntimeProvider::with_vfs(Arc::new(FfiVfsReader { services })),
        );
        Ok(())
    })())
}

extern "C" fn destroy_instance(request: FfiProviderInstanceRequest) -> FfiLegacyResult<()> {
    ffi_result((|| {
        let instance_id = request.instance_id.to_string();
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers.get(&instance_id).ok_or_else(instance_missing)?;
        if provider.has_active_sessions() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_INSTANCE_ACTIVE_SESSIONS",
                "provider instance still owns active sessions",
            ));
        }
        providers.remove(&instance_id);
        Ok(())
    })())
}

extern "C" fn probe(call: FfiProbeCall) -> FfiLegacyResult<FfiProbeReport> {
    ffi_result((|| {
        let providers = providers().lock().map_err(|_| lock_error())?;
        providers
            .get(call.instance_id.as_str())
            .ok_or_else(instance_missing)?
            .probe(&call.ctx.into(), call.request.into())
    })())
}

extern "C" fn open(call: FfiOpenCall) -> FfiLegacyResult<RString> {
    ffi_result((|| {
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(call.instance_id.as_str())
            .ok_or_else(instance_missing)?;
        provider
            .open(&call.ctx.into(), call.request.try_into()?)
            .map(|session| session.0)
    })())
}

extern "C" fn step(call: FfiStepCall) -> FfiLegacyResult<FfiStepOutput> {
    ffi_result((|| {
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(call.instance_id.as_str())
            .ok_or_else(instance_missing)?;
        provider.step(
            &call.ctx.into(),
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            call.input.into(),
        )
    })())
}

extern "C" fn save(call: FfiSessionCall) -> FfiLegacyResult<FfiSnapshotEnvelope> {
    ffi_result(with_session_mut(call, |provider, ctx, session| {
        provider.save(&ctx, &session)
    }))
}

extern "C" fn restore(call: FfiRestoreCall) -> FfiLegacyResult<FfiRestoreReport> {
    ffi_result((|| {
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(call.instance_id.as_str())
            .ok_or_else(instance_missing)?;
        provider.restore(
            &call.ctx.into(),
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            &call.snapshot.into(),
        )
    })())
}

extern "C" fn shutdown(call: FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport> {
    ffi_result(with_session_mut(call, |provider, ctx, session| {
        provider.shutdown(&ctx, &session)
    }))
}

extern "C" fn take_ephemeral_text(
    call: FfiTextLeaseCall,
) -> FfiLegacyResult<ROption<FfiEphemeralText>> {
    ffi_result::<ROption<FfiEphemeralText>, ROption<FfiEphemeralText>>((|| {
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(call.instance_id.as_str())
            .ok_or_else(instance_missing)?;
        provider
            .take_ephemeral_text(
                &call.ctx.into(),
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                call.lease_id.as_str(),
            )
            .map(|value| value.map(FfiEphemeralText::from).into())
    })())
}

extern "C" fn read_session_resource(call: FfiResourceReadCall) -> FfiLegacyResult<FfiBulkBytes> {
    ffi_result((|| {
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get_mut(call.instance_id.as_str())
            .ok_or_else(instance_missing)?;
        provider
            .read_session_resource(
                &call.ctx.into(),
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                call.resource_uri.as_str(),
                call.max_bytes,
            )
            .map(bulk_bytes_from_vec)
    })())
}

fn with_session_mut<T>(
    call: FfiSessionCall,
    action: impl FnOnce(
        &mut MinoriRuntimeProvider,
        astra_emu_family_api::LegacyRuntimeHostCtx,
        LegacyRuntimeSessionId,
    ) -> Result<T, LegacyProviderError>,
) -> Result<T, LegacyProviderError> {
    let mut providers = providers().lock().map_err(|_| lock_error())?;
    let provider = providers
        .get_mut(call.instance_id.as_str())
        .ok_or_else(instance_missing)?;
    action(
        provider,
        call.ctx.into(),
        LegacyRuntimeSessionId(call.session_id.to_string()),
    )
}

fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

fn lock_error() -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_INSTANCE_LOCK_POISONED",
        "provider instance registry lock is poisoned",
    )
}

fn instance_missing() -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_INSTANCE_MISSING",
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
