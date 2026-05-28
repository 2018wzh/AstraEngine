#include <Astra/AudioCore/AudioPlugin.h>
#include <Astra/Bootstrap/NativeRuntimePlugin.h>
#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/ModuleRuntime/AstraModuleABI.h>
#include <Astra/PlatformSDL3/SdlPlatformPlugin.h>
#include <Astra/Renderer2D/SdlGpuRendererPlugin.h>

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
    if (register_extension(host, "astra.provider.platform.sdl3", ASTRA_EXTENSION_PLATFORM_PROVIDER,
                           "platform_provider") != ASTRA_RESULT_OK) {
        return ASTRA_RESULT_ERROR;
    }
    if (register_extension(host, "astra.provider.renderer.sdl_gpu",
                           ASTRA_EXTENSION_RENDERER_PROVIDER,
                           "renderer_provider") != ASTRA_RESULT_OK) {
        return ASTRA_RESULT_ERROR;
    }
    if (register_extension(host, "astra.provider.audio.miniaudio", ASTRA_EXTENSION_AUDIO_PROVIDER,
                           "audio_provider") != ASTRA_RESULT_OK) {
        return ASTRA_RESULT_ERROR;
    }
    return register_extension(host, "astra.provider.content.default_project",
                              ASTRA_EXTENSION_PROJECT_CONTENT_PROVIDER, "project_content_provider");
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

extern "C" ASTRA_NATIVE_RUNTIME_PLUGIN_EXPORT bool
astra_register_native_runtime_providers(astra::RuntimeProviderRegistry* registry,
                                        astra::DiagnosticSink* diagnostics) {
    if (registry == nullptr || diagnostics == nullptr) {
        return false;
    }
    if (auto registered = astra::register_default_project_content_plugin(*registry, *diagnostics);
        !registered) {
        return false;
    }
    astra::register_sdl_platform_plugin(*registry, *diagnostics);
    astra::register_sdl_gpu_renderer_plugin(*registry, *diagnostics);
    astra::register_miniaudio_plugin(*registry, *diagnostics);
    return !diagnostics->has_errors();
}

extern "C" ASTRA_MODULE_EXPORT AstraResultCode astra_module_main(const AstraModuleHostApi* host,
                                                                 AstraModuleApi* out_module) {
    if (host == nullptr || out_module == nullptr || host->abi_version != ASTRA_MODULE_ABI_VERSION) {
        return ASTRA_RESULT_UNSUPPORTED_ABI;
    }
    out_module->abi_version = ASTRA_MODULE_ABI_VERSION;
    out_module->module_id = view("default_runtime_providers.runtime");
    out_module->module_context = nullptr;
    out_module->initialize = initialize;
    out_module->activate = activate;
    out_module->deactivate = deactivate;
    out_module->shutdown = shutdown;
    return ASTRA_RESULT_OK;
}
