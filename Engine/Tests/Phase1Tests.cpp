#include <Astra/Core/Config.h>
#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Path.h>
#include <Astra/ModuleRuntime/ExtensionRegistry.h>
#include <Astra/ModuleRuntime/ModuleManager.h>
#include <Astra/ModuleRuntime/PluginDescriptor.h>
#include <Astra/ModuleRuntime/ServiceRegistry.h>
#include <Astra/Platform/PlatformServices.h>
#include <Astra/PropertySystem/PropertySystem.h>

#include <catch2/catch_test_macros.hpp>

#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <unordered_set>

namespace {

#ifndef ASTRA_TEST_PLUGIN_ROOT
#define ASTRA_TEST_PLUGIN_ROOT ""
#endif

astra::VoidResult register_test_services(
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

std::filesystem::path temp_file(std::string_view name) {
    return std::filesystem::temp_directory_path() / std::filesystem::path(name);
}

std::filesystem::path plugin_root_from_env() {
    char* value = nullptr;
    std::size_t length = 0;
#if defined(_WIN32)
    if (_dupenv_s(&value, &length, "ASTRA_TEST_PLUGIN_ROOT") != 0) {
        value = nullptr;
    }
#else
    value = std::getenv("ASTRA_TEST_PLUGIN_ROOT");
    length = value == nullptr ? 0 : std::strlen(value);
#endif
    if (value == nullptr || length == 0 || value[0] == '\0') {
#if defined(_WIN32)
        free(value);
#endif
        return astra::utf8_to_path(ASTRA_TEST_PLUGIN_ROOT);
    }
    const auto path = astra::utf8_to_path(value);
#if defined(_WIN32)
    free(value);
#endif
    return path;
}

} // namespace

TEST_CASE("DiagnosticSink tracks emitted errors", "[core]") {
    astra::DiagnosticSink diagnostics;
    diagnostics.info("core.info", "hello");
    diagnostics.error("core.error", "bad");

    REQUIRE(diagnostics.diagnostics().size() == 2);
    REQUIRE(diagnostics.has_errors());
}

TEST_CASE("Path UTF-8 roundtrip preserves value", "[core]") {
    const auto original = astra::utf8_to_path("Phase1/Alpha/Beta.txt");
    const auto roundtrip = astra::utf8_to_path(astra::path_to_utf8(original));

    REQUIRE(astra::normalize_path(roundtrip) == astra::normalize_path(original));
}

TEST_CASE("Config loads YAML as JSON", "[core]") {
    const auto file = temp_file("astra_phase1_config.yaml");
    {
        std::ofstream output(file);
        output << "name: Phase1\ncount: 3\nenabled: true\n";
    }

    astra::DiagnosticSink diagnostics;
    auto json = astra::load_yaml_file_as_json(file, diagnostics);

    REQUIRE(json.has_value());
    REQUIRE((*json)["name"] == "Phase1");
    REQUIRE((*json)["count"] == 3);
    REQUIRE((*json)["enabled"] == true);
    REQUIRE_FALSE(diagnostics.has_errors());

    std::filesystem::remove(file);
}

TEST_CASE("Platform services initialize without a window", "[platform]") {
    astra::DiagnosticSink diagnostics;
    auto platform = astra::create_default_platform_services(diagnostics);

    REQUIRE(platform.has_value());
    REQUIRE((*platform).input != nullptr);
    REQUIRE((*platform).filesystem != nullptr);
    REQUIRE((*platform).timer != nullptr);
    REQUIRE((*platform).thread != nullptr);
    REQUIRE((*platform).dynamic_library != nullptr);
    REQUIRE((*platform).timer->now_seconds() >= 0.0);
    REQUIRE((*platform).thread->hardware_concurrency() >= 1u);
}

TEST_CASE("ServiceRegistry enforces required permissions", "[module]") {
    astra::ServiceRegistry registry;
    astra::DiagnosticSink diagnostics;
    int value = 42;

    REQUIRE(registry
                .register_service(
                    {"astra.test.secure", &value, "service_extension", "host.inspect"},
                    diagnostics)
                .has_value());

    const std::unordered_set<std::string> capabilities = {"service_extension"};
    const std::unordered_set<std::string> no_permissions;
    REQUIRE(registry.resolve("astra.test.secure", capabilities, no_permissions, diagnostics) ==
            nullptr);

    const std::unordered_set<std::string> permissions = {"host.inspect"};
    REQUIRE(registry.resolve("astra.test.secure", capabilities, permissions, diagnostics) ==
            &value);
}

TEST_CASE("ExtensionRegistry filters duplicate ids and kinds", "[module]") {
    astra::ExtensionRegistry registry;
    astra::DiagnosticSink diagnostics;
    const std::unordered_set<std::string> capabilities = {"service_extension",
                                                          "property_type_provider"};
    const std::unordered_set<std::string> permissions;

    REQUIRE(registry
                .register_extension({"ext.service", "module.alpha",
                                     astra::ExtensionKind::ServiceExtension, "", ""},
                                    capabilities, permissions, diagnostics)
                .has_value());
    REQUIRE(registry
                .register_extension({"ext.property", "module.alpha",
                                     astra::ExtensionKind::PropertyTypeProvider, "", ""},
                                    capabilities, permissions, diagnostics)
                .has_value());
    REQUIRE_FALSE(registry
                      .register_extension({"ext.service", "module.beta",
                                           astra::ExtensionKind::ServiceExtension, "", ""},
                                          capabilities, permissions, diagnostics)
                      .has_value());
    REQUIRE(registry.list(astra::ExtensionKind::ServiceExtension).size() == 1);
    REQUIRE(registry.list(astra::ExtensionKind::PropertyTypeProvider).size() == 1);
}

TEST_CASE("Plugin descriptor parses simplified phase1 schema", "[module]") {
    const auto file = temp_file("astra_phase1.plugin.yaml");
    {
        std::ofstream output(file);
        output << "id: astra.plugin.test_phase1\n";
        output << "display_name: Test Phase1\n";
        output << "version: 0.1.0\n";
        output << "astra_api: \">=0.2.0 <0.3.0\"\n";
        output << "modules:\n";
        output << "  - id: test.runtime\n";
        output << "    type: runtime\n";
        output << "    entrypoint: TestModule.dll\n";
        output << "    load_phase: runtime_startup\n";
        output << "    capabilities:\n";
        output << "      - service_extension\n";
    }

    astra::DiagnosticSink diagnostics;
    auto descriptor = astra::load_plugin_descriptor(file, diagnostics);

    REQUIRE(descriptor.has_value());
    REQUIRE(descriptor->id == "astra.plugin.test_phase1");
    REQUIRE(descriptor->modules.size() == 1);
    REQUIRE(descriptor->modules.front().capabilities.contains("service_extension"));

    std::filesystem::remove(file);
}

TEST_CASE("PropertyRegistry generates JSON schema with flags", "[property]") {
    astra::PropertyRegistry registry;
    astra::DiagnosticSink diagnostics;
    astra::TypeDescriptor descriptor;
    descriptor.id = "astra.type.character";
    descriptor.display_name = "Character";
    descriptor.properties = {
        {"name", astra::PropertyTypeKind::String, "Astra", astra::PropertyFlags::AiEditable, {}, ""},
        {"mood", astra::PropertyTypeKind::Enum, "calm",
         astra::PropertyFlags::RequiresReview, {"calm", "angry"}, ""},
    };

    REQUIRE(registry.register_type(std::move(descriptor), diagnostics).has_value());

    const auto schema = registry.generate_json_schema("astra.type.character");
    REQUIRE(schema["properties"]["name"]["type"] == "string");
    REQUIRE(schema["properties"]["name"]["x-astra-flags"]["ai_editable"] == true);
    REQUIRE(schema["properties"]["mood"]["enum"].size() == 2);
    REQUIRE(schema["properties"]["mood"]["x-astra-flags"]["requires_review"] == true);
}

TEST_CASE("ModuleManager loads phase1 example module", "[module][smoke]") {
    const auto plugin_root = plugin_root_from_env();
    REQUIRE_FALSE(plugin_root.empty());

    astra::DiagnosticSink diagnostics;
    auto platform = astra::create_default_platform_services(diagnostics);
    REQUIRE(platform.has_value());

    astra::ServiceRegistry service_registry;
    astra::ExtensionRegistry extension_registry;
    astra::PropertyRegistry property_registry;
    REQUIRE(register_test_services(service_registry, *platform, property_registry, diagnostics)
                .has_value());

    astra::ModuleManager manager(service_registry, extension_registry);
    REQUIRE(manager.discover({plugin_root}, diagnostics).has_value());
    REQUIRE(manager.load_discovered(diagnostics).has_value());
    REQUIRE(manager.loaded_module_count() == 1);
    REQUIRE(extension_registry.contains("phase1_example.service_extension"));
    REQUIRE(extension_registry.contains("phase1_example.property_type_provider"));

    manager.unload_all(diagnostics);
    REQUIRE_FALSE(diagnostics.has_errors());
}
