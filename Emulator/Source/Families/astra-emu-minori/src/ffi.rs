use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{RResult, RString},
};
use astra_emu_family_api::{
    ffi_result, validate_symbol, AstraLegacyFamilyModule, AstraLegacyFamilyModuleRef,
    FfiFamilyPluginDescriptor, FfiLegacyFamilyHostAdapter, FfiLegacyHostServices, FfiLegacyResult,
    FfiOpenCall, FfiProbeCall, FfiProbeReport, FfiProviderInstanceRequest, FfiSessionCall,
    FfiShutdownReport, FfiStepCall, FfiStepOutput, LegacyProviderError, LegacyRuntimeProvider,
    LegacyRuntimeSessionId,
};

use crate::MinoriRuntimeProvider;

type SharedProvider = Arc<Mutex<MinoriRuntimeProvider>>;

static PROVIDERS: OnceLock<Mutex<BTreeMap<String, SharedProvider>>> = OnceLock::new();

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
        let services = FfiLegacyFamilyHostAdapter::new(services).into_host_services();
        providers.insert(
            instance_id,
            Arc::new(Mutex::new(MinoriRuntimeProvider::with_host_services(
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
