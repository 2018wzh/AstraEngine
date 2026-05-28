#pragma once

#include <Astra/Core/Diagnostics.h>

namespace astra {

class RuntimeProviderRegistry;

inline constexpr const char* kNativeRuntimeProviderEntrypoint =
    "astra_register_native_runtime_providers";

using AstraNativeRuntimeProviderEntryFn = bool (*)(RuntimeProviderRegistry* registry,
                                                   DiagnosticSink* diagnostics);

} // namespace astra

#if defined(_WIN32)
#define ASTRA_NATIVE_RUNTIME_PLUGIN_EXPORT __declspec(dllexport)
#else
#define ASTRA_NATIVE_RUNTIME_PLUGIN_EXPORT __attribute__((visibility("default")))
#endif
