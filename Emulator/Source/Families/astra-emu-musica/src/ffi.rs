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

use crate::MusicaRuntimeProvider;

type SharedProvider = Arc<Mutex<MusicaRuntimeProvider>>;

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

/// Keep panics inside the signed plugin.  The FFI contract is a C ABI and a
/// panic must become an explicit provider error instead of unwinding through
/// the host stack.  This mirrors the FVP boundary and keeps lifecycle calls
/// observable without introducing a recovery or alternate provider path.
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
            tracing::error!(event, "Musica provider panicked at the dylib boundary");
            ffi_result::<T, U>(Err(invalid(
                "ASTRA_EMU_MUSICA_DYLIB_PANIC",
                "Musica provider panicked at the dylib boundary",
            )))
        }
    }
}

extern "C" fn descriptor() -> FfiLegacyResult<FfiFamilyPluginDescriptor> {
    boundary("astra.emu.musica.descriptor", || {
        let descriptor = MusicaRuntimeProvider::default().descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    })
}

extern "C" fn create_instance(
    services: FfiLegacyHostServices,
    request: FfiProviderInstanceRequest,
) -> FfiLegacyResult<()> {
    boundary("astra.emu.musica.create_instance", || {
        let instance_id = request.instance_id.to_string();
        validate_symbol("instance_id", &instance_id)?;
        let mut providers = providers().lock().map_err(|_| lock_error())?;
        if providers.contains_key(&instance_id) {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_INSTANCE_DUPLICATE",
                "provider instance id is already active",
            ));
        }
        let services = FfiLegacyFamilyHostAdapter::new(services).into_host_services();
        providers.insert(
            instance_id,
            Arc::new(Mutex::new(MusicaRuntimeProvider::with_host_services(
                services,
            ))),
        );
        Ok(())
    })
}

extern "C" fn destroy_instance(request: FfiProviderInstanceRequest) -> FfiLegacyResult<()> {
    boundary("astra.emu.musica.destroy_instance", || {
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
                "ASTRA_EMU_MUSICA_INSTANCE_ACTIVE_SESSIONS",
                "provider instance still owns active sessions",
            ));
        }
        providers.remove(&instance_id);
        Ok(())
    })
}

extern "C" fn probe(call: FfiProbeCall) -> FfiLegacyResult<FfiProbeReport> {
    boundary("astra.emu.musica.probe", || {
        let provider = provider(call.instance_id.as_str())?;
        let provider = provider.lock().map_err(|_| lock_error())?;
        provider.probe(&call.ctx.into(), call.request.into())
    })
}

extern "C" fn open(call: FfiOpenCall) -> FfiLegacyResult<RString> {
    boundary("astra.emu.musica.open", || {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider
            .open(&call.ctx.into(), call.request.try_into()?)
            .map(|session| session.0)
    })
}

extern "C" fn step(call: FfiStepCall) -> FfiLegacyResult<FfiStepOutput> {
    boundary("astra.emu.musica.step", || {
        let provider = provider(call.instance_id.as_str())?;
        let mut provider = provider.lock().map_err(|_| lock_error())?;
        provider
            .step(
                &call.ctx.into(),
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                call.input.into(),
            )
            .and_then(FfiStepOutput::try_from)
    })
}

extern "C" fn shutdown(call: FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport> {
    boundary("astra.emu.musica.shutdown", || {
        with_session_mut(call, |provider, ctx, session| {
            provider.shutdown(&ctx, &session)
        })
    })
}

fn with_session_mut<T>(
    call: FfiSessionCall,
    action: impl FnOnce(
        &mut MusicaRuntimeProvider,
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
        "ASTRA_EMU_MUSICA_INSTANCE_LOCK_POISONED",
        "provider instance registry lock is poisoned",
    )
}

fn instance_missing() -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MUSICA_INSTANCE_MISSING",
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

#[cfg(test)]
mod tests {
    use abi_stable::std_types::RResult;
    use astra_emu_family_api::LegacyFamilyPluginDescriptor;

    use super::*;

    #[test]
    fn boundary_converts_provider_panic_to_stable_error() {
        let result: FfiLegacyResult<()> = boundary(
            "astra.emu.musica.test_boundary",
            || -> Result<(), LegacyProviderError> { panic!("test panic") },
        );
        match result {
            RResult::RErr(error) => {
                assert_eq!(error.code.as_str(), "ASTRA_EMU_MUSICA_DYLIB_PANIC");
                assert_eq!(
                    error.message.as_str(),
                    "Musica provider panicked at the dylib boundary"
                );
            }
            RResult::ROk(()) => panic!("panic boundary unexpectedly returned success"),
        }
    }

    #[test]
    fn descriptor_is_validated_before_crossing_the_boundary() {
        match descriptor() {
            RResult::ROk(value) => {
                let descriptor: LegacyFamilyPluginDescriptor = value.into();
                descriptor.validate().unwrap();
            }
            RResult::RErr(error) => panic!("descriptor rejected: {}", error.code),
        }
    }
}
