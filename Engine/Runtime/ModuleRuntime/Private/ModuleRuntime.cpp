#include <Astra/ModuleRuntime/ModuleRuntime.hpp>

#include <Astra/Core/Logging.hpp>

#include <algorithm>
#include <ranges>
#include <yaml-cpp/yaml.h>

namespace Astra::ModuleRuntime {

namespace {

std::string ToString(AstraStringView value) {
    if (value.data == nullptr) {
        return {};
    }
    return std::string(value.data, value.data + value.size);
}

std::vector<std::string> ReadStringList(const YAML::Node& node) {
    std::vector<std::string> values;
    if (!node) {
        return values;
    }
    for (const auto& item : node) {
        values.push_back(item.as<std::string>());
    }
    return values;
}

void Emit(Astra::Core::DiagnosticSink& diagnostics, std::string code, std::string message) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "module.descriptor";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
    diagnostic.message = std::move(message);
    diagnostics.Emit(std::move(diagnostic));
}

bool Contains(const std::vector<std::string>& values, std::string_view value) {
    return std::ranges::any_of(values, [&](const std::string& item) { return item == value; });
}

bool IsValidModuleType(std::string_view type) {
    return type == "runtime" || type == "editor" || type == "developer" || type == "tool";
}

bool IsValidLoadPhase(std::string_view phase) {
    return phase == "runtime_startup" || phase == "editor_startup" || phase == "tool_startup" || phase == "manual";
}

struct HostContext {
    ServiceRegistry* services = nullptr;
    ExtensionRegistry* extensions = nullptr;
    EngineModuleRegistry* engine_modules = nullptr;
    Astra::Core::DiagnosticSink* diagnostics = nullptr;
    std::string module_id;
};

AstraModuleResult RegisterService(void* user, AstraStringView service_id, AstraStringView capability) {
    auto* context = static_cast<HostContext*>(user);
    RegisteredService service;
    service.service_id = ToString(service_id);
    service.provider_module = context->module_id;
    service.capability = ToString(capability);
    auto result = context->services->Register(std::move(service));
    return result ? ASTRA_MODULE_OK : ASTRA_MODULE_ERROR;
}

AstraModuleResult RegisterExtension(void* user, AstraStringView extension_id, AstraStringView kind) {
    auto* context = static_cast<HostContext*>(user);
    auto result = context->extensions->Register({ToString(extension_id), context->module_id, ToString(kind)});
    return result ? ASTRA_MODULE_OK : ASTRA_MODULE_ERROR;
}

AstraModuleResult RegisterProvider(void* user, AstraStringView slot_id, AstraStringView provider_id) {
    auto* context = static_cast<HostContext*>(user);
    auto result = context->engine_modules->RegisterProvider({ToString(slot_id), ToString(provider_id), context->module_id});
    return result ? ASTRA_MODULE_OK : ASTRA_MODULE_ERROR;
}

void EmitDiagnostic(void* user, AstraStringView code, AstraStringView message) {
    auto* context = static_cast<HostContext*>(user);
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = ToString(code);
    diagnostic.category = "module.abi";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Info;
    diagnostic.message = ToString(message);
    diagnostic.objects = {{"module", context->module_id}};
    context->diagnostics->Emit(std::move(diagnostic));
}

} // namespace

Astra::Core::Result<void> ServiceRegistry::Register(RegisteredService service) {
    if (Find(service.service_id) != nullptr) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "service id already registered");
    }
    services_.push_back(std::move(service));
    return Astra::Core::Result<void>::Success();
}

const RegisteredService* ServiceRegistry::Find(std::string_view service_id) const {
    auto it = std::ranges::find_if(services_, [&](const RegisteredService& service) { return service.service_id == service_id; });
    return it == services_.end() ? nullptr : &*it;
}

Astra::Core::Result<ServiceResolveAudit> ServiceRegistry::Resolve(const ServiceResolveRequest& request, Astra::Core::DiagnosticSink& diagnostics) const {
    ServiceResolveAudit audit;
    audit.requesting_module = request.requesting_module;
    audit.service_id = request.service_id;

    const auto* service = Find(request.service_id);
    if (service == nullptr) {
        audit.reason = "service not registered";
    } else if (request.module_state != ModuleState::Active && request.module_state != ModuleState::Initialized) {
        audit.reason = "module state cannot resolve services";
    } else if (service->version != request.required_version) {
        audit.reason = "service version mismatch";
    } else if (!request.module_capabilities.contains(service->capability)) {
        audit.reason = "requesting module lacks required capability";
    } else {
        auto missing_permission = std::ranges::find_if(service->permissions_required, [&](const std::string& permission) {
            return !request.module_permissions.contains(permission);
        });
        if (missing_permission != service->permissions_required.end()) {
            audit.reason = "requesting module lacks required permission";
        } else {
            audit.allowed = true;
            audit.reason = "allowed";
        }
    }

    if (!audit.allowed) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_MODULE_SERVICE_RESOLVE_DENIED";
        diagnostic.category = "module.service";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = audit.reason;
        diagnostic.objects = {{"module", request.requesting_module}, {"service", request.service_id}};
        Astra::Core::LogDiagnostic(diagnostic, "module.service", request.requesting_module);
        diagnostics.Emit(std::move(diagnostic));
        return Astra::Core::Result<ServiceResolveAudit>::Failure(Astra::Core::ErrorCode::PermissionDenied, audit.reason);
    }
    return Astra::Core::Result<ServiceResolveAudit>::Success(std::move(audit));
}

const std::vector<RegisteredService>& ServiceRegistry::Services() const { return services_; }

Astra::Core::Result<void> ExtensionRegistry::Register(RegisteredExtension extension) {
    auto duplicate = std::ranges::find_if(extensions_, [&](const RegisteredExtension& item) { return item.extension_id == extension.extension_id; });
    if (duplicate != extensions_.end()) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "extension id already registered");
    }
    extensions_.push_back(std::move(extension));
    return Astra::Core::Result<void>::Success();
}

const std::vector<RegisteredExtension>& ExtensionRegistry::Extensions() const { return extensions_; }

Astra::Core::Result<void> EngineModuleRegistry::RegisterSlot(RegisteredSlot slot) {
    auto duplicate = std::ranges::find_if(slots_, [&](const RegisteredSlot& item) { return item.slot_id == slot.slot_id; });
    if (duplicate != slots_.end()) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "slot id already registered");
    }
    slots_.push_back(std::move(slot));
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> EngineModuleRegistry::RegisterProvider(RegisteredProvider provider) {
    auto duplicate = std::ranges::find_if(providers_, [&](const RegisteredProvider& item) { return item.provider_id == provider.provider_id; });
    if (duplicate != providers_.end()) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "provider id already registered");
    }
    providers_.push_back(std::move(provider));
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> EngineModuleRegistry::ValidatePolicy(const EngineModulePolicy& policy, Astra::Core::DiagnosticSink& diagnostics) const {
    bool valid = true;
    for (const auto& [slot_id, provider_id] : policy.selections) {
        auto provider = std::ranges::find_if(providers_, [&](const RegisteredProvider& item) { return item.provider_id == provider_id; });
        if (provider == providers_.end()) {
            Emit(diagnostics, "ASTRA_MODULE_PROVIDER_MISSING", "Engine module policy references an unknown provider.");
            valid = false;
            continue;
        }
        if (provider->slot_id != slot_id) {
            Emit(diagnostics, "ASTRA_MODULE_PROVIDER_SLOT_MISMATCH", "Engine module policy selected provider for the wrong slot.");
            valid = false;
        }
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "engine module policy invalid");
}

const std::vector<RegisteredSlot>& EngineModuleRegistry::Slots() const { return slots_; }
const std::vector<RegisteredProvider>& EngineModuleRegistry::Providers() const { return providers_; }

Astra::Core::Result<PluginDescriptor> LoadPluginDescriptor(const std::filesystem::path& path, Astra::Core::DiagnosticSink& diagnostics) {
    YAML::Node root;
    try {
        root = YAML::LoadFile(path.string());
    } catch (const YAML::Exception& error) {
        Emit(diagnostics, "ASTRA_PLUGIN_DESCRIPTOR_PARSE", error.what());
        return Astra::Core::Result<PluginDescriptor>::Failure(Astra::Core::ErrorCode::InvalidFormat, error.what());
    }

    PluginDescriptor descriptor;
    descriptor.id = root["id"].as<std::string>("");
    descriptor.display_name = root["display_name"].as<std::string>("");
    descriptor.version = root["version"].as<std::string>("");
    descriptor.astra_api = root["astra_api"].as<std::string>("");
    descriptor.diagnostics_code_prefix = root["diagnostics"]["code_prefix"].as<std::string>("");
    descriptor.packaged_eligible = root["release"]["packaged_eligible"].as<bool>(false);
    descriptor.require_binary_hash = root["release"]["require_binary_hash"].as<bool>(false);

    for (const auto& module_node : root["modules"]) {
        ModuleDescriptor module;
        module.id = module_node["id"].as<std::string>("");
        module.type = module_node["type"].as<std::string>("");
        module.entrypoint = module_node["entrypoint"].as<std::string>("");
        module.load_phase = module_node["load_phase"].as<std::string>("");
        const auto dependencies = module_node["dependencies"];
        if (dependencies) {
            module.required_dependencies = ReadStringList(dependencies["required"]);
            module.optional_dependencies = ReadStringList(dependencies["optional"]);
        }
        module.capabilities = ReadStringList(module_node["capabilities"]);
        module.packaged = module_node["permissions"]["runtime"]["packaged"].as<bool>(false);
        const auto permissions = module_node["permissions"];
        if (permissions) {
            for (const auto& permission : permissions) {
                const auto key = permission.first.as<std::string>();
                if (permission.second.IsScalar() && permission.second.as<bool>(false)) {
                    module.permissions.insert(key);
                }
                if (key == "runtime" && permission.second["packaged"].as<bool>(false)) {
                    module.permissions.insert("runtime.packaged");
                }
            }
        }
        descriptor.modules.push_back(std::move(module));
    }

    return Astra::Core::Result<PluginDescriptor>::Success(std::move(descriptor));
}

Astra::Core::Result<void> ValidatePluginDescriptor(const PluginDescriptor& descriptor, const std::filesystem::path& plugin_root, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.empty() || descriptor.version.empty() || descriptor.astra_api.empty()) {
        Emit(diagnostics, "ASTRA_PLUGIN_DESCRIPTOR_REQUIRED", "Plugin descriptor requires id, version, and astra_api.");
    }
    if (!descriptor.astra_api.starts_with(">=") || descriptor.diagnostics_code_prefix.empty()) {
        Emit(diagnostics, "ASTRA_PLUGIN_DESCRIPTOR_RELEASE_FIELDS", "Plugin descriptor requires an astra_api range and diagnostics code prefix.");
    }
    if (descriptor.modules.empty()) {
        Emit(diagnostics, "ASTRA_PLUGIN_DESCRIPTOR_MODULES", "Plugin descriptor requires at least one module.");
    }
    for (const auto& module : descriptor.modules) {
        if (module.id.empty() || module.entrypoint.empty()) {
            Emit(diagnostics, "ASTRA_PLUGIN_MODULE_REQUIRED", "Module requires id and entrypoint.");
        }
        if (!IsValidModuleType(module.type) || !IsValidLoadPhase(module.load_phase)) {
            Emit(diagnostics, "ASTRA_PLUGIN_MODULE_POLICY", "Module type or load phase is not allowed by the foundation release gate.");
        }
        if (module.type == "runtime" && module.load_phase != "runtime_startup" && module.load_phase != "manual") {
            Emit(diagnostics, "ASTRA_PLUGIN_MODULE_PHASE_MISMATCH", "Runtime module must use runtime_startup or manual load phase.");
        }
        if (module.packaged && !descriptor.packaged_eligible) {
            Emit(diagnostics, "ASTRA_PLUGIN_PACKAGED_INELIGIBLE", "Packaged module belongs to a plugin that is not release eligible.");
        }
        if (module.packaged && !module.permissions.contains("runtime.packaged")) {
            Emit(diagnostics, "ASTRA_PLUGIN_PACKAGED_PERMISSION", "Packaged module must declare runtime.packaged permission.");
        }
        if (Contains(module.capabilities, "engine_module_provider") && !Contains(module.capabilities, "renderer2d_provider")) {
            Emit(diagnostics, "ASTRA_PLUGIN_CAPABILITY_PERMISSION", "Foundation engine module providers must declare a concrete provider capability.");
        }
        auto entry = std::filesystem::weakly_canonical(plugin_root / module.entrypoint);
        auto root = std::filesystem::weakly_canonical(plugin_root);
        auto entry_text = entry.string();
        auto root_text = root.string();
        if (!entry_text.starts_with(root_text)) {
            Emit(diagnostics, "ASTRA_PLUGIN_ENTRYPOINT_ESCAPE", "Module entrypoint escapes plugin root.");
        }
    }
    return diagnostics.HasBlocking() ? Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid plugin descriptor")
                                     : Astra::Core::Result<void>::Success();
}

Astra::Core::Result<std::vector<std::string>> ResolveModuleOrder(const PluginDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    std::set<std::string> module_ids;
    for (const auto& module : descriptor.modules) {
        module_ids.insert(module.id);
    }

    std::vector<std::string> ordered;
    std::set<std::string> visiting;
    std::set<std::string> visited;

    std::function<void(const ModuleDescriptor&)> visit = [&](const ModuleDescriptor& module) {
        if (visited.contains(module.id)) {
            return;
        }
        if (visiting.contains(module.id)) {
            Emit(diagnostics, "ASTRA_PLUGIN_DEPENDENCY_CYCLE", "Module dependency cycle detected.");
            return;
        }
        visiting.insert(module.id);
        for (const auto& dependency : module.required_dependencies) {
            if (!module_ids.contains(dependency)) {
                Emit(diagnostics, "ASTRA_PLUGIN_DEPENDENCY_MISSING", "Required module dependency is missing.");
                continue;
            }
            const auto& dependency_module = *std::ranges::find_if(descriptor.modules, [&](const ModuleDescriptor& item) { return item.id == dependency; });
            visit(dependency_module);
        }
        visiting.erase(module.id);
        visited.insert(module.id);
        ordered.push_back(module.id);
    };

    for (const auto& module : descriptor.modules) {
        visit(module);
    }

    if (diagnostics.HasBlocking()) {
        return Astra::Core::Result<std::vector<std::string>>::Failure(Astra::Core::ErrorCode::DependencyCycle, "module dependency resolution failed");
    }
    return Astra::Core::Result<std::vector<std::string>>::Success(ordered);
}

Astra::Core::Result<ModuleReleaseGateReport> ValidateModuleReleaseGate(const PluginDescriptor& descriptor, const std::filesystem::path& plugin_root, Astra::Core::DiagnosticSink& diagnostics) {
    ModuleReleaseGateReport report;
    report.plugin_id = descriptor.id;
    auto validation = ValidatePluginDescriptor(descriptor, plugin_root, diagnostics);
    auto order = ResolveModuleOrder(descriptor, diagnostics);
    if (order) {
        report.load_order = order.Value();
    }

    for (const auto& module : descriptor.modules) {
        const auto binary = plugin_root / module.entrypoint;
        const bool binary_exists = std::filesystem::exists(binary);
        report.modules.push_back({
            {"id", module.id},
            {"type", module.type},
            {"load_phase", module.load_phase},
            {"entrypoint", module.entrypoint},
            {"binary_exists", binary_exists},
            {"packaged", module.packaged},
            {"capabilities", module.capabilities},
        });
        if (!binary_exists) {
            Emit(diagnostics, "ASTRA_MODULE_RELEASE_BINARY_MISSING", "Release gate requires descriptor entrypoint binaries to exist.");
        }
    }

    report.passed = validation && order && !diagnostics.HasBlocking();
    if (!report.passed) {
        return Astra::Core::Result<ModuleReleaseGateReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "module release gate failed");
    }
    return Astra::Core::Result<ModuleReleaseGateReport>::Success(std::move(report));
}

ModuleManager::ModuleManager(Astra::Platform::PlatformServices& platform) : platform_(&platform) {}

Astra::Core::Result<void> ModuleManager::LoadAndActivate(const PluginDescriptor& descriptor, const std::filesystem::path& plugin_root, Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Core::DefaultLogger().Log(
        "module.lifecycle",
        descriptor.id,
        Astra::Core::LogLevel::Info,
        "plugin activation started",
        {{"plugin_root", plugin_root.string()}, {"modules", std::to_string(descriptor.modules.size())}});
    auto validation = ValidatePluginDescriptor(descriptor, plugin_root, diagnostics);
    if (!validation) {
        return validation;
    }
    auto order = ResolveModuleOrder(descriptor, diagnostics);
    if (!order) {
        return Astra::Core::Result<void>::Failure(order.Error(), order.Message());
    }

    for (const auto& module_id : order.Value()) {
        const auto& module = *std::ranges::find_if(descriptor.modules, [&](const ModuleDescriptor& item) { return item.id == module_id; });
        Astra::Core::DefaultLogger().Log(
            "module.lifecycle",
            module.id,
            Astra::Core::LogLevel::Info,
            "module load started",
            {{"entrypoint", (plugin_root / module.entrypoint).string()}, {"load_phase", module.load_phase}});
        states_[module.id] = ModuleState::DependenciesResolved;
        auto library = platform_->DynamicLibrary().Load(plugin_root / module.entrypoint);
        if (!library) {
            states_[module.id] = ModuleState::Failed;
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_MODULE_BINARY_LOAD";
            diagnostic.category = "module.lifecycle";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = library.Message();
            Astra::Core::LogDiagnostic(diagnostic, "module.lifecycle", module.id);
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(library.Error(), library.Message());
        }
        libraries_.push_back(library.Value());
        states_[module.id] = ModuleState::BinaryLoaded;

        auto symbol = platform_->DynamicLibrary().Symbol(library.Value(), "astra_module_main");
        if (!symbol) {
            states_[module.id] = ModuleState::Failed;
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_MODULE_ENTRYPOINT_MISSING";
            diagnostic.category = "module.abi";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = symbol.Message();
            Astra::Core::LogDiagnostic(diagnostic, "module.abi", module.id);
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(symbol.Error(), symbol.Message());
        }

        HostContext context{&services_, &extensions_, &engine_modules_, &diagnostics, module.id};
        AstraModuleHostApi host{1, {EmitDiagnostic, &context}, {RegisterService, &context}, {RegisterExtension, &context}, {RegisterProvider, &context}};
        AstraModuleApi api{};
        auto main = reinterpret_cast<AstraModuleMainFn>(symbol.Value());
        auto result = main(&host, &api);
        if (result != ASTRA_MODULE_OK || api.abi_version != 1 || api.initialize == nullptr || api.activate == nullptr || api.deactivate == nullptr || api.shutdown == nullptr) {
            states_[module.id] = ModuleState::Failed;
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_MODULE_ABI_INVALID";
            diagnostic.category = "module.abi";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = "Module ABI is incompatible or missing lifecycle functions.";
            Astra::Core::LogDiagnostic(diagnostic, "module.abi", module.id);
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::VersionMismatch, "invalid module ABI");
        }
        modules_.push_back(api);
        if (api.initialize(api.module_state, &host) != ASTRA_MODULE_OK) {
            states_[module.id] = ModuleState::Failed;
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InternalError, "module initialize failed");
        }
        states_[module.id] = ModuleState::Initialized;
        if (api.activate(api.module_state) != ASTRA_MODULE_OK) {
            states_[module.id] = ModuleState::Failed;
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InternalError, "module activate failed");
        }
        states_[module.id] = ModuleState::Active;
        Astra::Core::DefaultLogger().Log(
            "module.lifecycle",
            module.id,
            Astra::Core::LogLevel::Info,
            "module active",
            {{"services", std::to_string(services_.Services().size())},
             {"extensions", std::to_string(extensions_.Extensions().size())},
             {"providers", std::to_string(engine_modules_.Providers().size())}});
    }

    Astra::Core::DefaultLogger().Log(
        "module.lifecycle",
        descriptor.id,
        Astra::Core::LogLevel::Info,
        "plugin activation finished",
        {{"modules", std::to_string(modules_.size())}});
    return Astra::Core::Result<void>::Success();
}

void ModuleManager::DeactivateAndUnload(Astra::Core::DiagnosticSink&) {
    Astra::Core::DefaultLogger().Log(
        "module.lifecycle",
        "module_manager",
        Astra::Core::LogLevel::Info,
        "module unload started",
        {{"modules", std::to_string(modules_.size())}});
    for (auto it = modules_.rbegin(); it != modules_.rend(); ++it) {
        it->deactivate(it->module_state);
        it->shutdown(it->module_state);
        states_[ToString(it->module_id)] = ModuleState::Shutdown;
    }
    std::vector<std::string> module_ids;
    for (const auto& module : modules_) {
        module_ids.push_back(ToString(module.module_id));
    }
    for (auto library : libraries_) {
        platform_->DynamicLibrary().Unload(library);
    }
    for (const auto& module_id : module_ids) {
        states_[module_id] = ModuleState::Unloaded;
    }
    libraries_.clear();
    modules_.clear();
    Astra::Core::DefaultLogger().Log(
        "module.lifecycle",
        "module_manager",
        Astra::Core::LogLevel::Info,
        "module unload finished");
}

const ServiceRegistry& ModuleManager::Services() const { return services_; }
const ExtensionRegistry& ModuleManager::Extensions() const { return extensions_; }
const EngineModuleRegistry& ModuleManager::EngineModules() const { return engine_modules_; }

nlohmann::json ToJson(const ServiceResolveAudit& audit) {
    return {
        {"requesting_module", audit.requesting_module},
        {"service_id", audit.service_id},
        {"allowed", audit.allowed},
        {"reason", audit.reason},
    };
}

nlohmann::json ToJson(const ModuleReleaseGateReport& report) {
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"plugin_id", report.plugin_id},
        {"load_order", report.load_order},
        {"modules", report.modules},
    };
}

ModuleState ModuleManager::State(std::string_view module_id) const {
    auto it = states_.find(std::string(module_id));
    return it == states_.end() ? ModuleState::Discovered : it->second;
}

} // namespace Astra::ModuleRuntime
