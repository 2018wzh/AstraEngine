#pragma once

#include <stdint.h>

#if defined(_WIN32)
#if defined(ASTRA_MODULE_BUILD)
#define ASTRA_MODULE_EXPORT __declspec(dllexport)
#else
#define ASTRA_MODULE_EXPORT __declspec(dllimport)
#endif
#else
#define ASTRA_MODULE_EXPORT __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define ASTRA_MODULE_ABI_VERSION 2u

typedef struct AstraStringView {
    const char* data;
    uint64_t size;
} AstraStringView;

typedef void* AstraOpaqueHandle;

typedef enum AstraResultCode {
    ASTRA_RESULT_OK = 0,
    ASTRA_RESULT_ERROR = 1,
    ASTRA_RESULT_UNSUPPORTED_ABI = 2,
    ASTRA_RESULT_INVALID_ARGUMENT = 3,
    ASTRA_RESULT_PERMISSION_DENIED = 4,
    ASTRA_RESULT_NOT_FOUND = 5
} AstraResultCode;

typedef enum AstraDiagnosticSeverity {
    ASTRA_DIAGNOSTIC_INFO = 0,
    ASTRA_DIAGNOSTIC_WARNING = 1,
    ASTRA_DIAGNOSTIC_ERROR = 2
} AstraDiagnosticSeverity;

typedef enum AstraExtensionKind {
    ASTRA_EXTENSION_SERVICE_EXTENSION = 0,
    ASTRA_EXTENSION_PROPERTY_TYPE_PROVIDER = 1,
    ASTRA_EXTENSION_EDITOR_METADATA_PROVIDER = 2
} AstraExtensionKind;

typedef struct AstraDiagnosticSink {
    void* user_data;
    void (*emit)(void* user_data, AstraDiagnosticSeverity severity, AstraStringView code,
                 AstraStringView message);
} AstraDiagnosticSink;

typedef struct AstraExtensionDescriptor {
    AstraStringView id;
    AstraExtensionKind kind;
    AstraStringView required_capability;
    AstraStringView required_permission;
} AstraExtensionDescriptor;

typedef struct AstraModuleHostApi {
    uint32_t abi_version;
    void* host_context;
    AstraDiagnosticSink diagnostics;
    AstraResultCode (*register_extension)(void* host_context,
                                          const AstraExtensionDescriptor* descriptor);
    AstraResultCode (*get_service)(void* host_context, AstraStringView service_id,
                                   AstraOpaqueHandle* out_service);
} AstraModuleHostApi;

typedef AstraResultCode (*AstraModuleLifecycleFn)(void* module_context,
                                                  const AstraModuleHostApi* host);

typedef struct AstraModuleApi {
    uint32_t abi_version;
    AstraStringView module_id;
    void* module_context;
    AstraModuleLifecycleFn initialize;
    AstraModuleLifecycleFn activate;
    AstraModuleLifecycleFn deactivate;
    AstraModuleLifecycleFn shutdown;
} AstraModuleApi;

typedef AstraResultCode (*AstraModuleMainFn)(const AstraModuleHostApi* host,
                                             AstraModuleApi* out_module);

ASTRA_MODULE_EXPORT AstraResultCode astra_module_main(const AstraModuleHostApi* host,
                                                      AstraModuleApi* out_module);

#ifdef __cplusplus
}
#endif
