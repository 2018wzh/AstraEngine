use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{ROption, RResult, RString},
};
use astra_byte_source::FfiOwnedByteBuffer;
use astra_emu_family_api::{
    ffi_result, native_result, validate_symbol, AstraLegacyFamilyModule,
    AstraLegacyFamilyModuleRef, FfiEphemeralText, FfiFamilyPluginDescriptor, FfiLegacyHostServices,
    FfiLegacyResult, FfiOpenCall, FfiPrivateMaterialCallV8, FfiProbeCall, FfiProbeReport,
    FfiProviderInstanceRequest, FfiResourceReadCall, FfiRestoreCall, FfiRestoreReport,
    FfiSecretBufferV8, FfiSessionCall, FfiShutdownReport, FfiSnapshotEnvelope, FfiStepCall,
    FfiStepOutput, FfiTextLeaseCall, FfiVfsEnumerateCall, FfiVfsRangeCall, FfiVfsStatCall,
    LegacyPrivateMaterialHostV8, LegacyPrivateMaterialRequestV8, LegacyProviderError,
    LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacySecretBufferV8, LegacyVfsListedFile,
    LegacyVfsReader,
};

use crate::{provider::siglus_descriptor, SiglusRuntimeProvider};

type SharedProvider = Arc<Mutex<SiglusRuntimeProvider>>;

static PROVIDERS: OnceLock<Mutex<BTreeMap<String, SharedProvider>>> = OnceLock::new();

#[derive(Clone)]
struct FfiHostServices {
    services: FfiLegacyHostServices,
}

impl LegacyVfsReader for FfiHostServices {
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
                    expected_revision: expected_revision.0,
                    range: range.into(),
                    max_bytes,
                },
            ))?;
        if result.bytes.len() as u64 != range.len || result.bytes.len() as u64 > max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_SIGLUS_FFI_VFS_BOUNDS",
                "host VFS returned a range with invalid length",
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
        let entries: Vec<LegacyVfsListedFile> = match (self.services.enumerate_vfs)(
            self.services.host_token.clone(),
            FfiVfsEnumerateCall {
                mount_set_id: mount_set_id.into(),
                root: root.into(),
                extension_without_dot: extension_without_dot.into(),
                max_entries,
            },
        ) {
            RResult::ROk(entries) => entries.iter().cloned().map(Into::into).collect(),
            RResult::RErr(error) => return Err(error.into()),
        };
        if entries.len() > max_entries as usize {
            return Err(LegacyProviderError::invalid(
                "ASTRA_SIGLUS_FFI_VFS_ENUM_BOUNDS",
                "host VFS returned more entries than requested",
            ));
        }
        Ok(entries)
    }
}

impl LegacyPrivateMaterialHostV8 for FfiHostServices {
    fn read_private_material(
        &self,
        session: &LegacyRuntimeSessionId,
        request: LegacyPrivateMaterialRequestV8,
    ) -> Result<LegacySecretBufferV8, LegacyProviderError> {
        let result: FfiSecretBufferV8 = native_result((self.services.read_private_material)(
            self.services.host_token.clone(),
            FfiPrivateMaterialCallV8 {
                session_id: session.0.clone().into(),
                secret_id: request.secret_id.into(),
                exact_len: request.exact_len,
            },
        ))?;
        if result.bytes.len() != request.exact_len as usize {
            return Err(LegacyProviderError::invalid(
                "ASTRA_SIGLUS_FFI_PRIVATE_MATERIAL_BOUNDS",
                "host private material length does not match the exact request",
            ));
        }
        Ok(LegacySecretBufferV8::new(result.bytes.as_slice().to_vec()))
    }
}

fn providers() -> &'static Mutex<BTreeMap<String, SharedProvider>> {
    PROVIDERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn provider(instance_id: &str) -> Result<SharedProvider, LegacyProviderError> {
    providers()
        .lock()
        .map_err(|_| lock_error())?
        .get(instance_id)
        .cloned()
        .ok_or_else(instance_missing)
}

extern "C" fn descriptor() -> FfiLegacyResult<FfiFamilyPluginDescriptor> {
    RResult::ROk(siglus_descriptor().into())
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
            return Err(LegacyProviderError::invalid(
                "ASTRA_SIGLUS_INSTANCE_DUPLICATE",
                "provider instance id is already active",
            ));
        }
        let host = Arc::new(FfiHostServices { services });
        providers.insert(
            instance_id,
            Arc::new(Mutex::new(SiglusRuntimeProvider::new(
                host.clone(),
                Some(host),
            ))),
        );
        Ok(())
    })())
}

extern "C" fn destroy_instance(request: FfiProviderInstanceRequest) -> FfiLegacyResult<()> {
    ffi_result((|| {
        let instance_id = request.instance_id.to_string();
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        let provider = providers
            .get(&instance_id)
            .cloned()
            .ok_or_else(instance_missing)?;
        if provider
            .lock()
            .map_err(|_| lock_error())?
            .has_active_sessions()
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_SIGLUS_INSTANCE_ACTIVE_SESSIONS",
                "provider instance still owns active sessions",
            ));
        }
        providers.remove(&instance_id);
        Ok(())
    })())
}

extern "C" fn probe(call: FfiProbeCall) -> FfiLegacyResult<FfiProbeReport> {
    ffi_result((|| {
        let provider = provider(call.instance_id.as_str())?;
        let provider = provider.lock().map_err(|_| lock_error())?;
        provider.probe(&call.ctx.into(), call.request.into())
    })())
}

extern "C" fn open(call: FfiOpenCall) -> FfiLegacyResult<RString> {
    ffi_result((|| {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider
            .open(&call.ctx.into(), call.request.try_into()?)
            .map(|session| RString::from(session.0))
    })())
}

extern "C" fn step(call: FfiStepCall) -> FfiLegacyResult<FfiStepOutput> {
    ffi_result((|| {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider
            .step(
                &call.ctx.into(),
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                call.input.into(),
            )
            .and_then(FfiStepOutput::try_from)
    })())
}

extern "C" fn save(call: FfiSessionCall) -> FfiLegacyResult<FfiSnapshotEnvelope> {
    ffi_result(with_session_mut(call, |provider, ctx, session| {
        provider.save(&ctx, &session)
    }))
}

extern "C" fn restore(call: FfiRestoreCall) -> FfiLegacyResult<FfiRestoreReport> {
    ffi_result((|| {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider.restore(
            &call.ctx.into(),
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            &call.snapshot.into(),
        )
    })())
}

extern "C" fn take_ephemeral_text(
    call: FfiTextLeaseCall,
) -> FfiLegacyResult<ROption<FfiEphemeralText>> {
    ffi_result::<ROption<FfiEphemeralText>, ROption<FfiEphemeralText>>((|| {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider
            .take_ephemeral_text(
                &call.ctx.into(),
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                call.lease_id.as_str(),
            )
            .map(|value| value.map(FfiEphemeralText::from).into())
    })())
}

extern "C" fn read_session_resource(
    call: FfiResourceReadCall,
) -> FfiLegacyResult<FfiOwnedByteBuffer> {
    ffi_result((|| {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider
            .read_session_resource(
                &call.ctx.into(),
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                call.resource_uri.as_str(),
                call.max_bytes,
            )
            .map(astra_byte_source::OwnedByteBuffer::into_ffi)
    })())
}

extern "C" fn shutdown(call: FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport> {
    ffi_result(with_session_mut(call, |provider, ctx, session| {
        provider.shutdown(&ctx, &session)
    }))
}

fn with_session_mut<T>(
    call: FfiSessionCall,
    action: impl FnOnce(
        &mut SiglusRuntimeProvider,
        astra_emu_family_api::LegacyRuntimeHostCtx,
        LegacyRuntimeSessionId,
    ) -> Result<T, LegacyProviderError>,
) -> Result<T, LegacyProviderError> {
    let provider = provider(call.instance_id.as_str())?;
    let mut provider = provider.lock().map_err(|_| lock_error())?;
    action(
        &mut provider,
        call.ctx.into(),
        LegacyRuntimeSessionId(call.session_id.to_string()),
    )
}

fn lock_error() -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_SIGLUS_INSTANCE_LOCK_POISONED",
        "provider instance registry lock is poisoned",
    )
}

fn instance_missing() -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_SIGLUS_INSTANCE_MISSING",
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
