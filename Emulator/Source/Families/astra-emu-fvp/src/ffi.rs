use std::panic::{catch_unwind, AssertUnwindSafe};

use abi_stable::prefix_type::PrefixTypeTrait;
use astra_emu_family_api::{
    ffi_result, AstraLegacyFamilyModule, AstraLegacyFamilyModuleRef, FamilyId,
    FfiFamilyPluginDescriptor, FfiLegacyHostServices, FfiLegacyResult, FfiOpenCall, FfiProbeCall,
    FfiProbeReport, FfiProviderInstanceRequest, FfiSessionCall, FfiShutdownReport, FfiStepCall,
    FfiStepOutput, LegacyFamilyCoreKind, LegacyFamilyPluginDescriptor,
    LegacyFamilyPresentationMode, LegacyProviderError, LEGACY_FAMILY_ABI_FINGERPRINT,
};

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
            tracing::error!(event, "RFVP provider panicked at the dylib boundary");
            ffi_result::<T, U>(Err(LegacyProviderError::invalid(
                "ASTRA_FVP_DYLIB_PANIC",
                "RFVP provider panicked at the dylib boundary",
            )))
        }
    }
}

extern "C" fn descriptor() -> FfiLegacyResult<FfiFamilyPluginDescriptor> {
    boundary("astra.emu.fvp.descriptor", || {
        let descriptor = LegacyFamilyPluginDescriptor {
            family_id: FamilyId("fvp".into()),
            plugin_id: "astra.emu.fvp".into(),
            provider_id: "astra.emu.family.fvp".into(),
            core_kind: LegacyFamilyCoreKind::Ported,
            presentation_mode: LegacyFamilyPresentationMode::SingleLayer,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_FVP_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_FVP_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            supported_formats: vec![
                "fvp.hcb".into(),
                "fvp.bin".into(),
                "fvp.nvsg".into(),
                "fvp.hzc1".into(),
            ],
            permissions: vec![
                "vfs.read".into(),
                "surface.write".into(),
                "hook.invoke".into(),
                "writable_file".into(),
                "media.submit".into(),
            ],
            report_redaction: "astra.emu.redaction.v1".into(),
            license: "MPL-2.0".into(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    })
}

extern "C" fn create_instance(
    services: FfiLegacyHostServices,
    request: FfiProviderInstanceRequest,
) -> FfiLegacyResult<()> {
    boundary("astra.emu.fvp.create_instance", || {
        rfvp_astra_provider::ffi_bridge::create_instance(services, request)
    })
}

extern "C" fn destroy_instance(request: FfiProviderInstanceRequest) -> FfiLegacyResult<()> {
    boundary("astra.emu.fvp.destroy_instance", || {
        rfvp_astra_provider::ffi_bridge::destroy_instance(request)
    })
}

extern "C" fn probe(call: FfiProbeCall) -> FfiLegacyResult<FfiProbeReport> {
    boundary("astra.emu.fvp.probe", || {
        rfvp_astra_provider::ffi_bridge::probe(call)
    })
}

extern "C" fn open(call: FfiOpenCall) -> FfiLegacyResult<abi_stable::std_types::RString> {
    boundary("astra.emu.fvp.open", || {
        rfvp_astra_provider::ffi_bridge::open(call)
    })
}

extern "C" fn step(call: FfiStepCall) -> FfiLegacyResult<FfiStepOutput> {
    boundary("astra.emu.fvp.step", || {
        rfvp_astra_provider::ffi_bridge::step(call)
    })
}

extern "C" fn shutdown(call: FfiSessionCall) -> FfiLegacyResult<FfiShutdownReport> {
    boundary("astra.emu.fvp.shutdown", || {
        rfvp_astra_provider::ffi_bridge::shutdown(call)
    })
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
