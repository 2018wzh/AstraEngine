#include <Astra/ModuleRuntime/ModuleManager.h>

#include <Astra/Bootstrap/NativeRuntimePlugin.h>
#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/Core/Path.h>

#include <algorithm>
#include <cstring>

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
    return static_cast<ExtensionKind>(static_cast<std::uint32_t>(kind));
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
    DynamicLibrary() = default;
    ~DynamicLibrary() {
        close();
    }

    DynamicLibrary(const DynamicLibrary&) = delete;
    DynamicLibrary& operator=(const DynamicLibrary&) = delete;

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
    DynamicLibrary library;
    AstraModuleApi api{};
};

ModuleManager::ModuleManager(ExtensionRegistry& extension_registry,
                             RuntimeProviderRegistry* runtime_provider_registry)
    : extension_registry_(extension_registry),
      runtime_provider_registry_(runtime_provider_registry) {}

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
            plugins_.push_back(std::move(*descriptor));
        }
    }

    std::sort(plugins_.begin(), plugins_.end(), [](const auto& lhs, const auto& rhs) {
        const int lhs_phase =
            lhs.modules.empty() ? 0 : load_phase_order(lhs.modules.front().load_phase);
        const int rhs_phase =
            rhs.modules.empty() ? 0 : load_phase_order(rhs.modules.front().load_phase);
        return lhs_phase < rhs_phase;
    });

    if (diagnostics.has_errors()) {
        return std::unexpected(make_error("module.discover_failed", "Plugin discovery failed"));
    }
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

            auto entrypoint = reinterpret_cast<AstraModuleMainFn>(symbol);
            active_module_ = &module;
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

            if (runtime_provider_registry_ != nullptr) {
                auto* native_symbol = loaded->library.symbol(kNativeRuntimeProviderEntrypoint);
                if (native_symbol != nullptr) {
                    auto native_entry =
                        reinterpret_cast<AstraNativeRuntimeProviderEntryFn>(native_symbol);
                    if (!native_entry(runtime_provider_registry_, &diagnostics)) {
                        runtime_provider_registry_->clear();
                        diagnostics.error("module.native_provider_register_failed",
                                          module.id +
                                              " native runtime provider registration failed");
                        return std::unexpected(
                            make_error("module.native_provider_register_failed",
                                       "Native runtime provider registration failed"));
                    }
                }
            }

            loaded_modules_.push_back(std::move(loaded));
        }
    }
    return {};
}

void ModuleManager::unload_all(DiagnosticSink& diagnostics) {
    if (runtime_provider_registry_ != nullptr) {
        runtime_provider_registry_->clear();
    }

    AstraModuleHostApi host{};
    host.abi_version = ASTRA_MODULE_ABI_VERSION;
    host.host_context = this;
    host.diagnostics = {&diagnostics, emit_diagnostic};
    host.register_extension = &ModuleManager::register_extension_thunk;

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

AstraResultCode
ModuleManager::register_extension_thunk(void* host_context,
                                        const AstraExtensionDescriptor* descriptor) {
    auto* manager = static_cast<ModuleManager*>(host_context);
    if (manager == nullptr) {
        return ASTRA_RESULT_INVALID_ARGUMENT;
    }
    return manager->register_extension_from_abi(descriptor);
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

} // namespace astra
