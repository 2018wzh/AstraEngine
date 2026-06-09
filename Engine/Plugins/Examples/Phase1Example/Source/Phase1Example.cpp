#include <Astra/ModuleRuntime/ModuleAbi.h>

#include <string_view>

namespace {

AstraStringView View(std::string_view text) {
    return {text.data(), static_cast<uint64_t>(text.size())};
}

AstraModuleResult Initialize(void*, const AstraModuleHostApi* host) {
    host->diagnostics.emit(host->diagnostics.user, View("ASTRA_PHASE1_EXAMPLE_INIT"), View("Phase 1 example module initialized."));
    host->services.register_service(host->services.user, View("astra.phase1.example.service"), View("foundation_example"));
    host->extensions.register_extension(host->extensions.user, View("astra.phase1.example.asset_importer"), View("AssetImporter"));
    host->engine_modules.register_provider(host->engine_modules.user, View("astra.renderer2d"), View("astra.phase1.example.renderer2d"));
    return ASTRA_MODULE_OK;
}

AstraModuleResult Activate(void*) {
    return ASTRA_MODULE_OK;
}

AstraModuleResult Deactivate(void*) {
    return ASTRA_MODULE_OK;
}

AstraModuleResult Shutdown(void*) {
    return ASTRA_MODULE_OK;
}

} // namespace

extern "C" ASTRA_MODULE_EXPORT AstraModuleResult astra_module_main(const AstraModuleHostApi* host, AstraModuleApi* out_module) {
    if (host == nullptr || out_module == nullptr || host->abi_version != 1) {
        return ASTRA_MODULE_VERSION_MISMATCH;
    }

    static constexpr std::string_view ModuleId = "phase1.example.runtime";
    out_module->abi_version = 1;
    out_module->module_id = View(ModuleId);
    out_module->module_state = nullptr;
    out_module->initialize = Initialize;
    out_module->activate = Activate;
    out_module->deactivate = Deactivate;
    out_module->shutdown = Shutdown;
    return ASTRA_MODULE_OK;
}
