use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{RResult, RString},
};
use astra_emu_family_api::{
    ffi_result, native_result, validate_symbol, AstraLegacyFamilyModule,
    AstraLegacyFamilyModuleRef, FfiAcquireSurfaceCallV9, FfiCommitSurfaceCallV9,
    FfiFamilyPluginDescriptor, FfiHookInvocationV1, FfiLegacyHostServices, FfiLegacyResult,
    FfiOpenCall, FfiProbeCall, FfiProbeReport, FfiProviderInstanceRequest, FfiSessionCall,
    FfiShutdownReport, FfiStepCall, FfiStepOutput, FfiVfsEnumerateCall, FfiVfsRangeCall,
    FfiVfsStatCall, FfiWritableFileCallV1, LegacyFamilyHostServicesV9, LegacyHookInvocationV1,
    LegacyHookResultV1, LegacyProviderError, LegacyRuntimeProvider, LegacyRuntimeSessionId,
    LegacySurfaceCommitV9, LegacySurfaceFormatV9, LegacySurfaceLeaseV9, LegacyVfsListedFile,
    LegacyVfsReader, LegacyWritableFileRequestV1, LegacyWritableFileResultV1,
};

use crate::MinoriRuntimeProvider;

type SharedProvider = Arc<Mutex<MinoriRuntimeProvider>>;

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

impl LegacyFamilyHostServicesV9 for FfiHostServices {
    fn acquire_surface(
        &self,
        session_id: &str,
        fixed_step: u64,
        surface_id: &str,
        width: u32,
        height: u32,
        format: LegacySurfaceFormatV9,
    ) -> Result<LegacySurfaceLeaseV9, LegacyProviderError> {
        let lease = native_result((self.services.acquire_surface)(FfiAcquireSurfaceCallV9 {
            host_token: self.services.host_token.clone(),
            session_id: session_id.into(),
            fixed_step,
            surface_id: surface_id.into(),
            width,
            height,
            format: format.into(),
        }))?;
        lease.try_into()
    }

    fn commit_surface(
        &self,
        session_id: &str,
        fixed_step: u64,
        commit: LegacySurfaceCommitV9,
    ) -> Result<(), LegacyProviderError> {
        commit.validate()?;
        native_result((self.services.commit_surface)(FfiCommitSurfaceCallV9 {
            host_token: self.services.host_token.clone(),
            session_id: session_id.into(),
            fixed_step,
            lease: commit.lease.into(),
            damage: commit.damage.into(),
        }))
    }

    fn invoke_hook(
        &self,
        invocation: LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        native_result((self.services.invoke_hook)(FfiHookInvocationV1 {
            host_token: self.services.host_token.clone(),
            session_id: invocation.session_id.into(),
            invocation_id: invocation.invocation_id.into(),
            family_id: invocation.family_id.into(),
            family_game_id: invocation.family_game_id.into(),
            hook_id: invocation.hook_id.into(),
            timeout_ms: invocation.timeout_ms,
            payload: invocation.payload.into_ffi(),
        }))
        .map(Into::into)
    }

    fn writable_file(
        &self,
        session_id: &str,
        request: LegacyWritableFileRequestV1,
    ) -> Result<LegacyWritableFileResultV1, LegacyProviderError> {
        request.validate()?;
        native_result((self.services.writable_file)(FfiWritableFileCallV1 {
            host_token: self.services.host_token.clone(),
            session_id: session_id.into(),
            request: request.into(),
        }))
        .map(Into::into)
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
        let services = Arc::new(FfiHostServices { services });
        providers.insert(
            instance_id,
            Arc::new(Mutex::new(MinoriRuntimeProvider::with_host_services(
                services.clone(),
                services,
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
            .map(|session| session.0)
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

extern "C" fn shutdown(call: FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport> {
    ffi_result(with_session_mut(call, |provider, ctx, session| {
        provider.shutdown(&ctx, &session)
    }))
}

fn with_session_mut<T>(
    call: FfiSessionCall,
    action: impl FnOnce(
        &mut MinoriRuntimeProvider,
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
        shutdown,
    }
    .leak_into_prefix()
}
