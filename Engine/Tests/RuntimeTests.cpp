#include <Astra/AssetCore/AssetId.h>
#include <Astra/AssetRegistry/AssetRegistry.h>
#include <Astra/AstraRuntime/AstraRuntimeSession.h>
#include <Astra/Core/Log.h>
#include <Astra/ExtensionRegistry/ExtensionRegistry.h>
#include <Astra/ModuleRuntime/ModuleManager.h>
#include <Astra/ModuleRuntime/PluginDescriptor.h>
#include <Astra/VNPropertySystem/VNPropertySystem.h>

#include <fmt/format.h>

#include <catch2/catch_test_macros.hpp>

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <string_view>
#include <unordered_set>

struct CountingLogArgument {
    int* format_count = nullptr;
};

template <> struct fmt::formatter<CountingLogArgument> {
    constexpr auto parse(fmt::format_parse_context& context) {
        return context.begin();
    }

    template <typename FormatContext>
    auto format(const CountingLogArgument& argument, FormatContext& context) const {
        ++(*argument.format_count);
        return fmt::format_to(context.out(), "formatted");
    }
};

namespace {

const astra::log::Category LogTest{"LogTest", astra::log::Verbosity::Log};

struct TestBootstrap {
    astra::RuntimeProviderRegistry providers;
    astra::ExtensionRegistry extensions;
    astra::ModuleManager modules;

    TestBootstrap() : modules(extensions, &providers) {}
};

void initialize_test_bootstrap(TestBootstrap& bootstrap, astra::DiagnosticSink& diagnostics) {
    REQUIRE(bootstrap.modules.discover({ASTRA_TEST_PLUGIN_ROOT}, diagnostics));
    REQUIRE(bootstrap.modules.load_discovered(diagnostics));
    REQUIRE(bootstrap.providers.platform_provider("astra.platform.sdl3"));
    REQUIRE(bootstrap.providers.renderer_provider("astra.renderer.sdl_gpu"));
    REQUIRE(bootstrap.providers.audio_provider("astra.audio.miniaudio"));
    REQUIRE(bootstrap.providers.project_content_provider("astra.content.default_project"));
}

struct ScopedLogShutdown {
    ScopedLogShutdown() {
        astra::log::shutdown();
    }

    ~ScopedLogShutdown() {
        astra::log::shutdown();
    }
};

std::filesystem::path make_log_test_dir(std::string_view name) {
    auto path = std::filesystem::temp_directory_path() / "AstraEngineLogTests" / std::string(name);
    std::filesystem::remove_all(path);
    std::filesystem::create_directories(path);
    return path;
}

std::string read_text_file(const std::filesystem::path& path) {
    std::ifstream input(path);
    std::ostringstream buffer;
    buffer << input.rdbuf();
    return buffer.str();
}

} // namespace

TEST_CASE("Log captures formatted records and filters before formatting") {
    ScopedLogShutdown log_shutdown;
    astra::log::InitializeOptions options;
    options.enable_console = false;
    options.enable_file = false;
    options.memory_capacity = 8;
    options.global_verbosity = astra::log::Verbosity::Log;
    astra::log::initialize(options);

    astra::log::info(LogTest, "value {}", 42);
    int format_count = 0;
    astra::log::verbose(LogTest, "{}", CountingLogArgument{&format_count});

    auto records = astra::log::recent_records();
    REQUIRE(records.size() == 1);
    REQUIRE(records[0].category == "LogTest");
    REQUIRE(records[0].verbosity == astra::log::Verbosity::Log);
    REQUIRE(records[0].message == "value 42");
    REQUIRE(format_count == 0);

    astra::log::set_global_verbosity(astra::log::Verbosity::Verbose);
    astra::log::set_category_verbosity(LogTest, astra::log::Verbosity::Verbose);
    astra::log::verbose(LogTest, "{}", CountingLogArgument{&format_count});

    records = astra::log::recent_records();
    REQUIRE(records.size() == 2);
    REQUIRE(records[1].verbosity == astra::log::Verbosity::Verbose);
    REQUIRE(records[1].message == "formatted");
    REQUIRE(format_count == 1);
}

TEST_CASE("Log writes UE-style category and verbosity to file") {
    ScopedLogShutdown log_shutdown;
    const auto log_dir = make_log_test_dir("FileSink");
    astra::log::InitializeOptions options;
    options.log_directory = log_dir;
    options.file_stem = "AstraLogTest";
    options.enable_console = false;
    options.enable_file = true;
    options.memory_capacity = 4;
    astra::log::initialize(options);

    astra::log::info(LogTest, "persisted {}", 7);
    astra::log::shutdown();

    const std::string content = read_text_file(log_dir / "AstraLogTest.log");
    REQUIRE(content.find("LogTest: Log: persisted 7") != std::string::npos);
}

TEST_CASE("Log maps diagnostics to categorized records") {
    ScopedLogShutdown log_shutdown;
    astra::log::InitializeOptions options;
    options.enable_console = false;
    options.enable_file = false;
    options.memory_capacity = 8;
    astra::log::initialize(options);

    astra::DiagnosticSink diagnostics;
    diagnostics.info("test.info", "Informational diagnostic");
    diagnostics.warning("test.warning", "Warning diagnostic");
    diagnostics.error("test.error", "Error diagnostic");
    astra::log::write_diagnostics(diagnostics);

    const auto records = astra::log::recent_records();
    REQUIRE(records.size() == 3);
    REQUIRE(records[0].category == "LogDiagnostics");
    REQUIRE(records[0].verbosity == astra::log::Verbosity::Display);
    REQUIRE(records[0].message == "test.info: Informational diagnostic");
    REQUIRE(records[1].verbosity == astra::log::Verbosity::Warning);
    REQUIRE(records[1].message == "test.warning: Warning diagnostic");
    REQUIRE(records[2].verbosity == astra::log::Verbosity::Error);
    REQUIRE(records[2].message == "test.error: Error diagnostic");
}

TEST_CASE("AssetId validates stable schemes") {
    auto id = astra::AssetId::parse("native:/Backgrounds/RooftopEvening");
    REQUIRE(id);
    REQUIRE(id->scheme() == "native");
    REQUIRE_FALSE(astra::AssetId::parse("not-an-id").has_value());
}

TEST_CASE("AssetRegistry loads MinimalVN sidecars") {
    astra::DiagnosticSink diagnostics;
    astra::AssetRegistry registry;
    auto result = registry.load_sidecars(std::filesystem::path(ASTRA_TEST_PROJECT_ROOT) / "Content",
                                         diagnostics);
    REQUIRE(result);
    REQUIRE(registry.size() >= 4);
}

TEST_CASE("ExtensionRegistry diagnoses duplicate registrations") {
    astra::DiagnosticSink diagnostics;
    astra::ExtensionRegistry registry;
    std::unordered_set<std::string> caps{"runtime_command_source"};
    std::unordered_set<std::string> perms;
    astra::ExtensionDescriptor descriptor{"example.source", "example.module",
                                          astra::ExtensionKind::RuntimeCommandSource,
                                          "runtime_command_source", ""};
    REQUIRE(registry.register_extension(descriptor, caps, perms, diagnostics));
    REQUIRE_FALSE(registry.register_extension(descriptor, caps, perms, diagnostics).has_value());
    REQUIRE(diagnostics.has_errors());
}

TEST_CASE("VNPropertySystem emits JSON Schema metadata") {
    astra::DiagnosticSink diagnostics;
    astra::VNPropertyRegistry registry;
    astra::VNTypeDescriptor type;
    type.type_id = "astra.test.transition";
    type.display_name = "Transition";
    type.properties.push_back(
        {"kind", astra::VNPropertyKind::Enum, "fade", true, false, false, false, {"cut", "fade"}});
    REQUIRE(registry.register_type(type, diagnostics));
    auto schema = registry.generate_json_schema(type.type_id);
    REQUIRE(schema["properties"]["kind"]["enum"].size() == 2);
    REQUIRE(schema["properties"]["kind"]["x-astra-ai-editable"].get<bool>());
}

TEST_CASE("ModuleManager loads a real dynamic plugin and registers extensions") {
    astra::DiagnosticSink diagnostics;
    astra::ExtensionRegistry registry;
    astra::ModuleManager manager(registry);
    REQUIRE(manager.discover({std::filesystem::path(ASTRA_TEST_PLUGIN_ROOT) / "ExampleRuntime"},
                             diagnostics));
    REQUIRE(manager.discovered_plugins().size() == 1);
    REQUIRE(manager.load_discovered(diagnostics));
    REQUIRE(manager.loaded_module_count() == 1);
    REQUIRE(registry.contains("astra.example.runtime_command_source"));
    REQUIRE(registry.contains("astra.example.vn_property_type"));
    manager.unload_all(diagnostics);
}

TEST_CASE("ModuleManager loads runtime providers through plugin discovery") {
    astra::DiagnosticSink diagnostics;
    TestBootstrap bootstrap;
    initialize_test_bootstrap(bootstrap, diagnostics);
    REQUIRE(bootstrap.modules.loaded_module_count() >= 2);
    REQUIRE(bootstrap.extensions.contains("astra.provider.platform.sdl3"));
    REQUIRE(bootstrap.extensions.contains("astra.provider.renderer.sdl_gpu"));
    REQUIRE(bootstrap.extensions.contains("astra.provider.audio.miniaudio"));
    REQUIRE(bootstrap.extensions.contains("astra.provider.content.default_project"));
    bootstrap.modules.unload_all(diagnostics);
}

TEST_CASE("AstraRuntimeSession runs MinimalVN headlessly") {
    astra::DiagnosticSink diagnostics;
    TestBootstrap bootstrap;
    initialize_test_bootstrap(bootstrap, diagnostics);
    astra::AstraRuntimeSession session(bootstrap.providers, bootstrap.extensions);
    REQUIRE(session.load_project(ASTRA_TEST_PROJECT_ROOT, diagnostics));
    REQUIRE(session.start(diagnostics));
    auto first = session.render_snapshot();
    REQUIRE(first.dialogue == "You finally came.");
    REQUIRE(session.advance(diagnostics));
    auto choice = session.render_snapshot();
    REQUIRE(choice.choices.size() == 2);
    REQUIRE(session.choose(0, diagnostics));
    auto after = session.render_snapshot();
    REQUIRE(after.dialogue == "Then stay for a moment.");
    REQUIRE(after.variables.at("affection.alice") == 1);
    auto snapshot = session.save_snapshot();
    astra::AstraRuntimeSession restored(bootstrap.providers, bootstrap.extensions);
    restored.restore_snapshot(snapshot);
    REQUIRE(restored.render_snapshot().dialogue == "Then stay for a moment.");
    session.shutdown(diagnostics);
    bootstrap.modules.unload_all(diagnostics);
}
