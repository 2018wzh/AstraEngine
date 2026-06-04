#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Log.h>
#include <Astra/Core/Path.h>
#include <Astra/ModuleRuntime/ExtensionRegistry.h>
#include <Astra/ModuleRuntime/ModuleManager.h>
#include <Astra/ModuleRuntime/ServiceRegistry.h>
#include <Astra/Platform/PlatformServices.h>
#include <Astra/PropertySystem/PropertySystem.h>

#include <filesystem>
#include <iostream>
#include <memory>
#include <unordered_set>
#include <vector>

namespace {

#ifndef ASTRA_DEFAULT_PLUGIN_ROOT
#define ASTRA_DEFAULT_PLUGIN_ROOT ""
#endif

int fail_with_diagnostics(const astra::DiagnosticSink& diagnostics) {
    astra::log::write_diagnostics(diagnostics, astra::log::LogModule);
    for (const auto& diagnostic : diagnostics.diagnostics()) {
        std::cerr << diagnostic.code << ": " << diagnostic.message << '\n';
    }
    return 1;
}

astra::VoidResult register_host_services(
    astra::ServiceRegistry& registry, const astra::PlatformServicesBundle& platform,
    astra::PropertyRegistry& property_registry, astra::DiagnosticSink& diagnostics) {
    if (auto result = registry.register_service(
            {astra::kWindowServiceId, platform.window.get(), "", ""}, diagnostics);
        !result) {
        return result;
    }
    if (auto result = registry.register_service(
            {astra::kInputServiceId, platform.input.get(), "", ""}, diagnostics);
        !result) {
        return result;
    }
    if (auto result = registry.register_service(
            {astra::kFileSystemServiceId, platform.filesystem.get(), "", ""}, diagnostics);
        !result) {
        return result;
    }
    if (auto result = registry.register_service(
            {astra::kTimerServiceId, platform.timer.get(), "", ""}, diagnostics);
        !result) {
        return result;
    }
    if (auto result = registry.register_service(
            {astra::kThreadServiceId, platform.thread.get(), "", ""}, diagnostics);
        !result) {
        return result;
    }
    if (auto result = registry.register_service(
            {astra::kDynamicLibraryServiceId, platform.dynamic_library.get(), "", ""},
            diagnostics);
        !result) {
        return result;
    }
    if (auto result = registry.register_service(
            {"astra.property.registry", &property_registry, "property_type_provider", ""},
            diagnostics);
        !result) {
        return result;
    }
    return {};
}

} // namespace

int main(int argc, char** argv) {
    astra::log::initialize({
        .enable_console = true,
        .enable_file = false,
    });

    astra::DiagnosticSink diagnostics;
    auto platform = astra::create_default_platform_services(diagnostics);
    if (!platform) {
        return fail_with_diagnostics(diagnostics);
    }

    astra::ServiceRegistry service_registry;
    astra::ExtensionRegistry extension_registry;
    astra::PropertyRegistry property_registry;

    if (auto result = register_host_services(service_registry, *platform, property_registry,
                                             diagnostics);
        !result) {
        return fail_with_diagnostics(diagnostics);
    }

    std::filesystem::path plugin_root;
    if (argc > 1) {
        plugin_root = astra::utf8_to_path(argv[1]);
    } else {
        plugin_root = astra::utf8_to_path(ASTRA_DEFAULT_PLUGIN_ROOT);
        if (plugin_root.empty()) {
            plugin_root = std::filesystem::current_path() / "Engine" / "Plugins" / "Examples";
        }
    }

    astra::ModuleManager module_manager(service_registry, extension_registry);
    if (auto result = module_manager.discover({plugin_root}, diagnostics); !result) {
        return fail_with_diagnostics(diagnostics);
    }
    if (auto result = module_manager.load_discovered(diagnostics); !result) {
        return fail_with_diagnostics(diagnostics);
    }

    std::cout << "Loaded modules: " << module_manager.loaded_module_count() << '\n';
    for (const auto& extension : extension_registry.list()) {
        std::cout << extension.id << " [" << astra::extension_kind_to_string(extension.kind)
                  << "] from " << extension.module_id << '\n';
    }

    module_manager.unload_all(diagnostics);
    if (diagnostics.has_errors()) {
        return fail_with_diagnostics(diagnostics);
    }

    astra::log::shutdown();
    return 0;
}
