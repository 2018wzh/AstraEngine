#pragma once

#include <stdint.h>

#if defined(_WIN32)
#define ASTRA_MODULE_EXPORT __declspec(dllexport)
#else
#define ASTRA_MODULE_EXPORT __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct AstraStringView {
    const char* data;
    uint64_t size;
} AstraStringView;

typedef struct AstraOpaqueHandle {
    uint64_t value;
} AstraOpaqueHandle;

typedef enum AstraModuleResult {
    ASTRA_MODULE_OK = 0,
    ASTRA_MODULE_ERROR = 1,
    ASTRA_MODULE_VERSION_MISMATCH = 2
} AstraModuleResult;

typedef struct AstraDiagnosticsApi {
    void (*emit)(void* user, AstraStringView code, AstraStringView message);
    void* user;
} AstraDiagnosticsApi;

typedef struct AstraServiceRegistryApi {
    AstraModuleResult (*register_service)(void* user, AstraStringView service_id, AstraStringView capability);
    void* user;
} AstraServiceRegistryApi;

typedef struct AstraExtensionRegistryApi {
    AstraModuleResult (*register_extension)(void* user, AstraStringView extension_id, AstraStringView kind);
    void* user;
} AstraExtensionRegistryApi;

typedef struct AstraEngineModuleRegistryApi {
    AstraModuleResult (*register_provider)(void* user, AstraStringView slot_id, AstraStringView provider_id);
    void* user;
} AstraEngineModuleRegistryApi;

typedef struct AstraModuleHostApi {
    uint32_t abi_version;
    AstraDiagnosticsApi diagnostics;
    AstraServiceRegistryApi services;
    AstraExtensionRegistryApi extensions;
    AstraEngineModuleRegistryApi engine_modules;
} AstraModuleHostApi;

typedef AstraModuleResult (*AstraModuleInitializeFn)(void* module_state, const AstraModuleHostApi* host);
typedef AstraModuleResult (*AstraModuleActivateFn)(void* module_state);
typedef AstraModuleResult (*AstraModuleDeactivateFn)(void* module_state);
typedef AstraModuleResult (*AstraModuleShutdownFn)(void* module_state);

typedef struct AstraModuleApi {
    uint32_t abi_version;
    AstraStringView module_id;
    void* module_state;
    AstraModuleInitializeFn initialize;
    AstraModuleActivateFn activate;
    AstraModuleDeactivateFn deactivate;
    AstraModuleShutdownFn shutdown;
} AstraModuleApi;

typedef AstraModuleResult (*AstraModuleMainFn)(const AstraModuleHostApi* host, AstraModuleApi* out_module);

#ifdef __cplusplus
}
#endif
