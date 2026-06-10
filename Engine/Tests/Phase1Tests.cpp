#include <Astra/Asset/Asset.hpp>
#include <Astra/AstraVN/AstraVN.hpp>
#include <Astra/Core/Config.hpp>
#include <Astra/Core/BuildInfo.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Error.hpp>
#include <Astra/Core/Logging.hpp>
#include <Astra/Core/Path.hpp>
#include <Astra/Core/Profiling.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Time.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Media/Media.hpp>
#include <Astra/ModuleRuntime/ModuleRuntime.hpp>
#include <Astra/Platform/Platform.hpp>
#include <Astra/PropertySystem/PropertySystem.hpp>
#include <Astra/Runtime/Runtime.hpp>
#include <Astra/Scene/Scene.hpp>
#include <Astra/Script/Script.hpp>
#if defined(ASTRA_WITH_TOOLS)
#include <Astra/Tools/Tools.hpp>
#endif

#include <catch2/catch_test_macros.hpp>

#include <filesystem>
#include <fstream>
#include <vector>

namespace {

std::vector<Astra::Core::u8> TestPng1x1Rgba() {
    return {
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
        0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
        0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41,
        0x54, 0x78, 0xda, 0x63, 0xf8, 0xff, 0xff, 0x3f,
        0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x35, 0x81,
        0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
        0x44, 0xae, 0x42, 0x60, 0x82,
    };
}

} // namespace

TEST_CASE("Diagnostics serialize severity and block release") {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_TEST_001";
    diagnostic.category = "test";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
    diagnostic.message = "blocking diagnostic";
    REQUIRE(diagnostic.BlocksRelease());
    REQUIRE(Astra::Core::ToJson(diagnostic).at("severity") == "blocking");
}

TEST_CASE("Config stack layers and hashes resolved values") {
    Astra::Core::ConfigStack stack;
    stack.AddLayer({Astra::Core::ConfigScope::EngineDefault, {{"runtime", {{"fixed_step_hz", 60}, {"deterministic", false}}}}});
    stack.AddLayer({Astra::Core::ConfigScope::ReleaseProfile, {{"runtime", {{"deterministic", true}}}}});
    auto resolved = stack.Resolve();
    REQUIRE(resolved["runtime"]["fixed_step_hz"] == 60);
    REQUIRE(resolved["runtime"]["deterministic"] == true);
    REQUIRE_FALSE(stack.Hash().empty());
}

TEST_CASE("Config release profile excludes user overrides and keeps command line hash") {
    Astra::Core::ConfigStack stack;
    stack.AddLayer({Astra::Core::ConfigScope::EngineDefault, {{"runtime", {{"deterministic", false}}}}});
    stack.AddLayer({Astra::Core::ConfigScope::UserOverride, {{"runtime", {{"deterministic", false}}}, {"editor", {{"theme", "local"}}}}});
    stack.AddLayer({Astra::Core::ConfigScope::ReleaseProfile, {{"runtime", {{"deterministic", true}}}}});
    stack.AddLayer({Astra::Core::ConfigScope::CommandLine, {{"release", {{"profile", "deterministic"}}}}});
    auto resolved = stack.ResolveForProfile(Astra::Core::ConfigResolveProfile::Release);
    REQUIRE(resolved.values["runtime"]["deterministic"] == true);
    REQUIRE_FALSE(resolved.values.contains("editor"));
    REQUIRE(resolved.values["release"]["profile"] == "deterministic");
    REQUIRE_FALSE(resolved.user_overrides_included);
    REQUIRE(resolved.hash == stack.HashForProfile(Astra::Core::ConfigResolveProfile::Release));
}

TEST_CASE("Diagnostic registry and release policy produce foundation gate reports") {
    Astra::Core::DiagnosticCodeRegistry registry;
    REQUIRE(registry.Register({"ASTRA_TEST_BLOCK", "test", Astra::Core::DiagnosticSeverity::Blocking, true}));
    REQUIRE(registry.Register({"ASTRA_TEST_ERROR", "test", Astra::Core::DiagnosticSeverity::Error, true}));
    REQUIRE_FALSE(registry.Register({"ASTRA_TEST_BLOCK", "test", Astra::Core::DiagnosticSeverity::Blocking, true}));

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_TEST_BLOCK";
    diagnostic.category = "test";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
    diagnostic.message = "blocked";
    diagnostics.Emit(std::move(diagnostic));
    auto report = Astra::Core::EvaluateFoundationGate(diagnostics, registry, {Astra::Core::ReleaseProfile::Shipping, true, true});
    REQUIRE_FALSE(report.passed);
    REQUIRE(report.registered_codes[0] == "ASTRA_TEST_BLOCK");
    REQUIRE(Astra::Core::ToJson(report)["schema"] == "astra.foundation.gate.v1");

    Astra::Core::DiagnosticSink threshold_diagnostics;
    Astra::Core::Diagnostic error;
    error.code = "ASTRA_TEST_ERROR";
    error.category = "test";
    error.severity = Astra::Core::DiagnosticSeverity::Error;
    error.message = "registered threshold failed";
    threshold_diagnostics.Emit(std::move(error));
    auto threshold_report = Astra::Core::EvaluateFoundationGate(threshold_diagnostics, registry, {Astra::Core::ReleaseProfile::Deterministic, false, true});
    REQUIRE_FALSE(threshold_report.passed);

    Astra::Core::DiagnosticSink unregistered_diagnostics;
    Astra::Core::Diagnostic unregistered;
    unregistered.code = "ASTRA_TEST_UNREGISTERED";
    unregistered.category = "test";
    unregistered.severity = Astra::Core::DiagnosticSeverity::Warning;
    unregistered.message = "missing registry entry";
    unregistered_diagnostics.Emit(std::move(unregistered));
    auto unregistered_report = Astra::Core::EvaluateFoundationGate(unregistered_diagnostics, registry, {Astra::Core::ReleaseProfile::Deterministic, false, true});
    REQUIRE_FALSE(unregistered_report.passed);
    REQUIRE(unregistered_report.diagnostics[0].code == "ASTRA_DIAGNOSTIC_CODE_UNREGISTERED");
}

TEST_CASE("Build info reports version config features and ABI") {
    auto info = Astra::Core::GetBuildInfo();
    REQUIRE_FALSE(info.engine_version.empty());
    REQUIRE_FALSE(info.build_config.empty());
    REQUIRE(info.abi_version == 1);
    REQUIRE_FALSE(info.EnabledFeatures().empty());
}

TEST_CASE("Stable ids parse normalize and hash") {
    auto id = Astra::Core::ParseStableId("asset:/Characters\\\\Alice");
    REQUIRE(id);
    REQUIRE(id.Value().ToString() == "asset:/Characters/Alice");
    REQUIRE(std::hash<Astra::Core::StableId>()(id.Value()) != 0);
}

TEST_CASE("Core virtual path and fixed step time helpers are stable") {
    auto path = Astra::Core::ParseVirtualPath("project:/Config/settings.yaml");
    REQUIRE(path);
    REQUIRE(path.Value().ToString() == "project:/Config/settings.yaml");

    Astra::Core::FixedStepClock clock(0.5);
    clock.Tick();
    REQUIRE(clock.State().frame_index == 1);
    REQUIRE(clock.State().elapsed_seconds == 0.5);
}

TEST_CASE("Migration registry applies sequential rules") {
    Astra::Core::MigrationRegistry registry;
    Astra::Core::MigrationRule rule;
    rule.schema = "astra.test.v1";
    rule.from_version = 1;
    rule.to_version = 2;
    rule.migrate = [](const nlohmann::json& input) {
        auto output = input;
        output["migrated"] = true;
        return output;
    };
    registry.Register(std::move(rule));
    Astra::Core::DiagnosticSink diagnostics;
    auto migrated = registry.Migrate({"astra.test.v1", 1, "object:/one", {{"name", "demo"}}}, 2, diagnostics);
    REQUIRE(migrated);
    REQUIRE(migrated.Value().version == 2);
    REQUIRE(migrated.Value().payload["migrated"] == true);
}

TEST_CASE("Migration unknown field policies preserve warn error and drop") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Core::MigrationRule rule;
    rule.schema = "astra.test";
    rule.from_version = 1;
    rule.to_version = 2;
    rule.known_fields_after_migration = {"known"};
    rule.diagnostic_code = "ASTRA_CORE_UNKNOWN_FIELD";

    nlohmann::json preserve = {{"known", true}, {"future", 1}};
    rule.unknown_field_policy = Astra::Core::UnknownFieldPolicy::Preserve;
    auto preserved = Astra::Core::ApplyUnknownFieldPolicy(preserve, rule, diagnostics);
    REQUIRE(preserved.unknown_fields.size() == 1);
    REQUIRE(preserve.contains("future"));

    nlohmann::json warn = {{"known", true}, {"future", 1}};
    rule.unknown_field_policy = Astra::Core::UnknownFieldPolicy::Warn;
    auto warned = Astra::Core::ApplyUnknownFieldPolicy(warn, rule, diagnostics);
    REQUIRE_FALSE(warned.blocking);
    REQUIRE(diagnostics.Diagnostics().back().severity == Astra::Core::DiagnosticSeverity::Warning);

    nlohmann::json drop = {{"known", true}, {"future", 1}};
    rule.unknown_field_policy = Astra::Core::UnknownFieldPolicy::Drop;
    auto dropped = Astra::Core::ApplyUnknownFieldPolicy(drop, rule, diagnostics);
    REQUIRE(dropped.unknown_fields.size() == 1);
    REQUIRE_FALSE(drop.contains("future"));

    nlohmann::json error = {{"known", true}, {"future", 1}};
    rule.unknown_field_policy = Astra::Core::UnknownFieldPolicy::Error;
    auto errored = Astra::Core::ApplyUnknownFieldPolicy(error, rule, diagnostics);
    REQUIRE(errored.blocking);
}

TEST_CASE("Core error reports and profiling markers are runtime independent") {
    Astra::Core::ErrorReporter reporter;
    auto fatal = reporter.MakeFatal(Astra::Core::ErrorCode::InternalError, "fatal test");
    REQUIRE(fatal.diagnostic.BlocksRelease());
    REQUIRE(fatal.diagnostic.code == "ASTRA_CORE_FATAL");

    auto assertion = reporter.MakeDeveloperAssert("x != nullptr", "null pointer");
    REQUIRE(assertion.diagnostic.context.at("expression") == "x != nullptr");

    Astra::Core::ProfilingCapture capture;
    capture.Begin("phase1", "test", 10);
    capture.End("phase1", 25);
    REQUIRE(capture.Markers().size() == 1);
    REQUIRE(capture.Markers()[0].end_ns == 25);
}

TEST_CASE("Core logging supports structured memory and rotating file sinks") {
    Astra::Core::LogEvent event;
    event.monotonic_ns = 1;
    event.channel = "test";
    event.message = "hello";
    event.fields["key"] = "value";

    Astra::Core::MemoryLogSink memory;
    memory.Write(event);
    REQUIRE(memory.Events().size() == 1);
    REQUIRE(Astra::Core::ToJson(memory.Events()[0])["fields"]["key"] == "value");

    const auto log_path = std::filesystem::temp_directory_path() / "astra_phase1_log.jsonl";
    const auto rotated_path = std::filesystem::path(log_path.string() + ".1");
    std::filesystem::remove(log_path);
    std::filesystem::remove(rotated_path);
    Astra::Core::FileLogSink file(log_path.string(), 1);
    file.Write(event);
    file.Write(event);
    REQUIRE(std::filesystem::exists(rotated_path));
}

TEST_CASE("Property system generates schema validates defaults and migrates") {
    Astra::PropertySystem::TypeRegistry registry;
    Astra::PropertySystem::PropertyDescriptor display_name;
    display_name.id = "display_name";
    display_name.type = "string";
    display_name.kind = Astra::PropertySystem::TypeKind::LocalizedText;
    display_name.flags = Astra::PropertySystem::PropertyFlag::RequiresReview;
    display_name.default_value = "Unknown";
    display_name.audit_label = "Display Name";

    Astra::PropertySystem::PropertyDescriptor age;
    age.id = "age";
    age.type = "integer";
    age.default_value = 0;
    age.validation.required = true;
    age.validation.minimum = 0.0;
    age.validation.custom_validator = "positive";

    Astra::PropertySystem::TypeDescriptor character;
    character.type_id = "astra.test.character";
    character.properties = {display_name, age};
    registry.RegisterValidator("positive", [](const nlohmann::json& value) {
        if (value.get<int>() < 0) {
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "value must be positive");
        }
        return Astra::Core::Result<void>::Success();
    });
    REQUIRE(registry.Register(std::move(character)));

    Astra::Core::DiagnosticSink diagnostics;
    auto schema = registry.GenerateJsonSchema("astra.test.character", diagnostics);
    REQUIRE(schema["properties"].contains("display_name"));
    REQUIRE(schema["required"][0] == "age");

    auto migrated = registry.ApplyMigration({{"name", "Alice"}}, {{"name", "display_name", {}, false}, {"", "age", 1, false}});
    REQUIRE(migrated["display_name"] == "Alice");
    REQUIRE(migrated["age"] == 1);
    auto validated = registry.Validate("astra.test.character", migrated, diagnostics);
    REQUIRE(validated);
    auto diffs = registry.Diff("astra.test.character", {{"display_name", "Alice"}}, {{"display_name", "Bob"}});
    REQUIRE(diffs.size() == 1);
    REQUIRE(diffs[0].requires_review);
}

TEST_CASE("Property system validates nested schema versions and write policy") {
    Astra::PropertySystem::TypeRegistry registry;
    Astra::PropertySystem::TypeDescriptor nested;
    nested.type_id = "astra.test.nested";
    Astra::PropertySystem::PropertyDescriptor nested_value;
    nested_value.id = "value";
    nested_value.type = "string";
    nested_value.kind = Astra::PropertySystem::TypeKind::Scalar;
    nested.properties.push_back(std::move(nested_value));
    REQUIRE(registry.Register(std::move(nested)));

    Astra::PropertySystem::PropertyDescriptor nested_property;
    nested_property.id = "nested";
    nested_property.type = "astra.test.nested";
    nested_property.kind = Astra::PropertySystem::TypeKind::Struct;

    Astra::PropertySystem::PropertyDescriptor array_property;
    array_property.id = "nested_array";
    array_property.type = "astra.test.nested";
    array_property.kind = Astra::PropertySystem::TypeKind::Array;

    Astra::PropertySystem::PropertyDescriptor map_property;
    map_property.id = "nested_map";
    map_property.type = "astra.test.nested";
    map_property.kind = Astra::PropertySystem::TypeKind::Map;

    Astra::PropertySystem::PropertyDescriptor union_property;
    union_property.id = "choice";
    union_property.type = "astra.test.nested";
    union_property.kind = Astra::PropertySystem::TypeKind::TaggedUnion;

    Astra::PropertySystem::PropertyDescriptor editable;
    editable.id = "display_name";
    editable.type = "string";
    editable.flags = Astra::PropertySystem::PropertyFlag::AiEditable | Astra::PropertySystem::PropertyFlag::RequiresReview;

    Astra::PropertySystem::PropertyDescriptor guarded;
    guarded.id = "package_hash";
    guarded.type = "string";
    guarded.flags = Astra::PropertySystem::PropertyFlag::ReadOnly | Astra::PropertySystem::PropertyFlag::ReleaseSensitive;

    Astra::PropertySystem::PropertyDescriptor editor_only;
    editor_only.id = "editor_note";
    editor_only.type = "string";
    editor_only.flags = Astra::PropertySystem::PropertyFlag::EditorOnly;

    Astra::PropertySystem::PropertyDescriptor runtime_only;
    runtime_only.id = "runtime_counter";
    runtime_only.type = "integer";
    runtime_only.flags = Astra::PropertySystem::PropertyFlag::RuntimeOnly;

    Astra::PropertySystem::TypeDescriptor root;
    root.type_id = "astra.test.root";
    root.properties = {nested_property, array_property, map_property, union_property, editable, guarded, editor_only, runtime_only};
    REQUIRE(registry.Register(std::move(root)));
    REQUIRE(registry.RegisterMigration({"astra.test.root", 1, 2, {}}));

    Astra::Core::DiagnosticSink diagnostics;
    auto schema = registry.GenerateJsonSchema("astra.test.root", diagnostics);
    REQUIRE(schema["properties"]["nested"]["properties"].contains("value"));
    REQUIRE(schema["properties"]["nested_array"]["type"] == "array");
    REQUIRE(schema["properties"]["nested_array"]["items"]["properties"].contains("value"));
    REQUIRE(schema["properties"]["nested_map"]["additionalProperties"]["properties"].contains("value"));
    REQUIRE(schema["properties"]["choice"]["properties"].contains("value"));
    REQUIRE(registry.ValidateSchemaVersion("astra.test.root", 1, 2, diagnostics));

    auto allowed = registry.EvaluateWrite({"astra.test.root", "display_name", "Alice", "Bob", true, false, false, false});
    REQUIRE(allowed.allowed);
    REQUIRE(allowed.requires_review);
    auto ai_denied = registry.EvaluateWrite({"astra.test.root", "package_hash", "old", "new", true, false, false, false});
    REQUIRE_FALSE(ai_denied.allowed);
    auto editor_denied = registry.EvaluateWrite({"astra.test.root", "runtime_counter", 1, 2, false, true, false, false});
    REQUIRE_FALSE(editor_denied.allowed);
    auto runtime_denied = registry.EvaluateWrite({"astra.test.root", "editor_note", "old", "new", false, false, true, false});
    REQUIRE_FALSE(runtime_denied.allowed);

    auto denied = registry.EvaluateWrite({"astra.test.root", "package_hash", "old", "new", false, true, false, true});
    REQUIRE_FALSE(denied.allowed);
    REQUIRE(denied.release_sensitive);
    REQUIRE(Astra::PropertySystem::ToJson(denied)["diagnostics"].size() >= 1);

    auto diffs = registry.Diff("astra.test.root", {{"package_hash", "old"}}, {{"package_hash", "new"}});
    REQUIRE(diffs[0].release_sensitive);
    REQUIRE(Astra::PropertySystem::ToJson(diffs[0])["release_sensitive"] == true);
}

TEST_CASE("Headless platform provides filesystem timer thread and crash services") {
    auto platform = Astra::Platform::CreateHeadlessPlatform();
    REQUIRE(platform.Kind() == Astra::Platform::BackendKind::Headless);
    bool ran = false;
    platform.Thread().DispatchTagged("phase1", [&] { ran = true; });
    REQUIRE(platform.Thread().PendingTags()[0] == "phase1");
    platform.Thread().Drain();
    REQUIRE(ran);
    REQUIRE(platform.Thread().CompletedTags()[0] == "phase1");
    REQUIRE(platform.Thread().PendingTags().empty());
    REQUIRE(platform.Timer().MonotonicNanoseconds() > 0);
    REQUIRE(platform.FileSystem().Mount("project", std::filesystem::temp_directory_path(), false));
    REQUIRE(platform.FileSystem().Resolve("project", "demo.txt").filename() == "demo.txt");
    bool watch_triggered = false;
    const auto watch_root = std::filesystem::temp_directory_path() / "astra_phase1_watch";
    std::filesystem::create_directories(watch_root);
    platform.FileSystem().Watch(watch_root, [&](const std::filesystem::path&) { watch_triggered = true; });
    REQUIRE(platform.FileSystem().WriteText(watch_root / "changed.txt", "changed"));
    platform.FileSystem().PollWatches();
    REQUIRE(watch_triggered);
    auto missing_symbol = platform.DynamicLibrary().Symbol({}, "missing");
    REQUIRE_FALSE(missing_symbol);
    REQUIRE_FALSE(platform.Input().Snapshot().quit_requested);
    platform.Clipboard().SetText("phase1");
    REQUIRE(platform.Clipboard().GetText() == "phase1");
    platform.Cursor().SetVisible(false);
    REQUIRE_FALSE(platform.Cursor().IsVisible());
    REQUIRE(platform.Display().Displays().size() == 1);
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_TEST";
    diagnostic.category = "test";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Warning;
    diagnostic.message = "test";
    diagnostics.Emit(std::move(diagnostic));
    Astra::Platform::CrashCaptureContext crash_context;
    crash_context.build_info = "build";
    crash_context.frame_index = 42;
    crash_context.package_or_project_hash = "hash";
    crash_context.recent_logs = {"last log"};
    auto crash = platform.Crash().Capture(std::move(crash_context), diagnostics);
    REQUIRE(crash.diagnostics.size() == 1);
    REQUIRE(crash.frame_index == 42);
    REQUIRE_FALSE(crash.thread_id.empty());
    REQUIRE(crash.recent_logs[0] == "last log");
}

TEST_CASE("Public headers do not expose forbidden native ABI types") {
    const std::filesystem::path source_root = ASTRA_SOURCE_ROOT;
    const std::vector<std::filesystem::path> public_roots = {
        source_root / "Engine/Runtime/Core/Public",
        source_root / "Engine/Runtime/Platform/Public",
        source_root / "Engine/Runtime/ModuleRuntime/Public",
        source_root / "Engine/Runtime/PropertySystem/Public",
        source_root / "Engine/Runtime/Scene/Public",
        source_root / "Engine/Runtime/Runtime/Public",
        source_root / "Engine/Runtime/Asset/Public",
        source_root / "Engine/Runtime/Media/Public",
        source_root / "Engine/Runtime/Script/Public",
        source_root / "Engine/Runtime/AstraVN/Public",
    };
    const std::vector<std::string> forbidden = {"SDL_", "SDL.h", "HWND", "HINSTANCE", "Vk", "ID3D", "ALuint", "EditorWidget", "Actor*", "entt::", "entt/"};

    for (const auto& root : public_roots) {
        for (const auto& entry : std::filesystem::recursive_directory_iterator(root)) {
            if (!entry.is_regular_file()) {
                continue;
            }
            std::ifstream file(entry.path(), std::ios::binary);
            const std::string text(std::istreambuf_iterator<char>(file), {});
            for (const auto& token : forbidden) {
                INFO(entry.path().string() << " exposes " << token);
                REQUIRE(text.find(token) == std::string::npos);
            }
        }
    }
}

TEST_CASE("Asset foundation parses URIs resolves VFS and validates descriptors") {
    auto native = Astra::Asset::ParseAssetUri("native:/Characters\\\\Alice/Normal");
    REQUIRE(native);
    REQUIRE(native.Value().ToString() == "native:/Characters/Alice/Normal");
    REQUIRE(native.Value().ToStableId().Kind() == Astra::Core::StableIdKind::Asset);

    auto package = Astra::Core::ParseStableId("package:/Characters/Alice/Normal");
    REQUIRE(package);
    REQUIRE(package.Value().Kind() == Astra::Core::StableIdKind::Asset);

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Asset::Vfs vfs;
    REQUIRE(vfs.Mount({"low", "native", "low", Astra::Asset::VfsAccess::ReadOnly, 1}, diagnostics));
    REQUIRE(vfs.Mount({"high", "native", "high", Astra::Asset::VfsAccess::ReadWrite, 100}, diagnostics));
    auto resolved = vfs.Resolve(native.Value());
    REQUIRE(resolved);
    REQUIRE(resolved->mount.mount_id == "high");
    REQUIRE_FALSE(resolved->read_only);

    Astra::Asset::ImportPresetDescriptor preset;
    preset.id = "astra.import.sprite.character";
    preset.source_extensions = {".png", ".webp"};
    preset.asset_type = "image";
    REQUIRE(Astra::Asset::ValidateImportPreset(preset, diagnostics));

    Astra::Asset::ProjectTemplateDescriptor project_template;
    project_template.id = "astra.template.vn.standard";
    project_template.display_name = "Standard AstraVN";
    project_template.runtime_profile = "astra.vn.runtime";
    REQUIRE(Astra::Asset::ValidateProjectTemplate(project_template, diagnostics));

    REQUIRE(Astra::Asset::ValidateReviewQueueItem({"review:/asset/one", "asset_import", "pending", "Saved/Agent/Drafts/one.png", "native:/Characters/Alice/Normal"}, diagnostics));
}

TEST_CASE("Asset registry scans sidecars and reports broken dependencies") {
    const auto root = std::filesystem::temp_directory_path() / "astra_phase3_assets";
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root / "Characters");
    {
        std::ofstream image(root / "Characters/alice.png", std::ios::binary);
        image << "fake image bytes";
    }
    {
        std::ofstream sidecar(root / "Characters/alice.asset.yaml", std::ios::binary);
        sidecar << "id: native:/Characters/Alice/Normal\n";
        sidecar << "type: image\n";
        sidecar << "source_path: alice.png\n";
        sidecar << "display_name: Alice Normal\n";
        sidecar << "origin: HumanAuthored\n";
        sidecar << "license:\n  owner: project\n  usage: internal\n";
        sidecar << "review:\n  status: accepted\n";
        sidecar << "cook:\n  preset: sprite\n";
        sidecar << "dependencies:\n  hard: [native:/Backgrounds/Missing]\n";
    }

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Asset::AssetRegistryBuilder builder;
    auto registry = builder.Scan(root, diagnostics);
    REQUIRE(registry.entries.size() == 1);
    REQUIRE(registry.entries[0].id.ToString() == "native:/Characters/Alice/Normal");
    REQUIRE_FALSE(registry.entries[0].source_hash.empty());
    REQUIRE(diagnostics.HasBlocking());
    REQUIRE(Astra::Asset::ToJson(registry)["entries"][0]["dependencies"]["hard"][0] == "native:/Backgrounds/Missing");

    auto platform = Astra::Platform::CreateHeadlessPlatform();
    auto invalidations = builder.WatchContent(platform.FileSystem(), root);
    REQUIRE(invalidations.empty());
}

TEST_CASE("Media foundation extracts presentation commands and produces stable headless hashes") {
    Astra::Core::DiagnosticSink diagnostics;
    auto background = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room");
    auto character = Astra::Asset::ParseAssetUri("native:/Characters/Alice/Normal");
    auto voice = Astra::Asset::ParseAssetUri("native:/Voice/Alice/opening_001");
    auto filter_id = Astra::Asset::ParseAssetUri("native:/Filters/soft_vn");
    REQUIRE(background);
    REQUIRE(character);
    REQUIRE(voice);
    REQUIRE(filter_id);

    Astra::Media::FilterProfile profile;
    profile.id = filter_id.Value();
    profile.passes.push_back({"bg_blur", "astra.filter.gaussian_blur", Astra::Media::FilterTarget::Background, {{"radius", 2}}});
    profile.passes.push_back({"character_line", "astra.filter.line_enhance", Astra::Media::FilterTarget::Character, {{"strength", 0.4}}});
    profile.passes.push_back({"final_grade", "astra.filter.color_grade", Astra::Media::FilterTarget::Final, {{"preset", "warm"}}});
    REQUIRE(Astra::Media::ValidateFilterProfile(profile, diagnostics));

    std::vector<Astra::Media::PresentationCommand> commands;
    Astra::Media::PresentationCommand background_draw;
    background_draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    background_draw.command_id = "draw.background";
    background_draw.frame_index = 2;
    background_draw.layer = "background";
    background_draw.asset = background.Value();
    commands.push_back(std::move(background_draw));

    Astra::Media::PresentationCommand character_draw;
    character_draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    character_draw.command_id = "draw.character";
    character_draw.frame_index = 2;
    character_draw.layer = "character";
    character_draw.order = 100;
    character_draw.asset = character.Value();
    commands.push_back(std::move(character_draw));

    Astra::Media::PresentationCommand text;
    text.kind = Astra::Media::PresentationCommandKind::Text;
    text.command_id = "text.dialogue";
    text.frame_index = 2;
    text.layer = "text";
    text.order = 200;
    text.text = "Hello from Phase 3.";
    text.locale = "en-US";
    commands.push_back(std::move(text));

    Astra::Media::PresentationCommand audio;
    audio.kind = Astra::Media::PresentationCommandKind::Audio;
    audio.command_id = "audio.voice";
    audio.frame_index = 2;
    audio.asset = voice.Value();
    audio.bus = "voice";
    audio.payload = {{"kind", "play"}, {"volume", 0.75}};
    commands.push_back(std::move(audio));

    auto graph = Astra::Media::ExtractRenderGraph(commands, &profile, diagnostics);
    REQUIRE(graph.draws.size() == 2);
    REQUIRE(graph.text_requests.size() == 1);
    REQUIRE(graph.audio_commands.size() == 1);
    REQUIRE(graph.filter_applications.size() == 3);

    auto renderer = Astra::Media::CreateHeadlessRenderer2D();
    renderer->Submit(graph, diagnostics);
    const auto first = renderer->Capture();
    renderer->Submit(graph, diagnostics);
    const auto second = renderer->Capture();
    REQUIRE(first.render_hash == second.render_hash);
    REQUIRE(first.text_hash == second.text_hash);
    REQUIRE(first.audio_hash == second.audio_hash);
    REQUIRE(first.filter_hash == second.filter_hash);
    REQUIRE_FALSE(first.filter_hash.empty());
    REQUIRE(Astra::Media::ToJson(first)["commands"]["filter_applications"].size() == 3);

    const auto png_1x1_rgba = TestPng1x1Rgba();
    auto image = Astra::Media::InspectImageBytes(png_1x1_rgba, diagnostics);
    REQUIRE(image);
    REQUIRE(image.Value().format == "png");
    REQUIRE(image.Value().width == 1);
    REQUIRE(image.Value().height == 1);
    REQUIRE(image.Value().has_alpha);
    REQUIRE(Astra::Media::ToJson(image.Value())["decoded_by"] == "libpng");
}

TEST_CASE("Media provider descriptors and release gate validate foundation slots") {
    const auto capabilities = Astra::Media::ProbeMediaBackendCapabilities();
    const auto capabilities_json = Astra::Media::ToJson(capabilities);
    REQUIRE(capabilities_json["schema"] == "astra.media.backend_capabilities.v1");
    REQUIRE(capabilities_json["libraries"].size() >= 6);
    REQUIRE(capabilities_json["libraries"][0]["id"] == "sdl3");

    auto providers = Astra::Media::FoundationMediaProviders();
    REQUIRE(providers.size() == 3);

    Astra::Core::DiagnosticSink diagnostics;
    for (const auto& provider : providers) {
        REQUIRE(Astra::Media::ValidateMediaProviderDescriptor(provider, diagnostics));
    }
    REQUIRE_FALSE(diagnostics.HasBlocking());

    auto filter_id = Astra::Asset::ParseAssetUri("native:/Filters/release_gate");
    REQUIRE(filter_id);
    Astra::Media::FilterProfile profile;
    profile.id = filter_id.Value();
    profile.passes.push_back({"bg_soften", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Background, {{"strength", 0.25}}});
    profile.passes.push_back({"final_grade", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Final, {{"grade", "warm"}}});

    Astra::Media::MediaReleaseGateRequest request;
    request.providers = providers;
    request.filter_profile = profile;
    for (const auto& provider : providers) {
        request.selected_providers[provider.slot_id] = provider.provider_id;
    }
    auto report = Astra::Media::ValidateMediaReleaseGate(request, diagnostics);
    REQUIRE(report);
    REQUIRE(report.Value().selected_providers.size() == 3);
    REQUIRE(report.Value().filter_applications.size() == 2);
    REQUIRE(Astra::Media::ToJson(report.Value())["schema"] == Astra::Media::MediaReleaseGateSchema);

    std::vector<Astra::Media::PresentationCommand> commands;
    Astra::Media::PresentationCommand draw;
    draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    draw.command_id = "draw.release_gate";
    draw.frame_index = 3;
    draw.layer = "background";
    draw.asset = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room").Value();
    commands.push_back(std::move(draw));
    auto graph = Astra::Media::ExtractRenderGraph(commands, &profile, diagnostics);
    auto renderer = Astra::Media::CreateHeadlessRenderer2D();
    renderer->Submit(graph, diagnostics);
    auto capture = renderer->Capture();
    REQUIRE_FALSE(capture.render_hash.empty());
    REQUIRE_FALSE(capture.filter_hash.empty());

    auto missing_slot = request;
    missing_slot.selected_providers.erase(Astra::Media::AudioSlotId);
    Astra::Core::DiagnosticSink missing_slot_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(missing_slot, missing_slot_diagnostics));
    REQUIRE(missing_slot_diagnostics.HasBlocking());

    auto slot_mismatch = request;
    slot_mismatch.selected_providers[Astra::Media::Renderer2DSlotId] = providers[2].provider_id;
    Astra::Core::DiagnosticSink mismatch_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(slot_mismatch, mismatch_diagnostics));
    REQUIRE(mismatch_diagnostics.HasBlocking());

    auto not_packaged = request;
    not_packaged.providers[0].packaged_eligible = false;
    Astra::Core::DiagnosticSink packaged_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(not_packaged, packaged_diagnostics));
    REQUIRE(packaged_diagnostics.HasBlocking());

    auto missing_prefix = request;
    missing_prefix.providers[1].diagnostics_code_prefix.clear();
    Astra::Core::DiagnosticSink prefix_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(missing_prefix, prefix_diagnostics));
    REQUIRE(prefix_diagnostics.HasBlocking());

    auto no_headless = request;
    no_headless.providers[2].headless_supported = false;
    Astra::Core::DiagnosticSink headless_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(no_headless, headless_diagnostics));
    REQUIRE(headless_diagnostics.HasBlocking());
}

TEST_CASE("Scene world spawns actors snapshots components and rejects stale handles") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;

    auto actor_id = Astra::Core::ParseStableId("actor:/characters/alice");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.character");
    auto component_id = Astra::Core::ParseStableId("component:/astra.dialogue");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(component_id);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Alice";
    descriptor.components.push_back({component_id.Value(), "astra.dialogue", 1, {{"line", "hello"}}});

    auto handle = world.Spawn(descriptor, diagnostics);
    REQUIRE(handle);
    REQUIRE(world.Activate(handle.Value(), diagnostics));
    REQUIRE(world.ActorCount() == 1);
    REQUIRE(Astra::Scene::FoundationComponentDescriptors().size() == 6);
    REQUIRE(world.ResolveActor(actor_id.Value(), diagnostics));
    auto component = world.FindComponent(handle.Value(), "astra.dialogue");
    REQUIRE(component);
    REQUIRE(component->data["line"] == "hello");
    REQUIRE(world.FindComponent(handle.Value(), "astra.blackboard"));
    REQUIRE(world.FindComponent(handle.Value(), "astra.control_policy"));

    auto snapshot = world.Snapshot();
    REQUIRE(snapshot.actors.size() == 1);
    REQUIRE(Astra::Scene::ToJson(snapshot).dump().find("entt") == std::string::npos);

    REQUIRE(world.Destroy(handle.Value(), diagnostics));
    REQUIRE_FALSE(world.IsHandleAlive(handle.Value()));
    REQUIRE_FALSE(world.Destroy(handle.Value(), diagnostics));
    REQUIRE_FALSE(world.ResolveActor(actor_id.Value(), diagnostics));
}

TEST_CASE("Scene headless local ECS pack syncs snapshots without exposing entities") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;
    auto actor_id = Astra::Core::ParseStableId("actor:/ecs/one");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.test.actor");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    auto handle = world.Spawn({actor_id.Value(), actor_type.Value(), "ECS One", {}}, diagnostics);
    REQUIRE(handle);

    auto pack = Astra::Scene::CreateHeadlessLocalEcsPack();
    pack->SyncIn({world.Snapshot().actors});
    pack->Update(1.0 / 60.0);
    auto output = pack->SyncOut();
    REQUIRE(output.actors.size() == 1);
    REQUIRE(output.emitted_events.size() == 1);
    Astra::Scene::WorldSnapshot output_snapshot;
    output_snapshot.actors = output.actors;
    REQUIRE(Astra::Scene::ToJson(output_snapshot).dump().find("entity") == std::string::npos);
}

TEST_CASE("Runtime world orders events advances state machine and saves loads") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Runtime::RuntimeWorld runtime(1234);

    auto actor_id = Astra::Core::ParseStableId("actor:/systems/dialogue");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.dialogue_system");
    auto state_machine_id = Astra::Core::ParseStableId("state_machine:/dialogue_box");
    auto event_type = Astra::Core::ParseStableId("event:/astra.vn.dialogue.say_requested");
    auto component_id = Astra::Core::ParseStableId("component:/astra.state_machine");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(state_machine_id);
    REQUIRE(event_type);
    REQUIRE(component_id);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Dialogue System";
    descriptor.components.push_back({
        component_id.Value(),
        "astra.state_machine",
        1,
        {{"state_machine_id", state_machine_id.Value().ToString()}, {"current_state", "idle"}},
    });
    auto actor = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(actor);
    REQUIRE(runtime.Scene().Activate(actor.Value(), diagnostics));

    Astra::Runtime::StateMachineDefinition definition;
    definition.id = state_machine_id.Value();
    definition.initial_state = "idle";
    definition.transitions.push_back({"idle", event_type.Value(), "typing"});
    runtime.RegisterStateMachine(std::move(definition));

    Astra::Runtime::RuntimeEvent event;
    event.type = event_type.Value();
    event.category = "story";
    event.source = {"script", "native:/Scripts/opening"};
    event.target = {"actor", actor_id.Value().ToString()};
    event.payload_schema = "astra.vn.say_requested.v1";
    event.payload = {{"speaker", "actor:/characters/alice"}};
    REQUIRE(runtime.Emit(event, Astra::Runtime::RuntimeEventMode::Queued, diagnostics));
    REQUIRE(runtime.Tick(diagnostics));

    auto state_machine = runtime.Scene().FindComponent(actor.Value(), "astra.state_machine");
    REQUIRE(state_machine);
    REQUIRE(state_machine->data["current_state"] == "typing");
    REQUIRE(runtime.Hashes().state_hash == runtime.CaptureSnapshot().hashes.state_hash);
    auto replay = runtime.CaptureReplay();
    REQUIRE(replay.schema == Astra::Runtime::ReplaySchema);
    REQUIRE(replay.events.size() == 1);

    auto save = runtime.Save();
    REQUIRE(save.schema == Astra::Runtime::SnapshotSchema);
    const auto save_json = Astra::Core::ToJson(save).dump();
    REQUIRE(save_json.find("entt") == std::string::npos);
    REQUIRE(save_json.find("entity") == std::string::npos);
    REQUIRE(save_json.find("Actor*") == std::string::npos);

    Astra::Runtime::RuntimeWorld loaded;
    REQUIRE(loaded.Load(save, diagnostics));
    REQUIRE(loaded.CaptureSnapshot().frame_index == 1);
    REQUIRE(loaded.Scene().ActorCount() == 1);
    REQUIRE(loaded.Hashes().state_hash == runtime.Hashes().state_hash);
}

TEST_CASE("Runtime control policy allows queues and rejects locked channels") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Runtime::RuntimeWorld runtime;
    auto actor_id = Astra::Core::ParseStableId("actor:/characters/alice");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.character");
    auto policy_component = Astra::Core::ParseStableId("component:/astra.control_policy");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(policy_component);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Alice";
    descriptor.components.push_back({
        policy_component.Value(),
        "astra.control_policy",
        1,
        {{"owner", "story_script"}, {"locked_channels", {"pose"}}, {"queue_locked", true}},
    });
    auto handle = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(handle);

    auto owner = runtime.EvaluateControlPolicy({actor_id.Value(), "pose", "story_script", 80}, diagnostics);
    REQUIRE(owner.decision == Astra::Runtime::ControlDecision::Allow);
    auto queued = runtime.EvaluateControlPolicy({actor_id.Value(), "pose", "runtime_ai", 40}, diagnostics);
    REQUIRE(queued.decision == Astra::Runtime::ControlDecision::Queue);

    auto bob_id = Astra::Core::ParseStableId("actor:/characters/bob");
    REQUIRE(bob_id);
    descriptor.id = bob_id.Value();
    descriptor.name = "Bob";
    descriptor.components[0].data = {{"owner", "story_script"}, {"locked_channels", {"pose"}}, {"queue_locked", false}};
    auto reject_handle = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(reject_handle);
    auto rejected = runtime.EvaluateControlPolicy({descriptor.id, "pose", "runtime_ai", 40}, diagnostics);
    REQUIRE(rejected.decision == Astra::Runtime::ControlDecision::Reject);
    auto system = runtime.EvaluateControlPolicy({descriptor.id, "pose", "system", 100}, diagnostics);
    REQUIRE(system.decision == Astra::Runtime::ControlDecision::Allow);
}

TEST_CASE("Runtime replay foundation produces stable hashes for same seed and input") {
    auto run_once = [] {
        Astra::Core::DiagnosticSink diagnostics;
        Astra::Runtime::RuntimeWorld runtime(7);
        auto event_type = Astra::Core::ParseStableId("event:/astra.test.ping");
        REQUIRE(event_type);
        Astra::Runtime::RuntimeEvent event;
        event.type = event_type.Value();
        event.payload = {{"value", 42}};
        REQUIRE(runtime.Emit(event, Astra::Runtime::RuntimeEventMode::Deferred, diagnostics));
        REQUIRE(runtime.Tick(diagnostics));
        return runtime.Hashes();
    };

    const auto first = run_once();
    const auto second = run_once();
    REQUIRE(first.state_hash == second.state_hash);
    REQUIRE(first.event_hash == second.event_hash);
    REQUIRE(first.presentation_hash == second.presentation_hash);
}

TEST_CASE("Script foundation compiles Native DSL and reports actionable diagnostics") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/opening";
    source.file = "opening.astra";
    source.text = R"(label opening
bg native:/Backgrounds/Room
show alice native:/Characters/Alice/Normal center
say alice "Good morning." voice native:/Voice/Alice/opening_001
choice "Walk together" -> route_walk
label route_walk
set route walk
)";

    auto compiled = host.CompileNative(source, diagnostics);
    REQUIRE(compiled);
    REQUIRE(compiled.Value().labels.contains("opening"));
    REQUIRE(compiled.Value().labels.contains("route_walk"));
    REQUIRE_FALSE(compiled.Value().debug_symbols.empty());

    Astra::Core::DiagnosticSink bad_diagnostics;
    source.text = "label opening\njump missing_route\n";
    auto bad = host.CompileNative(source, bad_diagnostics);
    REQUIRE_FALSE(bad);
    REQUIRE(bad_diagnostics.HasBlocking());
    REQUIRE(bad_diagnostics.Diagnostics()[0].source.line > 0);
    REQUIRE_FALSE(bad_diagnostics.Diagnostics()[0].suggested_fixes.empty());
}

TEST_CASE("Script foundation Native DSL and Lua produce equivalent VN presentation") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptExecutionOptions options;
    options.entry_label = "opening";
    options.choice_index = 0;

    Astra::Script::ScriptSource native_source;
    native_source.runtime_id = Astra::Script::NativeRuntimeId;
    native_source.source_id = "native:/Scripts/opening";
    native_source.file = "opening.astra";
    native_source.text = R"(label opening
bg native:/Backgrounds/Room
show alice native:/Characters/Alice/Normal center
say alice "Good morning from Phase 4." voice native:/Voice/Alice/opening_001
audio music native:/Music/opening_theme
filter native:/Filters/soft_vn
choice "Walk together" -> route_walk
label route_walk
set route walk
say alice "Let's go."
)";

    Astra::Script::ScriptSource lua_source;
    lua_source.runtime_id = Astra::Script::LuaRuntimeId;
    lua_source.source_id = "native:/Scripts/opening";
    lua_source.file = "opening.lua";
    lua_source.text = R"(astra.label("opening")
astra.bg("native:/Backgrounds/Room")
astra.show("alice", "native:/Characters/Alice/Normal", "center")
astra.say("alice", "Good morning from Phase 4.", "native:/Voice/Alice/opening_001")
astra.audio("music", "native:/Music/opening_theme")
astra.filter("native:/Filters/soft_vn")
astra.choice("Walk together", "route_walk")
astra.label("route_walk")
astra.set("route", "walk")
astra.say("alice", "Let's go.")
)";

    Astra::AstraVN::VnSession native_session(44);
    auto native_result = native_session.RunNative(native_source, options, diagnostics);
    REQUIRE(native_result);
    auto native_snapshot = native_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession lua_session(44);
    auto lua_result = lua_session.RunLua(lua_source, options, diagnostics);
    REQUIRE(lua_result);
    auto lua_snapshot = lua_session.CaptureSnapshot(diagnostics);

    REQUIRE(native_snapshot.headless_capture.render_hash == lua_snapshot.headless_capture.render_hash);
    REQUIRE(native_snapshot.headless_capture.text_hash == lua_snapshot.headless_capture.text_hash);
    REQUIRE(native_snapshot.headless_capture.audio_hash == lua_snapshot.headless_capture.audio_hash);
    REQUIRE(native_snapshot.headless_capture.filter_hash == lua_snapshot.headless_capture.filter_hash);
    REQUIRE(native_snapshot.script_snapshot.variables["route"] == "walk");
    REQUIRE(native_snapshot.vn_events.size() >= 6);
    REQUIRE(native_snapshot.dialogue_history.size() == 2);

    Astra::AstraVN::VnSession restored(44);
    REQUIRE(restored.Restore(native_snapshot, diagnostics));
    REQUIRE(restored.Runtime().Hashes().state_hash == native_snapshot.hashes.state_hash);
    REQUIRE(restored.Runtime().Hashes().event_hash == native_snapshot.hashes.event_hash);
}

TEST_CASE("AstraVN foundation presets drive actor state machines without native handles") {
    Astra::Core::DiagnosticSink diagnostics;
    auto profile = Astra::AstraVN::FoundationProfile();
    REQUIRE(profile.event_schemas.size() >= 8);
    REQUIRE(profile.actor_presets.size() == 8);
    REQUIRE(profile.component_presets.size() >= 7);
    REQUIRE(profile.state_machines.size() >= 6);

    Astra::AstraVN::VnSession session(12);
    REQUIRE(session.Runtime().Scene().ActorCount() == profile.actor_presets.size());

    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/opening";
    source.file = "opening.astra";
    source.text = R"(label opening
say alice "Testing."
choice "Continue" -> done
label done
)";
    auto result = session.RunNative(source, {"opening", 0}, diagnostics);
    REQUIRE(result);

    auto dialogue_id = Astra::Core::ParseStableId("actor:/systems/dialogue");
    REQUIRE(dialogue_id);
    auto handle = session.Runtime().Scene().ResolveActor(dialogue_id.Value(), diagnostics);
    REQUIRE(handle);
    auto state = session.Runtime().Scene().FindComponent(handle.value(), "astra.state_machine");
    REQUIRE(state);
    REQUIRE(state->data["current_state"] == "waiting_choice");
}

TEST_CASE("Scene foundation stress spawns snapshots and destroys one thousand actors") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;
    std::vector<Astra::Scene::ActorHandle> handles;
    auto actor_type = Astra::Core::ParseStableId("type:/astra.test.actor");
    REQUIRE(actor_type);

    for (int index = 0; index < 1000; ++index) {
        auto actor_id = Astra::Core::ParseStableId("actor:/stress/" + std::to_string(index));
        REQUIRE(actor_id);
        Astra::Scene::ActorDescriptor descriptor;
        descriptor.id = actor_id.Value();
        descriptor.type_id = actor_type.Value();
        descriptor.name = "Stress";
        auto handle = world.Spawn(descriptor, diagnostics);
        REQUIRE(handle);
        handles.push_back(handle.Value());
    }
    REQUIRE(world.ActorCount() == 1000);
    REQUIRE(world.Snapshot().actors.size() == 1000);
    for (const auto& handle : handles) {
        REQUIRE(world.Destroy(handle, diagnostics));
    }
    REQUIRE(world.ActorCount() == 0);
}

TEST_CASE("Module descriptor validation and dependency resolver emit blocking diagnostics") {
    Astra::ModuleRuntime::ModuleDescriptor module;
    module.id = "a";
    module.entrypoint = "Bin/a.dll";
    module.required_dependencies = {"missing"};

    Astra::ModuleRuntime::PluginDescriptor descriptor;
    descriptor.id = "astra.plugin.test";
    descriptor.version = "0.1.0";
    descriptor.astra_api = ">=0.1 <0.2";
    descriptor.modules = {module};
    Astra::Core::DiagnosticSink diagnostics;
    auto order = Astra::ModuleRuntime::ResolveModuleOrder(descriptor, diagnostics);
    REQUIRE_FALSE(order);
    REQUIRE(diagnostics.HasBlocking());
}

TEST_CASE("Service extension and provider registries reject duplicates") {
    Astra::ModuleRuntime::ServiceRegistry services;
    REQUIRE(services.Register({"service", "module", "capability", "v1", "engine", {"project_read"}}));
    Astra::ModuleRuntime::RegisteredService duplicate_service;
    duplicate_service.service_id = "service";
    duplicate_service.provider_module = "module2";
    duplicate_service.capability = "capability";
    REQUIRE_FALSE(services.Register(std::move(duplicate_service)));
    Astra::Core::DiagnosticSink diagnostics;
    auto denied = services.Resolve({"consumer", "service", "v1", {"capability"}, {}, Astra::ModuleRuntime::ModuleState::Active}, diagnostics);
    REQUIRE_FALSE(denied);
    REQUIRE(diagnostics.HasBlocking());
    diagnostics.Clear();
    auto allowed = services.Resolve({"consumer", "service", "v1", {"capability"}, {"project_read"}, Astra::ModuleRuntime::ModuleState::Active}, diagnostics);
    REQUIRE(allowed);
    REQUIRE(Astra::ModuleRuntime::ToJson(allowed.Value())["allowed"] == true);

    Astra::ModuleRuntime::ExtensionRegistry extensions;
    REQUIRE(extensions.Register({"extension", "module", "Kind"}));
    REQUIRE_FALSE(extensions.Register({"extension", "module", "Kind"}));

    Astra::ModuleRuntime::EngineModuleRegistry providers;
    REQUIRE(providers.RegisterSlot({"slot", "provider"}));
    REQUIRE(providers.RegisterProvider({"slot", "provider", "module"}));
    REQUIRE_FALSE(providers.RegisterProvider({"slot", "provider", "module"}));
    REQUIRE(providers.ValidatePolicy({{{"slot", "provider"}}}, diagnostics));
    REQUIRE_FALSE(providers.ValidatePolicy({{{"other", "provider"}}}, diagnostics));
}

TEST_CASE("Module release gate validates descriptor policy and binary evidence") {
    Astra::ModuleRuntime::PluginDescriptor descriptor;
    descriptor.id = "astra.plugin.release_gate";
    descriptor.version = "0.1.0";
    descriptor.astra_api = ">=0.1 <0.2";
    descriptor.packaged_eligible = true;
    descriptor.diagnostics_code_prefix = "ASTRA_PLUGIN_RELEASE";
    Astra::ModuleRuntime::ModuleDescriptor module;
    module.id = "release.runtime";
    module.type = "runtime";
    module.load_phase = "runtime_startup";
    module.entrypoint = "Bin/missing.dll";
    module.packaged = true;
    module.permissions = {"runtime.packaged"};
    module.capabilities = {"service_provider"};
    descriptor.modules = {module};

    Astra::Core::DiagnosticSink diagnostics;
    auto report = Astra::ModuleRuntime::ValidateModuleReleaseGate(descriptor, std::filesystem::temp_directory_path(), diagnostics);
    REQUIRE_FALSE(report);
    REQUIRE(diagnostics.HasBlocking());
}

TEST_CASE("Module manager reports ABI failures for real invalid binaries") {
#ifndef ASTRA_PHASE1_INVALID_PLUGIN_ROOT
    SKIP("Invalid ABI fixture plugins are not part of this build.");
#else
    auto platform = Astra::Platform::CreateHeadlessPlatform();

    Astra::ModuleRuntime::PluginDescriptor no_entry;
    no_entry.id = "astra.plugin.invalid.no_entry";
    no_entry.version = "0.1.0";
    no_entry.astra_api = ">=0.1 <0.2";
    no_entry.diagnostics_code_prefix = "ASTRA_INVALID_NO_ENTRY";
    Astra::ModuleRuntime::ModuleDescriptor no_entry_module;
    no_entry_module.id = "invalid.no_entry";
    no_entry_module.type = "runtime";
    no_entry_module.load_phase = "runtime_startup";
    no_entry_module.entrypoint = std::string("Bin/win64/") + ASTRA_PHASE1_INVALID_NO_ENTRY;
    no_entry.modules.push_back(std::move(no_entry_module));
    Astra::Core::DiagnosticSink no_entry_diagnostics;
    Astra::ModuleRuntime::ModuleManager no_entry_manager(platform);
    REQUIRE_FALSE(no_entry_manager.LoadAndActivate(no_entry, ASTRA_PHASE1_INVALID_PLUGIN_ROOT, no_entry_diagnostics));
    REQUIRE(no_entry_diagnostics.HasBlocking());

    Astra::ModuleRuntime::PluginDescriptor bad_abi;
    bad_abi.id = "astra.plugin.invalid.bad_abi";
    bad_abi.version = "0.1.0";
    bad_abi.astra_api = ">=0.1 <0.2";
    bad_abi.diagnostics_code_prefix = "ASTRA_INVALID_BAD_ABI";
    Astra::ModuleRuntime::ModuleDescriptor bad_abi_module;
    bad_abi_module.id = "invalid.bad_abi";
    bad_abi_module.type = "runtime";
    bad_abi_module.load_phase = "runtime_startup";
    bad_abi_module.entrypoint = std::string("Bin/win64/") + ASTRA_PHASE1_INVALID_BAD_ABI;
    bad_abi.modules.push_back(std::move(bad_abi_module));
    Astra::Core::DiagnosticSink bad_abi_diagnostics;
    Astra::ModuleRuntime::ModuleManager bad_abi_manager(platform);
    REQUIRE_FALSE(bad_abi_manager.LoadAndActivate(bad_abi, ASTRA_PHASE1_INVALID_PLUGIN_ROOT, bad_abi_diagnostics));
    REQUIRE(bad_abi_diagnostics.HasBlocking());
#endif
}

TEST_CASE("Example foundation plugin loads registers and unloads through module manager") {
#ifndef ASTRA_PHASE1_PLUGIN_DESCRIPTOR
    SKIP("Example foundation plugin is not part of this build.");
#else
    const std::filesystem::path descriptor_path = ASTRA_PHASE1_PLUGIN_DESCRIPTOR;
    REQUIRE(std::filesystem::exists(descriptor_path));

    Astra::Core::DiagnosticSink diagnostics;
    auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(descriptor_path, diagnostics);
    REQUIRE(descriptor);

    auto platform = Astra::Platform::CreateHeadlessPlatform();
    Astra::ModuleRuntime::ModuleManager manager(platform);
    auto loaded = manager.LoadAndActivate(descriptor.Value(), descriptor_path.parent_path(), diagnostics);
    REQUIRE(loaded);
    REQUIRE(manager.State("phase1.example.runtime") == Astra::ModuleRuntime::ModuleState::Active);
    REQUIRE(manager.Services().Find("astra.phase1.example.service") != nullptr);
    REQUIRE(manager.Extensions().Extensions().size() == 1);
    REQUIRE(manager.Extensions().Extensions()[0].kind == "AssetImporter");
    REQUIRE(manager.EngineModules().Providers().size() == 1);
    REQUIRE(manager.EngineModules().Providers()[0].slot_id == "astra.renderer2d");

    manager.DeactivateAndUnload(diagnostics);
    REQUIRE(manager.State("phase1.example.runtime") == Astra::ModuleRuntime::ModuleState::Unloaded);
#endif
}

#if defined(ASTRA_WITH_TOOLS)
TEST_CASE("Tools reports validate inspect package and hash foundation artifacts") {
    Astra::Tools::CommandOptions options;
    options.strict = true;

    auto validation = Astra::Tools::Validate(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/PackageSmoke", options);
    REQUIRE(validation.Passed());
    REQUIRE(Astra::Tools::ToJson(validation)["command"] == "astra validate");
    REQUIRE(validation.artifacts["foundation_core_gate"]["passed"] == true);
    REQUIRE(validation.artifacts["engine_binaries"].is_array());
    REQUIRE(validation.artifacts["phase3_media_backend_capabilities"]["schema"] == "astra.media.backend_capabilities.v1");
    REQUIRE(validation.artifacts["phase3_media_release_gate"]["schema"] == Astra::Media::MediaReleaseGateSchema);
    REQUIRE(validation.artifacts["phase3_media_release_gate"]["selected_providers"].size() == 3);

    auto native_validation = Astra::Tools::Validate(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", options);
    REQUIRE(native_validation.Passed());
    REQUIRE(native_validation.artifacts["asset_registry"]["entries"].size() >= 7);
    REQUIRE(native_validation.artifacts["asset_dependency_graph"]["hard_edges"].contains("native:/Scripts/opening"));
    REQUIRE(native_validation.artifacts["phase3_media_release_gate"]["passed"] == true);
    REQUIRE(native_validation.artifacts["phase4_script_vn"]["status"] == "passed");
    REQUIRE(native_validation.artifacts["phase4_script_vn"]["parity"]["presentation_hashes_match"] == true);

    auto native_cook = Astra::Tools::Cook(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", options);
    REQUIRE(native_cook.Passed());
    REQUIRE(std::filesystem::exists(native_cook.artifacts["ddc_manifest"].get<std::string>()));
    REQUIRE(native_cook.artifacts["cook_manifest"]["ddc_entries"].size() >= 7);
    REQUIRE(native_cook.artifacts["ddc_status"]["rebuilt"].get<Astra::Core::u64>() + native_cook.artifacts["ddc_status"]["reused"].get<Astra::Core::u64>() >= 7);

    const auto ddc_root = std::filesystem::path(native_cook.artifacts["ddc_root"].get<std::string>());
    const auto first_cache_artifact = ddc_root / native_cook.artifacts["cook_manifest"]["ddc_entries"][0]["artifacts"][0]["path"].get<std::string>();
    REQUIRE(std::filesystem::exists(first_cache_artifact));
    {
        std::ofstream file(first_cache_artifact, std::ios::binary);
        file << "corrupted cache payload";
    }
    auto recovered_cook = Astra::Tools::Cook(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", options);
    REQUIRE(recovered_cook.Passed());
    REQUIRE(recovered_cook.artifacts["ddc_status"]["corruption_recovered"].get<Astra::Core::u64>() >= 1);

    const auto media_sample = std::filesystem::temp_directory_path() / "astra_media_cook_sample";
    std::filesystem::remove_all(media_sample);
    std::filesystem::create_directories(media_sample / "Content/Backgrounds");
    {
        std::ofstream descriptor(media_sample / "astra.sample.yaml", std::ios::binary);
        descriptor << "schema: astra.sample.v1\n";
        descriptor << "id: Samples/MediaCookSmoke\n";
        descriptor << "phase: 3\n";
        descriptor << "foundation_only: true\n";
    }
    {
        const auto png = TestPng1x1Rgba();
        std::ofstream image(media_sample / "Content/Backgrounds/Room.png", std::ios::binary);
        image.write(reinterpret_cast<const char*>(png.data()), static_cast<std::streamsize>(png.size()));
    }
    {
        std::ofstream sidecar(media_sample / "Content/Backgrounds/Room.asset.yaml", std::ios::binary);
        sidecar << "id: native:/Backgrounds/Room\n";
        sidecar << "schema: astra.asset.sidecar.v1\n";
        sidecar << "type: image\n";
        sidecar << "source_path: Room.png\n";
        sidecar << "display_name: Room\n";
        sidecar << "origin: HumanAuthored\n";
        sidecar << "license:\n  owner: project\n  usage: internal\n";
        sidecar << "review:\n  status: accepted\n";
        sidecar << "cook:\n  preset: background\n";
        sidecar << "dependencies:\n  hard: []\n  soft: []\n";
    }
    auto media_cook = Astra::Tools::Cook(media_sample, options);
    REQUIRE(media_cook.Passed());
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"].size() == 1);
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["media_inspect"]["status"] == "decoded");
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["media_inspect"]["decoded_by"] == "libpng");
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["media_inspect"]["width"] == 1);

    auto package = Astra::Tools::Package(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/PackageSmoke", options);
    REQUIRE(package.Passed());
    REQUIRE(package.artifacts.contains("package"));
    REQUIRE(std::filesystem::exists(package.artifacts["package"].get<std::string>()));

    auto inspected = Astra::Tools::Inspect(package.artifacts["package"].get<std::string>(), options);
    REQUIRE(inspected.Passed());
    REQUIRE(inspected.artifacts["document"]["schema"] == Astra::Asset::PackageManifestSchema);
    REQUIRE(inspected.artifacts["package_manifest"]["schema"] == Astra::Asset::PackageManifestSchema);

    Astra::Tools::CommandOptions deterministic_options = options;
    deterministic_options.compare = true;
    auto native_package = Astra::Tools::Package(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", deterministic_options);
    REQUIRE(native_package.Passed());
    REQUIRE(native_package.artifacts["package_manifest"]["profile"] == "deterministic");
    REQUIRE(native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"].size() >= 7);
    REQUIRE(native_package.artifacts["package_manifest"]["cook_manifest"]["ddc_entries"].size() >= native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"].size());
    REQUIRE(native_package.artifacts["package_manifest"]["payloads"].size() >= native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"].size());
    REQUIRE_FALSE(native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"][0]["ddc_key"].get<std::string>().empty());
    REQUIRE_FALSE(native_package.artifacts["package_manifest"]["package_hash"].get<std::string>().empty());
    REQUIRE(native_package.artifacts["package_manifest"]["runtime_evidence"]["asset_registry"]["entries"].size() >= 7);
    REQUIRE(std::filesystem::exists(native_package.artifacts["golden_replay"].get<std::string>()));

    auto native_inspected = Astra::Tools::Inspect(native_package.artifacts["package"].get<std::string>(), options);
    REQUIRE(native_inspected.Passed());
    REQUIRE(native_inspected.artifacts["package_manifest"]["cook_manifest"]["ddc_entries"].size() >= 7);
    REQUIRE(native_inspected.artifacts["package_mount"]["assets"].size() >= 7);
    REQUIRE(native_inspected.artifacts["payload_smoke"]["asset_id"].get<std::string>().starts_with("native:/"));

    Astra::Core::DiagnosticSink package_reader_diagnostics;
    Astra::Asset::PackageReader reader;
    auto opening_asset = Astra::Asset::ParseAssetUri("native:/Scripts/opening");
    REQUIRE(opening_asset);
    auto opening_payload = reader.ReadPayloadText(native_package.artifacts["package"].get<std::string>(), opening_asset.Value(), package_reader_diagnostics);
    REQUIRE(opening_payload);
    REQUIRE(opening_payload.Value().find("label opening") != std::string::npos);
    auto opening_chunks = reader.ReadPayloadChunks(native_package.artifacts["package"].get<std::string>(), opening_asset.Value(), 32, package_reader_diagnostics);
    REQUIRE(opening_chunks);
    REQUIRE(opening_chunks.Value().size() > 1);
    REQUIRE(opening_chunks.Value().back().final_chunk);
    auto package_mount = reader.MountPackage(native_package.artifacts["package"].get<std::string>(), package_reader_diagnostics);
    REQUIRE(package_mount);
    REQUIRE(package_mount.Value().assets.size() >= 7);

    auto tampered_package_json = native_package.artifacts["package_manifest"];
    tampered_package_json["package_hash"] = "sha256:tampered";
    const auto tampered_package = std::filesystem::temp_directory_path() / "astra_nativevn_tampered.astrapkg";
    {
        std::ofstream file(tampered_package, std::ios::binary);
        file << tampered_package_json.dump(2) << "\n";
    }
    Astra::Core::DiagnosticSink package_diagnostics;
    auto tampered = reader.ReadManifest(tampered_package, package_diagnostics);
    REQUIRE_FALSE(tampered);
    REQUIRE(package_diagnostics.HasBlocking());

    auto tampered_payload_json = native_package.artifacts["package_manifest"];
    tampered_payload_json["payloads"][0]["data"] = "dGFtcGVyZWQ=";
    tampered_payload_json["package_hash"] = Astra::Asset::ComputePackageManifestHash(tampered_payload_json);
    const auto tampered_payload_package = std::filesystem::temp_directory_path() / "astra_nativevn_payload_tampered.astrapkg";
    {
        std::ofstream file(tampered_payload_package, std::ios::binary);
        file << tampered_payload_json.dump(2) << "\n";
    }
    Astra::Core::DiagnosticSink payload_diagnostics;
    auto tampered_payload = reader.ReadManifest(tampered_payload_package, payload_diagnostics);
    REQUIRE_FALSE(tampered_payload);
    REQUIRE(payload_diagnostics.HasBlocking());

    auto replay = Astra::Tools::Replay(native_package.artifacts["golden_replay"].get<std::string>(), deterministic_options);
    REQUIRE(replay.Passed());
    REQUIRE(replay.artifacts["comparison"]["passed"] == true);

    const auto descriptor = std::filesystem::path(ASTRA_PHASE1_PLUGIN_DESCRIPTOR);
    auto plugin = Astra::Tools::Validate(descriptor, options);
    REQUIRE(plugin.Passed());
    REQUIRE(plugin.artifacts["plugin"]["modules"][0].contains("sha256"));

    Astra::Tools::CommandOptions run_options;
    run_options.headless_smoke = true;
    auto run = Astra::Tools::Run(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/PackageSmoke", run_options);
    REQUIRE(run.Passed());
    REQUIRE(run.artifacts["headless_smoke"]["phase3"]["media_release_gate"]["selected_providers"].size() == 3);
    REQUIRE(run.artifacts["headless_smoke"]["phase3"]["media_backend_capabilities"]["schema"] == "astra.media.backend_capabilities.v1");
    REQUIRE(run.artifacts["headless_smoke"]["phase3"]["image_decode_smoke"]["decoded_by"] == "libpng");
    REQUIRE_FALSE(run.artifacts["headless_smoke"]["phase3"]["headless_capture"]["render_hash"].get<std::string>().empty());

    auto native_run = Astra::Tools::Run(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", run_options);
    REQUIRE(native_run.Passed());
    REQUIRE(native_run.artifacts["headless_smoke"]["phase4_script_vn"]["status"] == "passed");

    auto package_run = Astra::Tools::Run(native_package.artifacts["package"].get<std::string>(), run_options);
    REQUIRE(package_run.Passed());
    REQUIRE(package_run.artifacts["headless_smoke"]["package_manifest"]["schema"] == Astra::Asset::PackageManifestSchema);
    REQUIRE(package_run.artifacts["headless_smoke"]["package_payload_smoke"]["read_mode"] == "random_access");
    REQUIRE(package_run.artifacts["headless_smoke"]["package_payload_smoke"]["stream_chunks"].get<std::size_t>() > 1);
}
#endif
