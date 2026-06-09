#include <Astra/ModuleRuntime/ModuleAbi.h>

extern "C" ASTRA_MODULE_EXPORT AstraModuleResult astra_module_main(const AstraModuleHostApi*, AstraModuleApi* out_module) {
    if (out_module == nullptr) {
        return ASTRA_MODULE_ERROR;
    }
    out_module->abi_version = 999;
    out_module->initialize = nullptr;
    out_module->activate = nullptr;
    out_module->deactivate = nullptr;
    out_module->shutdown = nullptr;
    return ASTRA_MODULE_OK;
}
