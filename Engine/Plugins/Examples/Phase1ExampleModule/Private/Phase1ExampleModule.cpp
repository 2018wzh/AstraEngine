#include <Astra/ModuleRuntime/AstraModuleABI.h>
#include <Astra/Platform/PlatformServices.h>

#include <cstring>

namespace {

constexpr const char* kModuleId = "phase1_example.runtime";
constexpr const char* kServiceExtensionId = "phase1_example.service_extension";
constexpr const char* kPropertyTypeProviderId = "phase1_example.property_type_provider";

AstraStringView to_view(const char* value) {
    return AstraStringView{value, static_cast<uint64_t>(std::strlen(value))};
}

AstraResultCode register_extension(const AstraModuleHostApi* host, const char* id,
                                   AstraExtensionKind kind, const char* capability) {
    AstraExtensionDescriptor descriptor{};
    descriptor.id = to_view(id);
    descriptor.kind = kind;
    descriptor.required_capability = to_view(capability);
    descriptor.required_permission = AstraStringView{nullptr, 0};
    return host->register_extension(host->host_context, &descriptor);
}

AstraResultCode initialize_module(void*, const AstraModuleHostApi* host) {
    if (host == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }

    AstraOpaqueHandle timer_service = nullptr;
    if (host->get_service(host->host_context, to_view(astra::kTimerServiceId), &timer_service) !=
            ASTRA_RESULT_OK ||
        timer_service == nullptr) {
        return ASTRA_RESULT_NOT_FOUND;
    }

    if (register_extension(host, kServiceExtensionId, ASTRA_EXTENSION_SERVICE_EXTENSION,
                           "service_extension") != ASTRA_RESULT_OK) {
        return ASTRA_RESULT_ERROR;
    }
    if (register_extension(host, kPropertyTypeProviderId,
                           ASTRA_EXTENSION_PROPERTY_TYPE_PROVIDER,
                           "property_type_provider") != ASTRA_RESULT_OK) {
        return ASTRA_RESULT_ERROR;
    }
    return ASTRA_RESULT_OK;
}

AstraResultCode activate_module(void*, const AstraModuleHostApi*) {
    return ASTRA_RESULT_OK;
}

AstraResultCode deactivate_module(void*, const AstraModuleHostApi*) {
    return ASTRA_RESULT_OK;
}

AstraResultCode shutdown_module(void*, const AstraModuleHostApi*) {
    return ASTRA_RESULT_OK;
}

} // namespace

extern "C" ASTRA_MODULE_EXPORT AstraResultCode
astra_module_main(const AstraModuleHostApi* host, AstraModuleApi* out_module) {
    if (host == nullptr || out_module == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    if (host->abi_version != ASTRA_MODULE_ABI_VERSION) {
        return ASTRA_RESULT_UNSUPPORTED_ABI;
    }

    out_module->abi_version = ASTRA_MODULE_ABI_VERSION;
    out_module->module_id = to_view(kModuleId);
    out_module->module_context = nullptr;
    out_module->initialize = &initialize_module;
    out_module->activate = &activate_module;
    out_module->deactivate = &deactivate_module;
    out_module->shutdown = &shutdown_module;
    return ASTRA_RESULT_OK;
}
