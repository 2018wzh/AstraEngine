#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>
#include <Astra/ExtensionRegistry/ExtensionRegistry.h>
#include <Astra/ModuleRuntime/AstraModuleABI.h>
#include <Astra/ModuleRuntime/PluginDescriptor.h>

#include <filesystem>
#include <memory>
#include <vector>

namespace astra {

class RuntimeProviderRegistry;

class ModuleManager {
  public:
    explicit ModuleManager(ExtensionRegistry& extension_registry,
                           RuntimeProviderRegistry* runtime_provider_registry = nullptr);
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
    AstraResultCode register_extension_from_abi(const AstraExtensionDescriptor* descriptor);

    std::vector<PluginDescriptor> plugins_;
    std::vector<std::unique_ptr<LoadedModule>> loaded_modules_;
    ExtensionRegistry& extension_registry_;
    RuntimeProviderRegistry* runtime_provider_registry_ = nullptr;
    ModuleDescriptor* active_module_ = nullptr;
    DiagnosticSink* active_diagnostics_ = nullptr;
};

} // namespace astra
