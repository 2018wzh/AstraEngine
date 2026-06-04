#include <Astra/ModuleRuntime/ModuleManager.h>

#include <Astra/Core/Path.h>

#include <algorithm>
#include <cstring>
#include <functional>
#include <map>
#include <sstream>
#include <unordered_map>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#endif

namespace astra {

namespace {

std::string from_view(AstraStringView view) {
    if (view.data == nullptr || view.size == 0) {
        return {};
    }
    return std::string(view.data, static_cast<std::size_t>(view.size));
}

DiagnosticSeverity to_severity(AstraDiagnosticSeverity severity) {
    switch (severity) {
    case ASTRA_DIAGNOSTIC_INFO:
        return DiagnosticSeverity::Info;
    case ASTRA_DIAGNOSTIC_WARNING:
        return DiagnosticSeverity::Warning;
    case ASTRA_DIAGNOSTIC_ERROR:
        return DiagnosticSeverity::Error;
    }
    return DiagnosticSeverity::Info;
}

ExtensionKind to_extension_kind(AstraExtensionKind kind) {
    switch (kind) {
    case ASTRA_EXTENSION_SERVICE_EXTENSION:
        return ExtensionKind::ServiceExtension;
    case ASTRA_EXTENSION_PROPERTY_TYPE_PROVIDER:
        return ExtensionKind::PropertyTypeProvider;
    case ASTRA_EXTENSION_EDITOR_METADATA_PROVIDER:
        return ExtensionKind::EditorMetadataProvider;
    }
    return ExtensionKind::ServiceExtension;
}

std::string current_platform_id() {
#if defined(_WIN32)
    return "windows-x64";
#elif defined(__APPLE__)
    return "macos-x64";
#else
    return "linux-x64";
#endif
}

struct Version {
    int major = 0;
    int minor = 0;
    int patch = 0;
};

Version parse_version(std::string value) {
    Version version;
    std::replace(value.begin(), value.end(), '.', ' ');
    std::istringstream stream(value);
    stream >> version.major >> version.minor >> version.patch;
    return version;
}

int compare_version(Version lhs, Version rhs) {
    if (lhs.major != rhs.major) {
        return lhs.major < rhs.major ? -1 : 1;
    }
    if (lhs.minor != rhs.minor) {
        return lhs.minor < rhs.minor ? -1 : 1;
    }
    if (lhs.patch != rhs.patch) {
        return lhs.patch < rhs.patch ? -1 : 1;
    }
    return 0;
}

bool satisfies_constraint(Version host, std::string token) {
    if (token.empty()) {
        return true;
    }
    std::string op = "==";
    if (token.starts_with(">=") || token.starts_with("<=") || token.starts_with("==")) {
        op = token.substr(0, 2);
        token = token.substr(2);
    } else if (token.starts_with(">") || token.starts_with("<")) {
        op = token.substr(0, 1);
        token = token.substr(1);
    }
    const int comparison = compare_version(host, parse_version(token));
    if (op == ">=") {
        return comparison >= 0;
    }
    if (op == ">") {
        return comparison > 0;
    }
    if (op == "<=") {
        return comparison <= 0;
    }
    if (op == "<") {
        return comparison < 0;
    }
    return comparison == 0;
}

bool astra_api_supported(std::string_view constraints) {
    constexpr Version host{0, 2, 0};
    std::istringstream stream{std::string(constraints)};
    std::string token;
    while (stream >> token) {
        if (!satisfies_constraint(host, token)) {
            return false;
        }
    }
    return true;
}

bool supports_current_platform(const ModuleDescriptor& module) {
    if (module.platforms.empty()) {
        return true;
    }
    const std::string platform = current_platform_id();
    return std::find(module.platforms.begin(), module.platforms.end(), platform) !=
           module.platforms.end();
}

int first_module_phase(const PluginDescriptor& plugin) {
    if (plugin.modules.empty()) {
        return 0;
    }
    return load_phase_order(plugin.modules.front().load_phase);
}

void emit_diagnostic(void* user_data, AstraDiagnosticSeverity severity, AstraStringView code,
                     AstraStringView message) {
    auto* sink = static_cast<DiagnosticSink*>(user_data);
    if (sink == nullptr) {
        return;
    }
    sink->emit({to_severity(severity), from_view(code), from_view(message)});
}

class DynamicLibrary {
  public:
    ~DynamicLibrary() {
        close();
    }

    VoidResult open(const std::filesystem::path& path) {
#if defined(_WIN32)
        handle_ = LoadLibraryW(path.wstring().c_str());
        if (handle_ == nullptr) {
            return std::unexpected(
                make_error("module.load_binary", "LoadLibrary failed for " + path_to_utf8(path)));
        }
#else
        handle_ = dlopen(path_to_utf8(path).c_str(), RTLD_NOW);
        if (handle_ == nullptr) {
            return std::unexpected(make_error("module.load_binary", dlerror()));
        }
#endif
        return {};
    }

    void* symbol(const char* name) const {
#if defined(_WIN32)
        return reinterpret_cast<void*>(GetProcAddress(static_cast<HMODULE>(handle_), name));
#else
        return dlsym(handle_, name);
#endif
    }

    void close() {
        if (handle_ == nullptr) {
            return;
        }
#if defined(_WIN32)
        FreeLibrary(static_cast<HMODULE>(handle_));
#else
        dlclose(handle_);
#endif
        handle_ = nullptr;
    }

  private:
    void* handle_ = nullptr;
};

} // namespace

struct ModuleManager::LoadedModule {
    ModuleDescriptor descriptor;
    CapabilitySet capabilities;
    PermissionSet permissions;
    DynamicLibrary library;
    AstraModuleApi api{};
};

ModuleManager::ModuleManager(ServiceRegistry& service_registry,
                             ExtensionRegistry& extension_registry)
    : service_registry_(service_registry), extension_registry_(extension_registry) {}

ModuleManager::~ModuleManager() {
    DiagnosticSink diagnostics;
    unload_all(diagnostics);
}

VoidResult ModuleManager::discover(const std::vector<std::filesystem::path>& plugin_roots,
                                   DiagnosticSink& diagnostics) {
    plugins_.clear();
    for (const auto& root : plugin_roots) {
        if (root.empty() || !std::filesystem::exists(root)) {
            continue;
        }
        for (const auto& entry : std::filesystem::recursive_directory_iterator(root)) {
            if (!entry.is_regular_file()) {
                continue;
            }
            const auto path = entry.path();
            if (!path.filename().string().ends_with(".plugin.yaml")) {
                continue;
            }
            auto descriptor = load_plugin_descriptor(path, diagnostics);
            if (!descriptor) {
                continue;
            }
            std::erase_if(descriptor->modules, [](const ModuleDescriptor& module) {
                return !supports_current_platform(module);
            });
            std::sort(descriptor->modules.begin(), descriptor->modules.end(),
                      [](const auto& lhs, const auto& rhs) {
                          if (load_phase_order(lhs.load_phase) !=
                              load_phase_order(rhs.load_phase)) {
                              return load_phase_order(lhs.load_phase) <
                                     load_phase_order(rhs.load_phase);
                          }
                          return lhs.id < rhs.id;
                      });
            plugins_.push_back(std::move(*descriptor));
        }
    }

    std::unordered_map<std::string, PluginDescriptor*> by_id;
    for (PluginDescriptor& plugin : plugins_) {
        if (!astra_api_supported(plugin.astra_api)) {
            diagnostics.error("module.astra_api_unsupported",
                              plugin.id + " requires Astra API " + plugin.astra_api);
        }
        if (by_id.contains(plugin.id)) {
            diagnostics.error("module.duplicate_plugin", "Duplicate plugin id: " + plugin.id);
        }
        by_id.emplace(plugin.id, &plugin);
    }
    for (const PluginDescriptor& plugin : plugins_) {
        for (const std::string& dependency : plugin.dependencies) {
            if (!by_id.contains(dependency)) {
                diagnostics.error("module.dependency_missing",
                                  plugin.id + " depends on missing plugin " + dependency);
            }
        }
    }
    if (diagnostics.has_errors()) {
        return std::unexpected(make_error("module.discover_failed", "Plugin discovery failed"));
    }

    std::vector<PluginDescriptor*> seeds;
    for (PluginDescriptor& plugin : plugins_) {
        seeds.push_back(&plugin);
    }
    std::sort(seeds.begin(), seeds.end(), [](const auto* lhs, const auto* rhs) {
        if (first_module_phase(*lhs) != first_module_phase(*rhs)) {
            return first_module_phase(*lhs) < first_module_phase(*rhs);
        }
        return lhs->id < rhs->id;
    });

    std::unordered_map<std::string, int> state;
    std::vector<PluginDescriptor> ordered;
    std::function<void(PluginDescriptor&)> visit = [&](PluginDescriptor& plugin) {
        const int current = state[plugin.id];
        if (current == 1) {
            diagnostics.error("module.dependency_cycle",
                              "Dependency cycle includes plugin " + plugin.id);
            return;
        }
        if (current == 2) {
            return;
        }
        state[plugin.id] = 1;
        for (const std::string& dependency : plugin.dependencies) {
            visit(*by_id[dependency]);
        }
        state[plugin.id] = 2;
        ordered.push_back(plugin);
    };
    for (PluginDescriptor* plugin : seeds) {
        visit(*plugin);
    }

    if (diagnostics.has_errors()) {
        return std::unexpected(make_error("module.discover_failed", "Plugin discovery failed"));
    }
    plugins_ = std::move(ordered);
    return {};
}

VoidResult ModuleManager::load_discovered(DiagnosticSink& diagnostics) {
    for (PluginDescriptor& plugin : plugins_) {
        for (ModuleDescriptor& module : plugin.modules) {
            if (!std::filesystem::exists(module.entrypoint)) {
                diagnostics.error("module.entrypoint_missing", module.id + " entrypoint missing: " +
                                                                   path_to_utf8(module.entrypoint));
                return std::unexpected(
                    make_error("module.entrypoint_missing", "Plugin entrypoint missing"));
            }

            auto loaded = std::make_unique<LoadedModule>();
            loaded->descriptor = module;
            loaded->capabilities = module.capabilities;
            loaded->permissions = module.permissions;
            if (auto opened = loaded->library.open(module.entrypoint); !opened) {
                diagnostics.error(opened.error().code, opened.error().message);
                return opened;
            }

            auto* symbol = loaded->library.symbol("astra_module_main");
            if (symbol == nullptr) {
                diagnostics.error("module.entrypoint_symbol_missing",
                                  module.id + " does not export astra_module_main");
                return std::unexpected(
                    make_error("module.entrypoint_symbol_missing", "Missing astra_module_main"));
            }

            AstraModuleHostApi host{};
            host.abi_version = ASTRA_MODULE_ABI_VERSION;
            host.host_context = this;
            host.diagnostics = {&diagnostics, emit_diagnostic};
            host.register_extension = &ModuleManager::register_extension_thunk;
            host.get_service = &ModuleManager::get_service_thunk;

            auto entrypoint = reinterpret_cast<AstraModuleMainFn>(symbol);
            active_module_ = &loaded->descriptor;
            active_diagnostics_ = &diagnostics;
            const AstraResultCode main_result = entrypoint(&host, &loaded->api);
            if (main_result != ASTRA_RESULT_OK ||
                loaded->api.abi_version != ASTRA_MODULE_ABI_VERSION) {
                active_module_ = nullptr;
                active_diagnostics_ = nullptr;
                diagnostics.error("module.abi_mismatch", module.id + " rejected ABI version");
                return std::unexpected(make_error("module.abi_mismatch", "ABI mismatch"));
            }

            if (loaded->api.initialize != nullptr &&
                loaded->api.initialize(loaded->api.module_context, &host) != ASTRA_RESULT_OK) {
                active_module_ = nullptr;
                active_diagnostics_ = nullptr;
                diagnostics.error("module.initialize_failed", module.id + " initialize failed");
                return std::unexpected(make_error("module.initialize_failed", "Initialize failed"));
            }
            if (loaded->api.activate != nullptr &&
                loaded->api.activate(loaded->api.module_context, &host) != ASTRA_RESULT_OK) {
                active_module_ = nullptr;
                active_diagnostics_ = nullptr;
                diagnostics.error("module.activate_failed", module.id + " activate failed");
                return std::unexpected(make_error("module.activate_failed", "Activate failed"));
            }
            active_module_ = nullptr;
            active_diagnostics_ = nullptr;
            loaded_modules_.push_back(std::move(loaded));
        }
    }
    return {};
}

void ModuleManager::unload_all(DiagnosticSink& diagnostics) {
    AstraModuleHostApi host{};
    host.abi_version = ASTRA_MODULE_ABI_VERSION;
    host.host_context = this;
    host.diagnostics = {&diagnostics, emit_diagnostic};
    host.register_extension = &ModuleManager::register_extension_thunk;
    host.get_service = &ModuleManager::get_service_thunk;

    for (auto it = loaded_modules_.rbegin(); it != loaded_modules_.rend(); ++it) {
        LoadedModule& loaded = **it;
        active_module_ = &loaded.descriptor;
        active_diagnostics_ = &diagnostics;
        if (loaded.api.deactivate != nullptr) {
            loaded.api.deactivate(loaded.api.module_context, &host);
        }
        if (loaded.api.shutdown != nullptr) {
            loaded.api.shutdown(loaded.api.module_context, &host);
        }
        active_module_ = nullptr;
        active_diagnostics_ = nullptr;
    }
    loaded_modules_.clear();
}

const std::vector<PluginDescriptor>& ModuleManager::discovered_plugins() const {
    return plugins_;
}

std::size_t ModuleManager::loaded_module_count() const {
    return loaded_modules_.size();
}

AstraResultCode ModuleManager::register_extension_thunk(void* host_context,
                                                        const AstraExtensionDescriptor* descriptor) {
    auto* manager = static_cast<ModuleManager*>(host_context);
    if (manager == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    return manager->register_extension_from_abi(descriptor);
}

AstraResultCode ModuleManager::get_service_thunk(void* host_context, AstraStringView service_id,
                                                 AstraOpaqueHandle* out_service) {
    auto* manager = static_cast<ModuleManager*>(host_context);
    if (manager == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    return manager->get_service_from_abi(service_id, out_service);
}

AstraResultCode
ModuleManager::register_extension_from_abi(const AstraExtensionDescriptor* descriptor) {
    if (descriptor == nullptr || active_module_ == nullptr || active_diagnostics_ == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    ExtensionDescriptor converted;
    converted.id = from_view(descriptor->id);
    converted.module_id = active_module_->id;
    converted.kind = to_extension_kind(descriptor->kind);
    converted.required_capability = from_view(descriptor->required_capability);
    converted.required_permission = from_view(descriptor->required_permission);

    auto result = extension_registry_.register_extension(
        converted, active_module_->capabilities, active_module_->permissions, *active_diagnostics_);
    return result ? ASTRA_RESULT_OK : ASTRA_RESULT_ERROR;
}

AstraResultCode ModuleManager::get_service_from_abi(AstraStringView service_id,
                                                    AstraOpaqueHandle* out_service) {
    if (active_module_ == nullptr || active_diagnostics_ == nullptr || out_service == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    void* service = service_registry_.resolve(from_view(service_id), active_module_->capabilities,
                                              active_module_->permissions, *active_diagnostics_);
    if (service == nullptr) {
        *out_service = nullptr;
        return ASTRA_RESULT_NOT_FOUND;
    }
    *out_service = service;
    return ASTRA_RESULT_OK;
}

} // namespace astra
