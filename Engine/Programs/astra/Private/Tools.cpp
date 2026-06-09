#include <Astra/Tools/Tools.hpp>

#include <Astra/Asset/Asset.hpp>
#include <Astra/AstraVN/AstraVN.hpp>
#include <Astra/Core/BuildInfo.hpp>
#include <Astra/Core/Config.hpp>
#include <Astra/Core/Logging.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Media/Media.hpp>
#include <Astra/ModuleRuntime/ModuleRuntime.hpp>
#include <Astra/Platform/Platform.hpp>
#include <Astra/PropertySystem/PropertySystem.hpp>
#include <Astra/Script/Script.hpp>

#include <openssl/evp.h>
#include <yaml-cpp/yaml.h>

#include <array>
#include <cstdlib>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>

namespace Astra::Tools {

namespace {

constexpr const char* ValidationSchema = "astra.validation.report.v1";
constexpr const char* PackageSchema = "astra.foundation.package.v1";
constexpr const char* Phase3SmokeSchema = "astra.phase3.foundation.smoke.v1";
constexpr const char* Phase4SmokeSchema = "astra.phase4.script_vn.foundation.v1";

std::filesystem::path SourceRoot() {
#if defined(ASTRA_SOURCE_ROOT)
    return ASTRA_SOURCE_ROOT;
#else
    return std::filesystem::current_path();
#endif
}

std::filesystem::path BinaryRoot() {
#if defined(ASTRA_BINARY_ROOT)
    return ASTRA_BINARY_ROOT;
#else
    return std::filesystem::current_path() / "build";
#endif
}

nlohmann::json BuildInfoJson() {
    const auto info = Astra::Core::GetBuildInfo();
    nlohmann::json features = nlohmann::json::array();
    for (const auto& feature : info.EnabledFeatures()) {
        features.push_back(feature);
    }
    return {
        {"engine_version", info.engine_version},
        {"git_commit", info.git_commit},
        {"build_config", info.build_config},
        {"abi_version", info.abi_version},
        {"features", features},
    };
}

void AddDiagnostic(CommandReport& report, std::string code, Astra::Core::DiagnosticSeverity severity, std::string message, std::filesystem::path file = {}) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "tools.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source.file = file.string();
    report.diagnostics.push_back(std::move(diagnostic));
    if (severity == Astra::Core::DiagnosticSeverity::Blocking || severity == Astra::Core::DiagnosticSeverity::Fatal) {
        report.status = "failed";
    }
}

bool HasSuffix(const std::filesystem::path& path, std::string_view suffix) {
    return path.filename().string().ends_with(suffix);
}

std::string ReadText(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), {});
}

std::filesystem::path SampleDescriptor(const std::filesystem::path& sample) {
    return sample / "astra.sample.yaml";
}

YAML::Node LoadYamlFile(const std::filesystem::path& path, CommandReport& report) {
    try {
        return YAML::LoadFile(path.string());
    } catch (const YAML::Exception& error) {
        AddDiagnostic(report, "ASTRA_TOOLS_YAML_PARSE", Astra::Core::DiagnosticSeverity::Blocking, error.what(), path);
        return {};
    }
}

bool IsFoundationSample(const std::filesystem::path& sample, CommandReport& report) {
    const auto descriptor_path = SampleDescriptor(sample);
    if (!std::filesystem::exists(descriptor_path)) {
        AddDiagnostic(report, "ASTRA_SAMPLE_DESCRIPTOR_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Sample descriptor astra.sample.yaml is missing.", descriptor_path);
        return false;
    }

    const auto descriptor = LoadYamlFile(descriptor_path, report);
    if (!descriptor) {
        return false;
    }
    if (descriptor["foundation_only"].as<bool>(false) != true || descriptor["phase"].as<int>(0) > 4) {
        AddDiagnostic(report, "ASTRA_SAMPLE_NOT_FOUNDATION", Astra::Core::DiagnosticSeverity::Blocking, "Foundation tools accept Phase 1-4 foundation-only sample descriptors.", descriptor_path);
        return false;
    }
    return true;
}

bool IsNativeVnSample(const std::filesystem::path& sample) {
    return sample.filename().string() == "NativeVN";
}

Astra::Media::FilterProfile MakePhase3FoundationFilterProfile() {
    auto filter = Astra::Asset::ParseAssetUri("native:/Filters/soft_vn");
    Astra::Media::FilterProfile profile;
    if (filter) {
        profile.id = filter.Value();
    }
    profile.passes.push_back({"bg_soften", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Background, {{"strength", 0.25}}});
    profile.passes.push_back({"character_line", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Character, {{"strength", 0.5}}});
    profile.passes.push_back({"final_grade", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Final, {{"grade", "warm"}}});
    return profile;
}

nlohmann::json Phase3MediaReleaseGateEvidence(Astra::Core::DiagnosticSink& diagnostics) {
    auto providers = Astra::Media::FoundationMediaProviders();
    Astra::Media::MediaReleaseGateRequest request;
    request.providers = providers;
    request.filter_profile = MakePhase3FoundationFilterProfile();
    for (const auto& provider : providers) {
        request.selected_providers[provider.slot_id] = provider.provider_id;
    }

    auto report = Astra::Media::ValidateMediaReleaseGate(request, diagnostics);
    if (!report) {
        return {{"schema", Astra::Media::MediaReleaseGateSchema}, {"passed", false}};
    }
    return Astra::Media::ToJson(report.Value());
}

void RegisterFoundationGateCodes(Astra::Core::DiagnosticCodeRegistry& registry) {
    const std::vector<Astra::Core::DiagnosticCodeDescriptor> codes = {
        {"ASTRA_CORE_ASSERT", "core.error", Astra::Core::DiagnosticSeverity::Fatal, true},
        {"ASTRA_CORE_MIGRATION_MISSING", "core.serialization", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_CORE_UNKNOWN_FIELD", "core.serialization", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DIAGNOSTIC_CODE_UNREGISTERED", "core.diagnostics", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLATFORM_SDL_INIT", "platform.sdl", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLATFORM_SDL_WINDOW", "platform.sdl", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_ABI_INVALID", "module.abi", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_BINARY_LOAD", "module.lifecycle", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_ENTRYPOINT_MISSING", "module.abi", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_PROVIDER_MISSING", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_PROVIDER_SLOT_MISMATCH", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_RELEASE_BINARY_MISSING", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_SERVICE_RESOLVE_DENIED", "module.service", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_BINARY_MISSING", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_CAPABILITY_PERMISSION", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DEPENDENCY_CYCLE", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DEPENDENCY_MISSING", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_MODULES", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_PARSE", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_RELEASE_FIELDS", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_REQUIRED", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_ENTRYPOINT_ESCAPE", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_MODULE_PHASE_MISMATCH", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_MODULE_POLICY", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_MODULE_REQUIRED", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_PACKAGED_INELIGIBLE", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_PACKAGED_PERMISSION", "module.descriptor", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_PACKAGED_UNSAFE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_CUSTOM_VALIDATION", "property.validation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_DEPENDENCY", "property.validation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_MIGRATION_MISSING", "property.migration", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_REQUIRED", "property.validation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_TYPE_MISSING", "property.schema", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_VALIDATOR_MISSING", "property.validation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_AI_DENIED", "property.write", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_EDITOR_DENIED", "property.write", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_READ_ONLY", "property.write", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_RELEASE_SENSITIVE", "property.write", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_RUNTIME_DENIED", "property.write", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_UNKNOWN", "property.write", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_API_DESIGN_COVERAGE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_API_MANUAL_COVERAGE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_API_TEST_COVERAGE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DOC_CHECK_FAILED", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_INSPECT_JSON_INVALID", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_INSPECT_TARGET_UNSUPPORTED", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PHASE4_SCRIPT_VN_FAILED", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RUN_HEADLESS_REQUIRED", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RUN_PLUGIN_DESCRIPTOR", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RUN_PLUGIN_LOAD", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_SAMPLE_DESCRIPTOR_MISSING", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_SAMPLE_NOT_FOUNDATION", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_TOOLS_YAML_PARSE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_VALIDATE_TARGET_UNSUPPORTED", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
    };
    for (const auto& code : codes) {
        (void)registry.Register(code);
    }
}

nlohmann::json FoundationCoreGateEvidence() {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Core::DiagnosticCodeRegistry registry;
    RegisterFoundationGateCodes(registry);

    Astra::Core::ConfigStack config;
    config.AddLayer({Astra::Core::ConfigScope::EngineDefault, {{"runtime", {{"deterministic", false}, {"fixed_step_hz", 60}}}}});
    config.AddLayer({Astra::Core::ConfigScope::ReleaseProfile, {{"runtime", {{"deterministic", true}}}}});
    config.AddLayer({Astra::Core::ConfigScope::UserOverride, {{"runtime", {{"deterministic", false}}}}});
    config.AddLayer({Astra::Core::ConfigScope::CommandLine, {{"release", {{"profile", "deterministic"}}}}});
    const auto release_config = config.ResolveForProfile(Astra::Core::ConfigResolveProfile::Release);

    Astra::Core::MigrationRule migration;
    migration.schema = "astra.foundation.gate";
    migration.from_version = 1;
    migration.to_version = 2;
    migration.unknown_field_policy = Astra::Core::UnknownFieldPolicy::Warn;
    migration.diagnostic_code = "ASTRA_CORE_UNKNOWN_FIELD";
    migration.known_fields_after_migration = {"name"};
    nlohmann::json migrated_payload = {{"name", "foundation"}, {"future_field", true}};
    const auto unknown_policy = Astra::Core::ApplyUnknownFieldPolicy(migrated_payload, migration, diagnostics);

    Astra::PropertySystem::TypeRegistry properties;
    Astra::PropertySystem::TypeDescriptor nested;
    nested.type_id = "astra.foundation.nested";
    Astra::PropertySystem::PropertyDescriptor nested_value;
    nested_value.id = "value";
    nested_value.type = "string";
    nested_value.kind = Astra::PropertySystem::TypeKind::Scalar;
    nested.properties.push_back(std::move(nested_value));
    (void)properties.Register(std::move(nested));

    Astra::PropertySystem::PropertyDescriptor review_field;
    review_field.id = "reviewed_name";
    review_field.type = "string";
    review_field.flags = Astra::PropertySystem::PropertyFlag::AiEditable | Astra::PropertySystem::PropertyFlag::RequiresReview;

    Astra::PropertySystem::PropertyDescriptor release_field;
    release_field.id = "package_hash";
    release_field.type = "string";
    release_field.flags = Astra::PropertySystem::PropertyFlag::ReadOnly | Astra::PropertySystem::PropertyFlag::ReleaseSensitive;

    Astra::PropertySystem::PropertyDescriptor nested_field;
    nested_field.id = "nested";
    nested_field.type = "astra.foundation.nested";
    nested_field.kind = Astra::PropertySystem::TypeKind::Struct;

    Astra::PropertySystem::TypeDescriptor gate_type;
    gate_type.type_id = "astra.foundation.gate_type";
    gate_type.properties = {review_field, release_field, nested_field};
    (void)properties.Register(std::move(gate_type));
    (void)properties.RegisterMigration({"astra.foundation.gate_type", 1, 2, {}});
    auto schema = properties.GenerateJsonSchema("astra.foundation.gate_type", diagnostics);
    auto version_path = properties.ValidateSchemaVersion("astra.foundation.gate_type", 1, 2, diagnostics);
    auto write = properties.EvaluateWrite({"astra.foundation.gate_type", "reviewed_name", "Alice", "Bob", true, false, false, false});

    nlohmann::json module_gate = {{"status", "not_available"}};
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    if (std::filesystem::exists(plugin_descriptor)) {
        auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(plugin_descriptor, diagnostics);
        if (descriptor) {
            auto gate = Astra::ModuleRuntime::ValidateModuleReleaseGate(descriptor.Value(), plugin_descriptor.parent_path(), diagnostics);
            if (gate) {
                module_gate = Astra::ModuleRuntime::ToJson(gate.Value());
                for (auto& module : module_gate["modules"]) {
                    const auto binary = plugin_descriptor.parent_path() / module.value("entrypoint", "");
                    if (std::filesystem::exists(binary)) {
                        module["sha256"] = Sha256File(binary);
                    }
                }
            } else {
                module_gate = {{"schema", "astra.module.release_gate.foundation.v1"}, {"passed", false}};
            }
        }
    }

    Astra::Core::ReleasePolicy policy;
    policy.profile = Astra::Core::ReleaseProfile::Deterministic;
    policy.block_on_error = true;
    policy.require_registered_codes = true;
    auto gate_report = Astra::Core::EvaluateFoundationGate(diagnostics, registry, policy);

    return {
        {"schema", "astra.foundation.core_gate.evidence.v1"},
        {"passed", gate_report.passed && version_path && write.allowed && release_config.hash.size() >= 8},
        {"release_config", Astra::Core::ToJson(release_config)},
        {"unknown_field_policy", Astra::Core::ToJson(unknown_policy)},
        {"property_schema", schema},
        {"property_write", Astra::PropertySystem::ToJson(write)},
        {"module_release_gate", module_gate},
        {"gate_report", Astra::Core::ToJson(gate_report)},
    };
}

void AppendDiagnostics(CommandReport& report, const Astra::Core::DiagnosticSink& diagnostics) {
    for (const auto& diagnostic : diagnostics.Diagnostics()) {
        report.diagnostics.push_back(diagnostic);
        if (diagnostic.BlocksRelease()) {
            report.status = "failed";
        }
    }
}

nlohmann::json Phase3FoundationSmoke(Astra::Core::DiagnosticSink& diagnostics) {
    auto background = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room");
    auto character = Astra::Asset::ParseAssetUri("native:/Characters/Alice/Normal");
    auto voice = Astra::Asset::ParseAssetUri("native:/Voice/Alice/opening_001");
    if (!background || !character || !voice) {
        return {{"schema", Phase3SmokeSchema}, {"status", "failed"}};
    }

    auto profile = MakePhase3FoundationFilterProfile();

    std::vector<Astra::Media::PresentationCommand> commands;
    Astra::Media::PresentationCommand background_draw;
    background_draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    background_draw.command_id = "draw.background.room";
    background_draw.frame_index = 1;
    background_draw.layer = "background";
    background_draw.asset = background.Value();
    commands.push_back(std::move(background_draw));
    Astra::Media::PresentationCommand character_draw;
    character_draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    character_draw.command_id = "draw.character.alice";
    character_draw.frame_index = 1;
    character_draw.layer = "character";
    character_draw.order = 100;
    character_draw.asset = character.Value();
    commands.push_back(std::move(character_draw));
    Astra::Media::PresentationCommand text;
    text.kind = Astra::Media::PresentationCommandKind::Text;
    text.command_id = "text.dialogue.001";
    text.frame_index = 1;
    text.layer = "text";
    text.order = 200;
    text.text = "Phase 3 foundation dialogue text";
    text.locale = "en-US";
    text.payload = {{"size", 32}, {"wrap", "rectangle"}};
    commands.push_back(std::move(text));
    Astra::Media::PresentationCommand audio;
    audio.kind = Astra::Media::PresentationCommandKind::Audio;
    audio.command_id = "audio.voice.001";
    audio.frame_index = 1;
    audio.asset = voice.Value();
    audio.bus = "voice";
    audio.payload = {{"kind", "play"}, {"volume", 0.9}, {"loop", false}};
    commands.push_back(std::move(audio));

    auto graph = Astra::Media::ExtractRenderGraph(commands, &profile, diagnostics);
    auto renderer = Astra::Media::CreateHeadlessRenderer2D();
    renderer->Submit(std::move(graph), diagnostics);
    auto capture = renderer->Capture();
    return {
        {"schema", Phase3SmokeSchema},
        {"headless_capture", Astra::Media::ToJson(capture)},
        {"filter_profile", Astra::Media::ToJson(profile)},
        {"media_release_gate", Phase3MediaReleaseGateEvidence(diagnostics)},
    };
}

nlohmann::json Phase4ScriptVnSmoke(const std::filesystem::path& sample, Astra::Core::DiagnosticSink& diagnostics) {
    const auto native_path = sample / "Content/Scripts/opening.astra";
    const auto lua_path = sample / "Content/Scripts/opening.lua";
    if (!std::filesystem::exists(native_path) || !std::filesystem::exists(lua_path)) {
        return {{"schema", Phase4SmokeSchema}, {"status", "failed"}, {"reason", "NativeVN script sources are missing."}};
    }

    Astra::Script::ScriptSource native_source;
    native_source.runtime_id = Astra::Script::NativeRuntimeId;
    native_source.source_id = "native:/Scripts/opening";
    native_source.file = native_path.string();
    native_source.text = ReadText(native_path);

    Astra::Script::ScriptSource lua_source;
    lua_source.runtime_id = Astra::Script::LuaRuntimeId;
    lua_source.source_id = "native:/Scripts/opening";
    lua_source.file = lua_path.string();
    lua_source.text = ReadText(lua_path);

    Astra::Script::ScriptExecutionOptions options;
    options.entry_label = "opening";
    options.choice_index = 0;

    Astra::AstraVN::VnSession native_session(44);
    auto native_result = native_session.RunNative(native_source, options, diagnostics);
    if (!native_result) {
        return {{"schema", Phase4SmokeSchema}, {"status", "failed"}, {"runtime", "native"}, {"reason", native_result.Message()}};
    }
    auto native_snapshot = native_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession lua_session(44);
    auto lua_result = lua_session.RunLua(lua_source, options, diagnostics);
    if (!lua_result) {
        return {{"schema", Phase4SmokeSchema}, {"status", "failed"}, {"runtime", "lua"}, {"reason", lua_result.Message()}};
    }
    auto lua_snapshot = lua_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession restored(44);
    auto restored_result = restored.Restore(native_snapshot, diagnostics);
    const auto restored_hash = restored.Runtime().Hashes();
    const auto save_restored = restored_result && restored_hash.state_hash == native_snapshot.hashes.state_hash && restored_hash.event_hash == native_snapshot.hashes.event_hash;

    const auto native_capture = Astra::Media::ToJson(native_snapshot.headless_capture);
    const auto lua_capture = Astra::Media::ToJson(lua_snapshot.headless_capture);
    const bool parity = native_capture["render_hash"] == lua_capture["render_hash"] && native_capture["text_hash"] == lua_capture["text_hash"] &&
                        native_capture["audio_hash"] == lua_capture["audio_hash"] && native_capture["filter_hash"] == lua_capture["filter_hash"];

    nlohmann::json providers = nlohmann::json::array();
    for (const auto& provider : Astra::Script::FoundationScriptProviders()) {
        providers.push_back(Astra::Script::ToJson(provider));
    }

    return {
        {"schema", Phase4SmokeSchema},
        {"status", parity && save_restored ? "passed" : "failed"},
        {"providers", providers},
        {"foundation_profile", Astra::AstraVN::ToJson(Astra::AstraVN::FoundationProfile())},
        {"native", Astra::AstraVN::ToJson(native_snapshot)},
        {"lua", Astra::AstraVN::ToJson(lua_snapshot)},
        {"parity", {{"presentation_hashes_match", parity}}},
        {"save_restore", {{"restored", save_restored}, {"state_hash", restored_hash.state_hash}, {"event_hash", restored_hash.event_hash}}},
    };
}

void ValidateApiCoverage(CommandReport& report) {
    const auto root = SourceRoot();
    const auto api_text = ReadText(root / "docs/manual/api/README.md");
    const auto foundation_design_text = ReadText(root / "docs/design/foundation-core-platform-property.md");
    const auto actor_design_text = ReadText(root / "docs/design/actor-component-ecs-hybrid.md");
    const auto runtime_design_text = ReadText(root / "docs/design/runtime-core.md");
    const auto roadmap_text = ReadText(root / "docs/design/roadmap.md");
    const auto test_text = ReadText(root / "Engine/Tests/Phase1Tests.cpp");

    const std::array<const char*, 21> headers = {
        "Types.hpp", "Diagnostics.hpp", "Error.hpp", "Profiling.hpp", "Logging.hpp", "Config.hpp", "StableId.hpp", "Serialization.hpp",
        "Path.hpp", "Time.hpp", "BuildInfo.hpp", "Platform.hpp", "ModuleAbi.h", "ModuleRuntime.hpp", "PropertySystem.hpp", "Scene.hpp", "Runtime.hpp",
        "Asset.hpp", "Media.hpp", "Script.hpp", "AstraVN.hpp",
    };
    for (const auto* header : headers) {
        if (!api_text.contains(header)) {
            AddDiagnostic(report, "ASTRA_API_MANUAL_COVERAGE", Astra::Core::DiagnosticSeverity::Blocking, std::string("Manual API index missing ") + header);
        }
        if (!foundation_design_text.contains("Phase 1") || !actor_design_text.contains("Actor") || !runtime_design_text.contains("RuntimeWorld") || !roadmap_text.contains("Phase 4")) {
            AddDiagnostic(report, "ASTRA_API_DESIGN_COVERAGE", Astra::Core::DiagnosticSeverity::Blocking, "Foundation or Phase 2/3 design docs are missing implementation evidence markers.");
            break;
        }
        if (!test_text.contains(header) && std::string_view(header) != "ModuleAbi.h") {
            AddDiagnostic(report, "ASTRA_API_TEST_COVERAGE", Astra::Core::DiagnosticSeverity::Blocking, std::string("Phase1Tests missing header evidence for ") + header);
        }
    }
}

void ValidatePlugin(const std::filesystem::path& descriptor_path, CommandReport& report) {
    Astra::Core::DiagnosticSink diagnostics;
    auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(descriptor_path, diagnostics);
    if (!descriptor) {
        for (const auto& diagnostic : diagnostics.Diagnostics()) {
            report.diagnostics.push_back(diagnostic);
        }
        report.status = "failed";
        return;
    }

    const auto plugin_root = descriptor_path.parent_path();
    auto validation = Astra::ModuleRuntime::ValidatePluginDescriptor(descriptor.Value(), plugin_root, diagnostics);
    auto order = Astra::ModuleRuntime::ResolveModuleOrder(descriptor.Value(), diagnostics);
    (void)validation;
    (void)order;
    for (const auto& diagnostic : diagnostics.Diagnostics()) {
        report.diagnostics.push_back(diagnostic);
        if (diagnostic.BlocksRelease()) {
            report.status = "failed";
        }
    }

    nlohmann::json modules = nlohmann::json::array();
    for (const auto& module : descriptor.Value().modules) {
        nlohmann::json module_report = {
            {"id", module.id},
            {"entrypoint", module.entrypoint},
            {"packaged", module.packaged},
        };
        const auto binary = plugin_root / module.entrypoint;
        if (std::filesystem::exists(binary)) {
            module_report["sha256"] = Sha256File(binary);
        } else {
            AddDiagnostic(report, "ASTRA_PLUGIN_BINARY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Plugin binary is missing for descriptor entrypoint.", binary);
        }
        if (!module.packaged) {
            AddDiagnostic(report, "ASTRA_PLUGIN_PACKAGED_UNSAFE", Astra::Core::DiagnosticSeverity::Warning, "Module is not marked packaged-safe.", descriptor_path);
        }
        modules.push_back(module_report);
    }
    report.artifacts["plugin"] = {
        {"id", descriptor.Value().id},
        {"version", descriptor.Value().version},
        {"packaged_eligible", descriptor.Value().packaged_eligible},
        {"modules", modules},
    };
}

std::filesystem::path PackagePathForSample(const std::filesystem::path& sample) {
    return BinaryRoot() / "Saved/Packages" / (sample.filename().string() + ".foundation.json");
}

CommandReport MakeReport(std::string command) {
    CommandReport report;
    report.command = std::move(command);
    report.build_info = BuildInfoJson();
    return report;
}

} // namespace

bool CommandReport::Passed() const {
    return status == "passed";
}

nlohmann::json ToJson(const CommandReport& report) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", ValidationSchema},
        {"command", report.command},
        {"status", report.status},
        {"build_info", report.build_info},
        {"diagnostics", diagnostics},
        {"artifacts", report.artifacts},
    };
}

void WriteDiagnosticsIfRequested(const CommandReport& report, const CommandOptions& options) {
    if (options.diagnostics_out.empty()) {
        return;
    }
    if (options.diagnostics_out.has_parent_path()) {
        std::filesystem::create_directories(options.diagnostics_out.parent_path());
    }
    std::ofstream file(options.diagnostics_out, std::ios::binary);
    file << ToJson(report).dump(2) << "\n";
}

void PrintReport(const CommandReport& report, const CommandOptions& options) {
    if (options.json) {
        std::cout << ToJson(report).dump(2) << "\n";
    } else {
        std::cout << report.command << ": " << report.status << "\n";
        for (const auto& diagnostic : report.diagnostics) {
            std::cout << diagnostic.code << " [" << Astra::Core::ToString(diagnostic.severity) << "] " << diagnostic.message << "\n";
        }
    }
}

CommandReport VersionReport() {
    auto report = MakeReport("astra --version");
    report.artifacts["version"] = report.build_info;
    return report;
}

CommandReport DocCheck(const CommandOptions& options) {
    auto report = MakeReport("astra doc-check");
    const auto script = SourceRoot() / "tools/doc-check.ps1";
    auto command = "powershell -NoProfile -ExecutionPolicy Bypass -File \"" + script.string() + "\" -Root \"" + SourceRoot().string() + "\"";
    if (options.json) {
        command += " > NUL 2> NUL";
    }
    const auto result = std::system(command.c_str());
    if (result != 0) {
        AddDiagnostic(report, "ASTRA_DOC_CHECK_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "tools/doc-check.ps1 failed.", script);
    }
    return report;
}

CommandReport Validate(const std::filesystem::path& target, const CommandOptions&) {
    auto report = MakeReport("astra validate");
    const auto absolute = std::filesystem::absolute(target);
    if (std::filesystem::is_directory(absolute) && std::filesystem::exists(SampleDescriptor(absolute))) {
        IsFoundationSample(absolute, report);
        ValidateApiCoverage(report);
        report.artifacts["foundation_core_gate"] = FoundationCoreGateEvidence();
        Astra::Core::DiagnosticSink diagnostics;
        report.artifacts["phase3_media_release_gate"] = Phase3MediaReleaseGateEvidence(diagnostics);
        if (IsNativeVnSample(absolute)) {
            report.artifacts["phase4_script_vn"] = Phase4ScriptVnSmoke(absolute, diagnostics);
            if (report.artifacts["phase4_script_vn"].value("status", "failed") != "passed") {
                AddDiagnostic(report, "ASTRA_PHASE4_SCRIPT_VN_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Phase 4 Script/AstraVN foundation smoke failed.", absolute);
            }
        }
        AppendDiagnostics(report, diagnostics);
    } else if (std::filesystem::is_regular_file(absolute) && HasSuffix(absolute, ".yaml")) {
        ValidatePlugin(absolute, report);
    } else if (std::filesystem::is_directory(absolute)) {
        ValidateApiCoverage(report);
        report.artifacts["foundation_core_gate"] = FoundationCoreGateEvidence();
    } else {
        AddDiagnostic(report, "ASTRA_VALIDATE_TARGET_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Foundation validate supports repository roots, plugin descriptors, and Phase 1-3 foundation sample descriptors only.", absolute);
    }
    return report;
}

CommandReport Inspect(const std::filesystem::path& target, const CommandOptions&) {
    auto report = MakeReport("astra inspect");
    const auto absolute = std::filesystem::absolute(target);
    if (std::filesystem::is_regular_file(absolute) && HasSuffix(absolute, ".yaml")) {
        ValidatePlugin(absolute, report);
    } else if (std::filesystem::is_regular_file(absolute) && absolute.extension() == ".json") {
        std::ifstream file(absolute, std::ios::binary);
        report.artifacts["document"] = nlohmann::json::parse(file, nullptr, false);
        if (report.artifacts["document"].is_discarded()) {
            AddDiagnostic(report, "ASTRA_INSPECT_JSON_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "JSON document could not be parsed.", absolute);
        }
    } else {
        AddDiagnostic(report, "ASTRA_INSPECT_TARGET_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Foundation inspect supports plugin YAML and foundation JSON reports only.", absolute);
    }
    return report;
}

CommandReport Cook(const std::filesystem::path& sample, const CommandOptions&) {
    auto report = MakeReport("astra cook");
    if (!IsFoundationSample(sample, report)) {
        return report;
    }
    const auto cooked = BinaryRoot() / "Saved/Cooked" / sample.filename() / "foundation-cook.json";
    std::filesystem::create_directories(cooked.parent_path());
    std::ofstream file(cooked, std::ios::binary);
    Astra::Core::DiagnosticSink diagnostics;
    nlohmann::json cook_report = {{"schema", "astra.foundation.cook.v1"}, {"sample", sample.filename().string()}, {"status", "foundation-only"}, {"phase3_smoke", Phase3FoundationSmoke(diagnostics)}};
    if (IsNativeVnSample(sample)) {
        cook_report["phase4_script_vn"] = Phase4ScriptVnSmoke(sample, diagnostics);
    }
    file << cook_report.dump(2) << "\n";
    AppendDiagnostics(report, diagnostics);
    report.artifacts["cook_report"] = cooked.string();
    return report;
}

CommandReport Package(const std::filesystem::path& sample, const CommandOptions& options) {
    auto report = MakeReport("astra package");
    if (!IsFoundationSample(sample, report)) {
        return report;
    }
    const auto package = PackagePathForSample(sample);
    std::filesystem::create_directories(package.parent_path());
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    nlohmann::json package_json = {
        {"schema", PackageSchema},
        {"sample", sample.filename().string()},
        {"profile", options.profile},
        {"foundation_only", true},
        {"phase3_foundation", true},
        {"phase4_script_vn_foundation", IsNativeVnSample(sample)},
        {"build_info", report.build_info},
        {"example_plugin_descriptor", plugin_descriptor.string()},
    };
    if (std::filesystem::exists(plugin_descriptor)) {
        package_json["example_plugin_descriptor_sha256"] = Sha256File(plugin_descriptor);
    }
    std::ofstream file(package, std::ios::binary);
    file << package_json.dump(2) << "\n";
    report.artifacts["package"] = package.string();
    return report;
}

CommandReport Run(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra run");
    if (!options.headless_smoke) {
        AddDiagnostic(report, "ASTRA_RUN_HEADLESS_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Phase 1 run only supports --headless-smoke.");
        return report;
    }

    const auto path = std::filesystem::absolute(target);
    if (std::filesystem::is_directory(path) && !IsFoundationSample(path, report)) {
        return report;
    }

    auto platform = Astra::Platform::CreateHeadlessPlatform();
    Astra::Core::DiagnosticSink diagnostics;
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(plugin_descriptor, diagnostics);
    if (!descriptor) {
        AddDiagnostic(report, "ASTRA_RUN_PLUGIN_DESCRIPTOR", Astra::Core::DiagnosticSeverity::Blocking, "Could not load Phase 1 example plugin descriptor.", plugin_descriptor);
        return report;
    }
    Astra::ModuleRuntime::ModuleManager manager(platform);
    auto loaded = manager.LoadAndActivate(descriptor.Value(), plugin_descriptor.parent_path(), diagnostics);
    AppendDiagnostics(report, diagnostics);
    if (!loaded) {
        AddDiagnostic(report, "ASTRA_RUN_PLUGIN_LOAD", Astra::Core::DiagnosticSeverity::Blocking, "Phase 1 example plugin failed to load.", plugin_descriptor);
        return report;
    }
    diagnostics.Clear();
    report.artifacts["headless_smoke"] = {
        {"services", manager.Services().Services().size()},
        {"extensions", manager.Extensions().Extensions().size()},
        {"providers", manager.EngineModules().Providers().size()},
        {"phase3", Phase3FoundationSmoke(diagnostics)},
    };
    if (std::filesystem::is_directory(path) && IsNativeVnSample(path)) {
        report.artifacts["headless_smoke"]["phase4_script_vn"] = Phase4ScriptVnSmoke(path, diagnostics);
        if (report.artifacts["headless_smoke"]["phase4_script_vn"].value("status", "failed") != "passed") {
            AddDiagnostic(report, "ASTRA_PHASE4_SCRIPT_VN_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Phase 4 Script/AstraVN foundation smoke failed.", path);
        }
    }
    AppendDiagnostics(report, diagnostics);
    manager.DeactivateAndUnload(diagnostics);
    return report;
}

std::string Sha256File(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return {};
    }

    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    std::array<char, 4096> buffer{};
    while (file.good()) {
        file.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
        if (file.gcount() > 0) {
            EVP_DigestUpdate(context, buffer.data(), static_cast<std::size_t>(file.gcount()));
        }
    }
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_size = 0;
    EVP_DigestFinal_ex(context, digest.data(), &digest_size);
    EVP_MD_CTX_free(context);

    std::ostringstream output;
    for (unsigned int index = 0; index < digest_size; ++index) {
        output << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(digest[index]);
    }
    return output.str();
}

} // namespace Astra::Tools
