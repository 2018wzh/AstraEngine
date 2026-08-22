use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{prefix_type::PrefixTypeTrait, std_types::RString};
use astra_emu_family_api::{
    ffi_result, validate_symbol, AstraLegacyFamilyModule, AstraLegacyFamilyModuleRef,
    FfiFamilyPluginDescriptor, FfiLegacyFamilyHostAdapter, FfiLegacyHostServices, FfiLegacyResult,
    FfiOpenCall, FfiProbeCall, FfiProbeReport, FfiProviderInstanceRequest, FfiSessionCall,
    FfiShutdownReport, FfiStepCall, FfiStepOutput, LegacyProviderError, LegacyRuntimeProvider,
    LegacyRuntimeSessionId,
};

use crate::{minori_descriptor, MinoriRuntimeProvider};

type SharedProvider = Arc<Mutex<MinoriRuntimeProvider>>;
static PROVIDERS: OnceLock<Mutex<BTreeMap<String, SharedProvider>>> = OnceLock::new();

fn boundary<T, U>(
    event: &'static str,
    action: impl FnOnce() -> Result<T, LegacyProviderError>,
) -> FfiLegacyResult<U>
where
    T: Into<U>,
{
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => ffi_result(result),
        Err(_) => {
            tracing::error!(event, "Minori provider panicked at the dylib boundary");
            ffi_result::<T, U>(Err(invalid(
                "ASTRA_EMU_MINORI_DYLIB_PANIC",
                "Minori provider panicked at the dylib boundary",
            )))
        }
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
    boundary("astra.emu.minori.descriptor", || {
        let descriptor = minori_descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    })
}

extern "C" fn create_instance(
    services: FfiLegacyHostServices,
    request: FfiProviderInstanceRequest,
) -> FfiLegacyResult<()> {
    boundary("astra.emu.minori.create_instance", || {
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
            Arc::new(Mutex::new(MinoriRuntimeProvider::new(services))),
        );
        Ok(())
    })
}

extern "C" fn destroy_instance(request: FfiProviderInstanceRequest) -> FfiLegacyResult<()> {
    boundary("astra.emu.minori.destroy_instance", || {
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
    })
}

extern "C" fn probe(call: FfiProbeCall) -> FfiLegacyResult<FfiProbeReport> {
    boundary("astra.emu.minori.probe", || {
        let provider = provider(call.instance_id.as_str())?;
        let result = provider
            .lock()
            .map_err(|_| lock_error())?
            .probe(&call.ctx.into(), call.request.into())?;
        Ok(FfiProbeReport::from(result))
    })
}

extern "C" fn open(call: FfiOpenCall) -> FfiLegacyResult<RString> {
    boundary("astra.emu.minori.open", || {
        let provider = provider(call.instance_id.as_str())?;
        let result = provider
            .lock()
            .map_err(|_| lock_error())?
            .open(&call.ctx.into(), call.request.try_into()?)?;
        Ok(RString::from(result.0))
    })
}

extern "C" fn step(call: FfiStepCall) -> FfiLegacyResult<FfiStepOutput> {
    boundary("astra.emu.minori.step", || {
        let provider = provider(call.instance_id.as_str())?;
        let output = provider.lock().map_err(|_| lock_error())?.step(
            &call.ctx.into(),
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            call.input.into(),
        )?;
        FfiStepOutput::try_from(output)
    })
}

extern "C" fn shutdown(call: FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport> {
    boundary("astra.emu.minori.shutdown", || {
        let provider = provider(call.instance_id.as_str())?;
        let result = provider.lock().map_err(|_| lock_error())?.shutdown(
            &call.ctx.into(),
            &LegacyRuntimeSessionId(call.session_id.to_string()),
        )?;
        Ok(FfiShutdownReport::from(result))
    })
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
