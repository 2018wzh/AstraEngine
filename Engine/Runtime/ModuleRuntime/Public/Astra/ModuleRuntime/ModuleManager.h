#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>
#include <Astra/ModuleRuntime/AstraModuleABI.h>
#include <Astra/ModuleRuntime/ExtensionRegistry.h>
#include <Astra/ModuleRuntime/PluginDescriptor.h>
#include <Astra/ModuleRuntime/ServiceRegistry.h>

#include <filesystem>
#include <memory>
#include <vector>

namespace astra {

class ModuleManager {
  public:
    ModuleManager(ServiceRegistry& service_registry, ExtensionRegistry& extension_registry);
    ~ModuleManager();

    ModuleManager(const ModuleManager&) = delete;
    ModuleManager& operator=(const ModuleManager&) = delete;

    VoidResult discover(const std::vector<std::filesystem::path>& plugin_roots,
                        DiagnosticSink& diagnostics);
    VoidResult load_discovered(DiagnosticSink& diagnostics);
    void unload_all(DiagnosticSink& diagnostics);

    [[nodiscard]] const std::vector<PluginDescriptor>& discovered_plugins() const;
    [[nodiscard]] std::size_t loaded_module_count() const;

  private:
    struct LoadedModule;

    static AstraResultCode register_extension_thunk(void* host_context,
                                                    const AstraExtensionDescriptor* descriptor);
    static AstraResultCode get_service_thunk(void* host_context, AstraStringView service_id,
                                             AstraOpaqueHandle* out_service);

    AstraResultCode register_extension_from_abi(const AstraExtensionDescriptor* descriptor);
    AstraResultCode get_service_from_abi(AstraStringView service_id, AstraOpaqueHandle* out_service);

    std::vector<PluginDescriptor> plugins_;
    std::vector<std::unique_ptr<LoadedModule>> loaded_modules_;
    ServiceRegistry& service_registry_;
    ExtensionRegistry& extension_registry_;
    ModuleDescriptor* active_module_ = nullptr;
    DiagnosticSink* active_diagnostics_ = nullptr;
};

} // namespace astra
