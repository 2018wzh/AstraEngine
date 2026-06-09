#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/ModuleRuntime/ModuleAbi.h>
#include <Astra/Platform/Platform.hpp>
#include <nlohmann/json.hpp>

#include <filesystem>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <vector>

namespace Astra::ModuleRuntime {

enum class ModuleState {
    Discovered,
    DescriptorValid,
    DependenciesResolved,
    BinaryLoaded,
    Initialized,
    ExtensionsRegistered,
    Active,
    Deactivating,
    Shutdown,
    Unloaded,
    Failed
};

struct ModuleDescriptor {
    std::string id;
    std::string type;
    std::string entrypoint;
    std::string load_phase;
    std::vector<std::string> required_dependencies;
    std::vector<std::string> optional_dependencies;
    std::vector<std::string> capabilities;
    std::set<std::string> permissions;
    bool packaged = false;
};

struct PluginDescriptor {
    std::string id;
    std::string display_name;
    std::string version;
    std::string astra_api;
    bool packaged_eligible = false;
    bool require_binary_hash = false;
    std::string diagnostics_code_prefix;
    std::vector<ModuleDescriptor> modules;
};

struct RegisteredService {
    std::string service_id;
    std::string provider_module;
    std::string capability;
    std::string version = "v1";
    std::string lifetime = "engine";
    std::set<std::string> permissions_required;
};

struct ServiceResolveRequest {
    std::string requesting_module;
    std::string service_id;
    std::string required_version = "v1";
    std::set<std::string> module_capabilities;
    std::set<std::string> module_permissions;
    ModuleState module_state = ModuleState::Discovered;
};

struct ServiceResolveAudit {
    std::string requesting_module;
    std::string service_id;
    bool allowed = false;
    std::string reason;
};

struct RegisteredExtension {
    std::string extension_id;
    std::string provider_module;
    std::string kind;
};

struct RegisteredSlot {
    std::string slot_id;
    std::string default_provider_id;
};

struct RegisteredProvider {
    std::string slot_id;
    std::string provider_id;
    std::string provider_module;
};

struct EngineModulePolicy {
    std::map<std::string, std::string> selections;
};

struct ModuleReleaseGateReport {
    std::string schema = "astra.module.release_gate.foundation.v1";
    bool passed = true;
    std::string plugin_id;
    std::vector<std::string> load_order;
    nlohmann::json modules = nlohmann::json::array();
};

class ServiceRegistry {
public:
    [[nodiscard]] Astra::Core::Result<void> Register(RegisteredService service);
    [[nodiscard]] const RegisteredService* Find(std::string_view service_id) const;
    [[nodiscard]] Astra::Core::Result<ServiceResolveAudit> Resolve(const ServiceResolveRequest& request, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] const std::vector<RegisteredService>& Services() const;

private:
    std::vector<RegisteredService> services_;
};

class ExtensionRegistry {
public:
    [[nodiscard]] Astra::Core::Result<void> Register(RegisteredExtension extension);
    [[nodiscard]] const std::vector<RegisteredExtension>& Extensions() const;

private:
    std::vector<RegisteredExtension> extensions_;
};

class EngineModuleRegistry {
public:
    [[nodiscard]] Astra::Core::Result<void> RegisterSlot(RegisteredSlot slot);
    [[nodiscard]] Astra::Core::Result<void> RegisterProvider(RegisteredProvider provider);
    [[nodiscard]] Astra::Core::Result<void> ValidatePolicy(const EngineModulePolicy& policy, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] const std::vector<RegisteredSlot>& Slots() const;
    [[nodiscard]] const std::vector<RegisteredProvider>& Providers() const;

private:
    std::vector<RegisteredSlot> slots_;
    std::vector<RegisteredProvider> providers_;
};

[[nodiscard]] Astra::Core::Result<PluginDescriptor> LoadPluginDescriptor(const std::filesystem::path& path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<void> ValidatePluginDescriptor(const PluginDescriptor& descriptor, const std::filesystem::path& plugin_root, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<std::vector<std::string>> ResolveModuleOrder(const PluginDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<ModuleReleaseGateReport> ValidateModuleReleaseGate(const PluginDescriptor& descriptor, const std::filesystem::path& plugin_root, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] nlohmann::json ToJson(const ServiceResolveAudit& audit);
[[nodiscard]] nlohmann::json ToJson(const ModuleReleaseGateReport& report);

class ModuleManager {
public:
    explicit ModuleManager(Astra::Platform::PlatformServices& platform);
    [[nodiscard]] Astra::Core::Result<void> LoadAndActivate(const PluginDescriptor& descriptor, const std::filesystem::path& plugin_root, Astra::Core::DiagnosticSink& diagnostics);
    void DeactivateAndUnload(Astra::Core::DiagnosticSink& diagnostics);

    [[nodiscard]] const ServiceRegistry& Services() const;
    [[nodiscard]] const ExtensionRegistry& Extensions() const;
    [[nodiscard]] const EngineModuleRegistry& EngineModules() const;
    [[nodiscard]] ModuleState State(std::string_view module_id) const;

private:
    Astra::Platform::PlatformServices* platform_ = nullptr;
    ServiceRegistry services_;
    ExtensionRegistry extensions_;
    EngineModuleRegistry engine_modules_;
    std::map<std::string, ModuleState> states_;
    std::vector<Astra::Platform::DynamicLibraryHandle> libraries_;
    std::vector<AstraModuleApi> modules_;
};

} // namespace Astra::ModuleRuntime
