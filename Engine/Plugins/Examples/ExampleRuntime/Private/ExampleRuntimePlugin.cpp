#include <Astra/ModuleRuntime/AstraModuleABI.h>

#include <cstring>

namespace {

AstraStringView view(const char* value) {
    return {value, value == nullptr ? 0u : static_cast<uint64_t>(std::strlen(value))};
}

AstraResultCode register_extension(const AstraModuleHostApi* host, const char* id,
                                   AstraExtensionKind kind, const char* capability) {
    AstraExtensionDescriptor descriptor{};
    descriptor.id = view(id);
    descriptor.kind = kind;
    descriptor.required_capability = view(capability);
    descriptor.required_permission = {nullptr, 0};
    return host->register_extension(host->host_context, &descriptor);
}

AstraResultCode initialize(void*, const AstraModuleHostApi* host) {
    if (host == nullptr || host->register_extension == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    if (register_extension(host, "astra.example.runtime_command_source",
                           ASTRA_EXTENSION_RUNTIME_COMMAND_SOURCE,
                           "runtime_command_source") != ASTRA_RESULT_OK) {
        return ASTRA_RESULT_ERROR;
    }
    return register_extension(host, "astra.example.vn_property_type",
                              ASTRA_EXTENSION_VN_PROPERTY_TYPE, "vn_property_type");
}

AstraResultCode activate(void*, const AstraModuleHostApi*) {
    return ASTRA_RESULT_OK;
}

AstraResultCode deactivate(void*, const AstraModuleHostApi*) {
    return ASTRA_RESULT_OK;
}

AstraResultCode shutdown(void*, const AstraModuleHostApi*) {
    return ASTRA_RESULT_OK;
}

} // namespace

extern "C" ASTRA_MODULE_EXPORT AstraResultCode astra_module_main(const AstraModuleHostApi* host,
                                                                 AstraModuleApi* out_module) {
    if (host == nullptr || out_module == nullptr || host->abi_version != ASTRA_MODULE_ABI_VERSION) {
        return ASTRA_RESULT_UNSUPPORTED_ABI;
    }
    out_module->abi_version = ASTRA_MODULE_ABI_VERSION;
    out_module->module_id = view("example_runtime.runtime");
    out_module->module_context = nullptr;
    out_module->initialize = initialize;
    out_module->activate = activate;
    out_module->deactivate = deactivate;
    out_module->shutdown = shutdown;
    return ASTRA_RESULT_OK;
}
