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

#define ASTRA_MODULE_ABI_VERSION 1u

typedef struct AstraStringView {
    const char* data;
    uint64_t size;
} AstraStringView;

typedef struct AstraByteSpan {
    const uint8_t* data;
    uint64_t size;
} AstraByteSpan;

typedef void* AstraOpaqueHandle;

typedef enum AstraResultCode {
    ASTRA_RESULT_OK = 0,
    ASTRA_RESULT_ERROR = 1,
    ASTRA_RESULT_UNSUPPORTED_ABI = 2,
    ASTRA_RESULT_INVALID_ARGUMENT = 3,
    ASTRA_RESULT_PERMISSION_DENIED = 4
} AstraResultCode;

typedef enum AstraDiagnosticSeverity {
    ASTRA_DIAGNOSTIC_INFO = 0,
    ASTRA_DIAGNOSTIC_WARNING = 1,
    ASTRA_DIAGNOSTIC_ERROR = 2
} AstraDiagnosticSeverity;

typedef enum AstraExtensionKind {
    ASTRA_EXTENSION_SERVICE_EXTENSION = 0,
    ASTRA_EXTENSION_RUNTIME_COMMAND_SOURCE = 1,
    ASTRA_EXTENSION_COMPATIBILITY_ADAPTER = 2,
    ASTRA_EXTENSION_VFS_MOUNT_PROVIDER = 3,
    ASTRA_EXTENSION_FOREIGN_ASSET_RESOLVER = 4,
    ASTRA_EXTENSION_SAVE_EXTENSION_STATE_PROVIDER = 5,
    ASTRA_EXTENSION_RUNTIME_ECS_SYSTEM_PACK = 6,
    ASTRA_EXTENSION_SCRIPT_FUNCTION_PROVIDER = 7,
    ASTRA_EXTENSION_STORY_GRAPH_NODE_PROVIDER = 8,
    ASTRA_EXTENSION_ASSET_VALIDATOR = 9,
    ASTRA_EXTENSION_COOK_PROCESSOR = 10,
    ASTRA_EXTENSION_EDITOR_PANEL_PROVIDER = 11,
    ASTRA_EXTENSION_MCP_PROVIDER = 12,
    ASTRA_EXTENSION_AI_PROVIDER = 13,
    ASTRA_EXTENSION_VN_PROPERTY_TYPE = 14,
    ASTRA_EXTENSION_PLATFORM_PROVIDER = 15,
    ASTRA_EXTENSION_RENDERER_PROVIDER = 16,
    ASTRA_EXTENSION_AUDIO_PROVIDER = 17,
    ASTRA_EXTENSION_PROJECT_CONTENT_PROVIDER = 18
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
