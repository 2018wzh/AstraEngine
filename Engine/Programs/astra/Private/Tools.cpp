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

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdlib>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <set>
#include <sstream>
#include <vector>

namespace Astra::Tools {

namespace {

constexpr const char* ValidationSchema = "astra.validation.report.v1";
constexpr const char* Phase3SmokeSchema = "astra.phase3.foundation.smoke.v1";
constexpr const char* Phase4SmokeSchema = "astra.phase4.script_vn.foundation.v1";
constexpr const char* ReplayEvidenceSchema = "astra.nativevn.replay.v1";

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

std::filesystem::path ResolveToolTarget(const std::filesystem::path& target) {
    const auto absolute = std::filesystem::absolute(target);
    if (std::filesystem::exists(absolute)) {
        return absolute;
    }
    const auto under_binary_root = BinaryRoot() / target;
    if (std::filesystem::exists(under_binary_root)) {
        return std::filesystem::absolute(under_binary_root);
    }
    return absolute;
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

std::vector<Astra::Core::u8> ReadBytes(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    const std::string bytes(std::istreambuf_iterator<char>(file), {});
    return {bytes.begin(), bytes.end()};
}

void WriteBinaryFile(const std::filesystem::path& path, std::string_view payload) {
    if (path.has_parent_path()) {
        std::filesystem::create_directories(path.parent_path());
    }
    std::ofstream file(path, std::ios::binary);
    file.write(payload.data(), static_cast<std::streamsize>(payload.size()));
}

std::string StableSourcePath(const std::filesystem::path& path) {
    const auto absolute = std::filesystem::absolute(path).lexically_normal();
    const auto root = std::filesystem::absolute(SourceRoot()).lexically_normal();
    auto relative = absolute.lexically_relative(root);
    if (!relative.empty() && !relative.generic_string().starts_with("..")) {
        return relative.generic_string();
    }
    return absolute.generic_string();
}

std::string Sha256Text(std::string_view text) {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    EVP_DigestUpdate(context, text.data(), text.size());
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

std::string Base64Encode(std::string_view payload) {
    if (payload.empty()) {
        return {};
    }
    if (payload.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        return {};
    }
    std::vector<unsigned char> encoded(4 * ((payload.size() + 2) / 3) + 1);
    const auto encoded_size = EVP_EncodeBlock(encoded.data(), reinterpret_cast<const unsigned char*>(payload.data()), static_cast<int>(payload.size()));
    return std::string(reinterpret_cast<const char*>(encoded.data()), static_cast<std::size_t>(encoded_size));
}

std::vector<Astra::Core::u8> OnePixelPngBytes() {
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

void WriteJsonFile(const std::filesystem::path& path, const nlohmann::json& json) {
    if (path.has_parent_path()) {
        std::filesystem::create_directories(path.parent_path());
    }
    std::ofstream file(path, std::ios::binary);
    file << json.dump(2) << "\n";
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
        {"ASTRA_PACKAGE_HASH_MISMATCH", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_CHUNK_INVALID", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_ENCODING", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_MISSING", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_NOT_FOUND", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_SIZE_MISMATCH", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_SOURCE_MISSING", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_COOK_MANIFEST_HASH_MISMATCH", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_ENTRY_INVALID", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_ARTIFACT_MISSING", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_ARTIFACT_HASH_MISMATCH", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_BACKEND_FAILED", "media.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_BACKEND_MISSING", "media.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_DECODE_FAILED", "media.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_EMPTY", "media.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_FORMAT_UNSUPPORTED", "media.foundation", Astra::Core::DiagnosticSeverity::Blocking, true},
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
    const auto png_bytes = OnePixelPngBytes();
    auto image_decode = Astra::Media::InspectImageBytes(png_bytes, diagnostics);
    return {
        {"schema", Phase3SmokeSchema},
        {"headless_capture", Astra::Media::ToJson(capture)},
        {"image_decode_smoke", image_decode ? Astra::Media::ToJson(image_decode.Value()) : nlohmann::json::object({{"schema", "astra.media.image_decode.v1"}, {"status", "failed"}})},
        {"filter_profile", Astra::Media::ToJson(profile)},
        {"media_backend_capabilities", Astra::Media::ToJson(Astra::Media::ProbeMediaBackendCapabilities())},
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
    native_source.file = StableSourcePath(native_path);
    native_source.text = ReadText(native_path);

    Astra::Script::ScriptSource lua_source;
    lua_source.runtime_id = Astra::Script::LuaRuntimeId;
    lua_source.source_id = "native:/Scripts/opening";
    lua_source.file = StableSourcePath(lua_path);
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

bool IsRuntimeBinary(const std::filesystem::path& path) {
#if defined(_WIN32)
    return path.extension() == ".dll" && path.filename().string().starts_with("Astra");
#elif defined(__APPLE__)
    return path.extension() == ".dylib" && path.filename().string().starts_with("libAstra");
#else
    return path.extension() == ".so" && path.filename().string().starts_with("libAstra");
#endif
}

nlohmann::json EngineDllEvidence(CommandReport& report) {
    nlohmann::json binaries = nlohmann::json::array();
    const auto bin_root = BinaryRoot() / "Bin";
    if (!std::filesystem::exists(bin_root)) {
        AddDiagnostic(report, "ASTRA_ENGINE_DLL_ROOT_MISSING", Astra::Core::DiagnosticSeverity::Warning, "Engine DLL output root is missing before build.", bin_root);
        return binaries;
    }
    for (const auto& entry : std::filesystem::directory_iterator(bin_root)) {
        if (!entry.is_regular_file() || !IsRuntimeBinary(entry.path())) {
            continue;
        }
        binaries.push_back({
            {"path", entry.path().string()},
            {"name", entry.path().filename().string()},
            {"sha256", Sha256File(entry.path())},
        });
    }
    if (binaries.empty()) {
        AddDiagnostic(report, "ASTRA_ENGINE_DLL_EVIDENCE_MISSING", Astra::Core::DiagnosticSeverity::Warning, "No Astra engine DLLs were found in the runtime output directory.", bin_root);
    }
    return binaries;
}

std::filesystem::path PackagePathForSample(const std::filesystem::path& sample) {
    return BinaryRoot() / "Saved/Packages" / (sample.filename().string() + ".astrapkg");
}

std::filesystem::path ReplayPathForSample(const std::filesystem::path& sample) {
    if (IsNativeVnSample(sample)) {
        return BinaryRoot() / "Saved/Replays/NativeVNGolden.replay";
    }
    return BinaryRoot() / "Saved/Replays" / (sample.filename().string() + ".replay");
}

std::filesystem::path CookManifestPathForSample(const std::filesystem::path& sample) {
    return BinaryRoot() / "Saved/Cooked" / sample.filename() / "cook-manifest.json";
}

std::filesystem::path DdcManifestPathForSample(const std::filesystem::path& sample) {
    return BinaryRoot() / "Saved/DDC" / sample.filename() / "ddc-manifest.json";
}

std::filesystem::path DdcRootForSample(const std::filesystem::path& sample) {
    return BinaryRoot() / "Saved/DDC" / sample.filename();
}

Astra::Asset::AssetRegistry ScanSampleRegistry(const std::filesystem::path& sample, Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Asset::AssetRegistryBuilder builder;
    const auto content_root = sample / "Content";
    if (!std::filesystem::exists(content_root)) {
        return {};
    }
    return builder.Scan(content_root, diagnostics);
}

std::string CookProcessorForType(std::string_view type) {
    if (type == "image") {
        return "astra.cook.image.foundation";
    }
    if (type == "audio") {
        return "astra.cook.audio.foundation";
    }
    if (type == "filter_profile") {
        return "astra.cook.filter_profile.foundation";
    }
    if (type == "script.native") {
        return "astra.cook.script.native";
    }
    if (type == "script.lua") {
        return "astra.cook.script.lua";
    }
    return "astra.cook.asset.foundation";
}

std::filesystem::path ArtifactPathForSample(const std::filesystem::path& sample, const std::filesystem::path& artifact) {
    const auto absolute_artifact = std::filesystem::absolute(artifact).lexically_normal();
    const auto absolute_sample = std::filesystem::absolute(sample).lexically_normal();
    const auto relative = absolute_artifact.lexically_relative(absolute_sample);
    if (!relative.empty() && !relative.generic_string().starts_with("..")) {
        return std::filesystem::path(relative.generic_string());
    }
    return std::filesystem::path(absolute_artifact.generic_string());
}

nlohmann::json BuildCookArtifactMetadata(const Astra::Asset::AssetRegistryEntry& entry, Astra::Core::DiagnosticSink& diagnostics) {
    nlohmann::json metadata = nlohmann::json::object();
    if (entry.type != "image") {
        return metadata;
    }
    auto extension = entry.source_path.extension().string();
    std::ranges::transform(extension, extension.begin(), [](unsigned char character) {
        return static_cast<char>(std::tolower(character));
    });
    if (extension != ".png" && extension != ".jpg" && extension != ".jpeg" && extension != ".webp") {
        metadata["media_inspect"] = {
            {"schema", "astra.media.image_decode.v1"},
            {"status", "skipped"},
            {"reason", "source_extension_not_png_jpeg_webp"},
        };
        return metadata;
    }
    const auto bytes = ReadBytes(entry.source_path);
    auto image = Astra::Media::InspectImageBytes(bytes, diagnostics);
    if (image) {
        auto image_json = Astra::Media::ToJson(image.Value());
        image_json["status"] = "decoded";
        metadata["media_inspect"] = image_json;
    } else {
        metadata["media_inspect"] = {
            {"schema", "astra.media.image_decode.v1"},
            {"status", "failed"},
            {"source_path", entry.source_path.string()},
        };
    }
    return metadata;
}

void AddRegistryArtifactsToCookManifest(const Astra::Asset::AssetRegistry& registry, const std::filesystem::path& sample, Astra::Asset::CookManifest& manifest, Astra::Core::DiagnosticSink& diagnostics) {
    std::set<std::string> existing;
    for (const auto& artifact : manifest.artifacts) {
        existing.insert(artifact.asset_id.ToString());
    }
    for (const auto& entry : registry.entries) {
        if (!existing.insert(entry.id.ToString()).second) {
            continue;
        }
        Astra::Asset::CookArtifact artifact;
        artifact.asset_id = entry.id;
        artifact.type = entry.type;
        artifact.artifact_path = ArtifactPathForSample(sample, entry.source_path);
        artifact.hash = std::filesystem::exists(entry.source_path) ? Sha256Text(ReadText(entry.source_path)) : entry.sidecar_hash;
        artifact.processor_id = CookProcessorForType(entry.type);
        artifact.metadata = BuildCookArtifactMetadata(entry, diagnostics);
        manifest.artifacts.push_back(std::move(artifact));
    }
}

void PopulateDdcEntries(Astra::Asset::CookManifest& manifest, const std::filesystem::path& sample) {
    manifest.ddc_entries.clear();
    const auto engine_version = BuildInfoJson().value("engine_version", "");
    for (auto& artifact : manifest.artifacts) {
        const auto input_hash = artifact.hash;
        const auto settings_hash = Sha256Text(artifact.processor_id + "|" + artifact.type + "|" + manifest.profile + "|ddc.v1");
        const auto ddc_key = "ddc:/" + artifact.processor_id + "/" + settings_hash + "/" + input_hash;
        artifact.ddc_key = ddc_key;

        Astra::Core::u64 size_bytes = 0;
        const auto source_path = sample / artifact.artifact_path;
        if (std::filesystem::exists(source_path)) {
            size_bytes = static_cast<Astra::Core::u64>(std::filesystem::file_size(source_path));
        }

        Astra::Asset::DerivedDataCacheEntry entry;
        entry.key = ddc_key;
        entry.processor_id = artifact.processor_id;
        entry.input_hash = input_hash;
        entry.settings_hash = settings_hash;
        entry.output_hash = artifact.hash;
        entry.platform = "win64";
        entry.profile = manifest.profile;
        entry.engine_version = engine_version;
        entry.rebuilt = true;
        entry.artifacts.push_back({
            std::filesystem::path("cache") / artifact.processor_id / (artifact.hash + ".bin"),
            artifact.hash,
            size_bytes,
        });
        manifest.ddc_entries.push_back(std::move(entry));
    }
}

nlohmann::json WriteDdcArtifacts(Astra::Asset::CookManifest& manifest, const std::filesystem::path& sample, const std::filesystem::path& ddc_root) {
    Astra::Core::u64 rebuilt = 0;
    Astra::Core::u64 reused = 0;
    Astra::Core::u64 corruption_recovered = 0;
    for (auto& entry : manifest.ddc_entries) {
        if (entry.artifacts.empty()) {
            continue;
        }
        auto& cache_artifact = entry.artifacts.front();
        const auto cache_path = ddc_root / cache_artifact.path;
        const auto source_artifact = std::ranges::find_if(manifest.artifacts, [&](const Astra::Asset::CookArtifact& artifact) {
            return artifact.ddc_key == entry.key;
        });
        if (source_artifact == manifest.artifacts.end()) {
            continue;
        }
        const auto source_path = sample / source_artifact->artifact_path;
        const auto payload = ReadText(source_path);
        const auto expected_hash = source_artifact->hash;
        const auto existing_hash = std::filesystem::exists(cache_path) ? Sha256File(cache_path) : std::string();
        entry.corruption_recovered = !existing_hash.empty() && existing_hash != expected_hash;
        entry.rebuilt = existing_hash != expected_hash;
        if (entry.rebuilt) {
            WriteBinaryFile(cache_path, payload);
            ++rebuilt;
            if (entry.corruption_recovered) {
                ++corruption_recovered;
            }
        } else {
            ++reused;
        }
        cache_artifact.hash = expected_hash;
        if (std::filesystem::exists(cache_path)) {
            cache_artifact.size_bytes = static_cast<Astra::Core::u64>(std::filesystem::file_size(cache_path));
        }
    }
    return {
        {"schema", "astra.asset.ddc.execution.v1"},
        {"root", ddc_root.string()},
        {"rebuilt", rebuilt},
        {"reused", reused},
        {"corruption_recovered", corruption_recovered},
    };
}

void PopulatePackagePayloads(Astra::Asset::PackageManifest& manifest, const std::filesystem::path& sample, CommandReport& report) {
    manifest.payloads.clear();
    std::set<std::string> added;
    for (const auto& artifact : manifest.cook_manifest.artifacts) {
        if (!added.insert(artifact.asset_id.ToString()).second) {
            continue;
        }
        const auto source_path = sample / artifact.artifact_path;
        if (!std::filesystem::exists(source_path)) {
            AddDiagnostic(report, "ASTRA_PACKAGE_PAYLOAD_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact source payload is missing and cannot be embedded in the package.", source_path);
            continue;
        }
        const auto payload = ReadText(source_path);
        Astra::Asset::PackagePayloadEntry entry;
        entry.asset_id = artifact.asset_id;
        entry.artifact_path = artifact.artifact_path;
        entry.hash = Sha256Text(payload);
        entry.size_bytes = static_cast<Astra::Core::u64>(payload.size());
        entry.encoding = "base64";
        entry.data = Base64Encode(payload);
        manifest.payloads.push_back(std::move(entry));
    }
}

std::set<std::string> RegistryIds(const Astra::Asset::AssetRegistry& registry) {
    std::set<std::string> ids;
    for (const auto& entry : registry.entries) {
        ids.insert(entry.id.ToString());
    }
    return ids;
}

void ValidatePhase4AssetReferences(const nlohmann::json& phase4, const Astra::Asset::AssetRegistry& registry, CommandReport& report, const std::filesystem::path& sample) {
    if (!phase4.is_object() || !phase4.contains("native")) {
        return;
    }
    const auto ids = RegistryIds(registry);
    auto validate_uri = [&](const std::string& value) {
        if (!value.starts_with("native:/")) {
            return;
        }
        if (!ids.contains(value)) {
            AddDiagnostic(report, "ASTRA_ASSET_REFERENCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "NativeVN script references an asset missing from the AssetRegistry.", sample);
            report.diagnostics.back().objects.push_back({"AssetId", value});
        }
    };
    const auto events = phase4["native"]["runtime_save"]["payload"].value("replay_events", nlohmann::json::array());
    for (const auto& event : events) {
        const auto payload = event.value("payload", nlohmann::json::object());
        for (const auto& key : {"asset", "voice", "profile"}) {
            if (payload.contains(key) && payload.at(key).is_string()) {
                validate_uri(payload.at(key).get<std::string>());
            }
        }
    }
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
    const auto absolute = ResolveToolTarget(target);
    if (std::filesystem::is_directory(absolute) && std::filesystem::exists(SampleDescriptor(absolute))) {
        IsFoundationSample(absolute, report);
        ValidateApiCoverage(report);
        report.artifacts["foundation_core_gate"] = FoundationCoreGateEvidence();
        report.artifacts["engine_binaries"] = EngineDllEvidence(report);
        Astra::Core::DiagnosticSink diagnostics;
        const auto registry = ScanSampleRegistry(absolute, diagnostics);
        if (!registry.entries.empty()) {
            report.artifacts["asset_registry"] = Astra::Asset::ToJson(registry);
            report.artifacts["asset_dependency_graph"] = Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry));
        }
        report.artifacts["phase3_media_backend_capabilities"] = Astra::Media::ToJson(Astra::Media::ProbeMediaBackendCapabilities());
        report.artifacts["phase3_media_release_gate"] = Phase3MediaReleaseGateEvidence(diagnostics);
        if (IsNativeVnSample(absolute)) {
            report.artifacts["phase4_script_vn"] = Phase4ScriptVnSmoke(absolute, diagnostics);
            ValidatePhase4AssetReferences(report.artifacts["phase4_script_vn"], registry, report, absolute);
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
        report.artifacts["engine_binaries"] = EngineDllEvidence(report);
    } else {
        AddDiagnostic(report, "ASTRA_VALIDATE_TARGET_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Foundation validate supports repository roots, plugin descriptors, and Phase 1-3 foundation sample descriptors only.", absolute);
    }
    return report;
}

CommandReport Inspect(const std::filesystem::path& target, const CommandOptions&) {
    auto report = MakeReport("astra inspect");
    const auto absolute = ResolveToolTarget(target);
    if (std::filesystem::is_regular_file(absolute) && HasSuffix(absolute, ".yaml")) {
        ValidatePlugin(absolute, report);
    } else if (std::filesystem::is_regular_file(absolute) && (absolute.extension() == ".json" || absolute.extension() == ".astrapkg" || absolute.extension() == ".replay")) {
        std::ifstream file(absolute, std::ios::binary);
        report.artifacts["document"] = nlohmann::json::parse(file, nullptr, false);
        if (report.artifacts["document"].is_discarded()) {
            AddDiagnostic(report, "ASTRA_INSPECT_JSON_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "JSON document could not be parsed.", absolute);
        } else if (report.artifacts["document"].value("schema", "") == Astra::Asset::PackageManifestSchema) {
            Astra::Core::DiagnosticSink diagnostics;
            Astra::Asset::PackageReader reader;
            auto manifest = reader.ReadManifest(absolute, diagnostics);
            AppendDiagnostics(report, diagnostics);
            if (manifest) {
                report.artifacts["package_manifest"] = Astra::Asset::ToJson(manifest.Value());
                auto mount = reader.MountPackage(absolute, diagnostics);
                AppendDiagnostics(report, diagnostics);
                diagnostics.Clear();
                if (mount) {
                    report.artifacts["package_mount"] = Astra::Asset::ToJson(mount.Value());
                }
                if (!manifest.Value().payloads.empty()) {
                    auto payload = reader.ReadPayloadBytes(absolute, manifest.Value().payloads.front().asset_id, diagnostics);
                    AppendDiagnostics(report, diagnostics);
                    diagnostics.Clear();
                    if (payload) {
                        report.artifacts["payload_smoke"] = {
                            {"asset_id", manifest.Value().payloads.front().asset_id.ToString()},
                            {"size_bytes", payload.Value().size()},
                            {"hash", manifest.Value().payloads.front().hash},
                        };
                    }
                }
            }
        }
    } else {
        AddDiagnostic(report, "ASTRA_INSPECT_TARGET_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Foundation inspect supports plugin YAML and foundation JSON reports only.", absolute);
    }
    return report;
}

CommandReport Cook(const std::filesystem::path& sample, const CommandOptions& options) {
    auto report = MakeReport("astra cook");
    if (!IsFoundationSample(sample, report)) {
        return report;
    }
    const auto cooked = CookManifestPathForSample(sample);
    Astra::Core::DiagnosticSink diagnostics;
    const auto registry = ScanSampleRegistry(sample, diagnostics);
    Astra::Asset::CookManifest manifest;
    manifest.project_id = "package:/" + sample.filename().string();
    manifest.profile = options.config;

    auto add_artifact = [&](std::string id, std::string type, std::filesystem::path artifact_path, std::string processor_id, std::string payload) {
        auto uri = Astra::Asset::ParseAssetUri(id);
        if (!uri) {
            return;
        }
        Astra::Asset::CookArtifact artifact;
        artifact.asset_id = uri.Value();
        artifact.type = std::move(type);
        artifact.artifact_path = std::move(artifact_path);
        artifact.hash = Sha256Text(payload);
        artifact.processor_id = std::move(processor_id);
        manifest.artifacts.push_back(std::move(artifact));
    };

    nlohmann::json cook_report = {{"schema", Astra::Asset::CookManifestSchema}, {"sample", sample.filename().string()}, {"status", "runtime-cooked"}, {"phase3_smoke", Phase3FoundationSmoke(diagnostics)}};
    if (IsNativeVnSample(sample)) {
        const auto native_path = sample / "Content/Scripts/opening.astra";
        const auto lua_path = sample / "Content/Scripts/opening.lua";
        add_artifact("native:/Scripts/opening", "script.native", native_path.lexically_relative(sample), "astra.cook.script.native", ReadText(native_path));
        add_artifact("native:/Scripts/opening_lua", "script.lua", lua_path.lexically_relative(sample), "astra.cook.script.lua", ReadText(lua_path));
        cook_report["phase4_script_vn"] = Phase4ScriptVnSmoke(sample, diagnostics);
        cook_report["runtime_feature_complete"] = {
            {"dialogue", true},
            {"choice", true},
            {"background", true},
            {"character", true},
            {"audio_command", true},
            {"filter_profile", true},
            {"save_restore", cook_report["phase4_script_vn"].value("status", "failed") == "passed"},
        };
        ValidatePhase4AssetReferences(cook_report["phase4_script_vn"], registry, report, sample);
    }
    AddRegistryArtifactsToCookManifest(registry, sample, manifest, diagnostics);
    PopulateDdcEntries(manifest, sample);
    auto ddc_status = WriteDdcArtifacts(manifest, sample, DdcRootForSample(sample));
    manifest.manifest_hash = Astra::Asset::ComputeCookManifestHash(Astra::Asset::ToJson(manifest));
    cook_report["cook_manifest"] = Astra::Asset::ToJson(manifest);
    cook_report["ddc_status"] = ddc_status;
    cook_report["asset_registry"] = Astra::Asset::ToJson(registry);
    cook_report["asset_dependency_graph"] = Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry));
    WriteJsonFile(cooked, cook_report);
    const auto ddc_path = DdcManifestPathForSample(sample);
    WriteJsonFile(ddc_path, {{"schema", "astra.asset.ddc_manifest.v1"}, {"sample", sample.filename().string()}, {"profile", manifest.profile}, {"status", ddc_status}, {"entries", cook_report["cook_manifest"]["ddc_entries"]}});
    AppendDiagnostics(report, diagnostics);
    report.artifacts["cook_report"] = cooked.string();
    report.artifacts["ddc_status"] = ddc_status;
    report.artifacts["ddc_root"] = DdcRootForSample(sample).string();
    report.artifacts["ddc_manifest"] = ddc_path.string();
    report.artifacts["cook_manifest"] = Astra::Asset::ToJson(manifest);
    return report;
}

CommandReport Package(const std::filesystem::path& sample, const CommandOptions& options) {
    auto report = MakeReport("astra package");
    if (!IsFoundationSample(sample, report)) {
        return report;
    }
    const auto package = PackagePathForSample(sample);
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    Astra::Core::DiagnosticSink diagnostics;
    const auto registry = ScanSampleRegistry(sample, diagnostics);
    const auto phase3 = Phase3FoundationSmoke(diagnostics);
    nlohmann::json phase4;
    if (IsNativeVnSample(sample)) {
        phase4 = Phase4ScriptVnSmoke(sample, diagnostics);
    }

    Astra::Asset::CookManifest cook_manifest;
    cook_manifest.project_id = "package:/" + sample.filename().string();
    cook_manifest.profile = options.config;
    if (IsNativeVnSample(sample)) {
        auto native_uri = Astra::Asset::ParseAssetUri("native:/Scripts/opening");
        auto lua_uri = Astra::Asset::ParseAssetUri("native:/Scripts/opening_lua");
        if (native_uri) {
            cook_manifest.artifacts.push_back({native_uri.Value(), "script.native", "Content/Scripts/opening.astra", Sha256Text(ReadText(sample / "Content/Scripts/opening.astra")), "astra.cook.script.native", ""});
        }
        if (lua_uri) {
            cook_manifest.artifacts.push_back({lua_uri.Value(), "script.lua", "Content/Scripts/opening.lua", Sha256Text(ReadText(sample / "Content/Scripts/opening.lua")), "astra.cook.script.lua", ""});
        }
    }
    AddRegistryArtifactsToCookManifest(registry, sample, cook_manifest, diagnostics);
    PopulateDdcEntries(cook_manifest, sample);
    (void)WriteDdcArtifacts(cook_manifest, sample, DdcRootForSample(sample));
    cook_manifest.manifest_hash = Astra::Asset::ComputeCookManifestHash(Astra::Asset::ToJson(cook_manifest));

    Astra::Asset::PackageManifest manifest;
    manifest.package_id = "package:/" + sample.filename().string();
    manifest.profile = options.compare ? "deterministic" : options.profile;
    manifest.project_hash = Sha256Text(ReadText(SampleDescriptor(sample)));
    manifest.cook_manifest = cook_manifest;
    manifest.runtime_evidence = {
        {"source_sample", std::filesystem::absolute(sample).lexically_normal().generic_string()},
        {"build_info", report.build_info},
        {"engine_binaries", EngineDllEvidence(report)},
        {"phase3_headless", phase3},
        {"phase4_script_vn", phase4.is_null() ? nlohmann::json::object() : phase4},
        {"asset_registry", Astra::Asset::ToJson(registry)},
        {"asset_dependency_graph", Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry))},
    };
    ValidatePhase4AssetReferences(phase4, registry, report, sample);
    if (std::filesystem::exists(plugin_descriptor)) {
        manifest.modules.push_back({"phase1.example.runtime", plugin_descriptor.string(), Sha256File(plugin_descriptor), true});
    }
    PopulatePackagePayloads(manifest, sample, report);
    auto package_json = Astra::Asset::ToJson(manifest);
    package_json["package_hash"] = Astra::Asset::ComputePackageManifestHash(package_json);
    WriteJsonFile(package, package_json);

    if (IsNativeVnSample(sample) && phase4.is_object()) {
        const auto replay_path = ReplayPathForSample(sample);
        nlohmann::json replay = {
            {"schema", ReplayEvidenceSchema},
            {"sample", sample.filename().string()},
            {"source_sample", std::filesystem::absolute(sample).lexically_normal().generic_string()},
            {"package", package.string()},
            {"expected_hashes", phase4["native"]["hashes"]},
            {"runtime_replay", phase4["native"]["runtime_save"]["payload"]["replay_events"]},
            {"presentation_capture", phase4["native"]["headless_capture"]},
        };
        WriteJsonFile(replay_path, replay);
        report.artifacts["golden_replay"] = replay_path.string();
    }
    AppendDiagnostics(report, diagnostics);
    report.artifacts["package"] = package.string();
    report.artifacts["package_manifest"] = package_json;
    return report;
}

CommandReport Run(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra run");
    if (!options.headless_smoke) {
        AddDiagnostic(report, "ASTRA_RUN_HEADLESS_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Phase 1 run only supports --headless-smoke.");
        return report;
    }

    Astra::Core::DiagnosticSink diagnostics;
    auto path = ResolveToolTarget(target);
    nlohmann::json package_manifest;
    nlohmann::json package_mount;
    nlohmann::json package_payload_smoke;
    if (std::filesystem::is_regular_file(path) && path.extension() == ".astrapkg") {
        Astra::Asset::PackageReader reader;
        auto manifest = reader.ReadManifest(path, diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
        if (!manifest) {
            return report;
        }
        package_manifest = Astra::Asset::ToJson(manifest.Value());
        auto mount = reader.MountPackage(path, diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
        if (mount) {
            package_mount = Astra::Asset::ToJson(mount.Value());
        }
        auto payload_id = Astra::Asset::ParseAssetUri("native:/Scripts/opening");
        if ((!payload_id || std::ranges::none_of(manifest.Value().payloads, [&](const Astra::Asset::PackagePayloadEntry& payload) { return payload.asset_id.ToString() == payload_id.Value().ToString(); })) && !manifest.Value().payloads.empty()) {
            payload_id = Astra::Core::Result<Astra::Asset::AssetUri>::Success(manifest.Value().payloads.front().asset_id);
        }
        if (payload_id) {
            auto payload = reader.ReadPayloadBytes(path, payload_id.Value(), diagnostics);
            AppendDiagnostics(report, diagnostics);
            diagnostics.Clear();
            auto chunks = reader.ReadPayloadChunks(path, payload_id.Value(), 64, diagnostics);
            AppendDiagnostics(report, diagnostics);
            diagnostics.Clear();
            if (payload) {
                package_payload_smoke = {
                    {"asset_id", payload_id.Value().ToString()},
                    {"size_bytes", payload.Value().size()},
                    {"stream_chunks", chunks ? chunks.Value().size() : 0},
                    {"read_mode", "random_access"},
                };
            }
        }
        const auto source_sample = manifest.Value().runtime_evidence.value("source_sample", "");
        if (!source_sample.empty()) {
            path = source_sample;
        }
    }
    if (std::filesystem::is_directory(path) && !IsFoundationSample(path, report)) {
        return report;
    }

    auto platform = Astra::Platform::CreateHeadlessPlatform();
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
        {"package_manifest", package_manifest.is_null() ? nlohmann::json::object() : package_manifest},
        {"package_mount", package_mount.is_null() ? nlohmann::json::object() : package_mount},
        {"package_payload_smoke", package_payload_smoke.is_null() ? nlohmann::json::object() : package_payload_smoke},
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

CommandReport Replay(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra replay");
    const auto path = ResolveToolTarget(target);
    if (!std::filesystem::is_regular_file(path)) {
        AddDiagnostic(report, "ASTRA_REPLAY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Replay file is missing.", path);
        return report;
    }
    std::ifstream file(path, std::ios::binary);
    auto replay = nlohmann::json::parse(file, nullptr, false);
    if (replay.is_discarded() || replay.value("schema", "") != ReplayEvidenceSchema) {
        AddDiagnostic(report, "ASTRA_REPLAY_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Replay file has an unsupported schema.", path);
        return report;
    }
    report.artifacts["replay"] = replay;
    if (!options.compare) {
        return report;
    }

    const auto sample = std::filesystem::path(replay.value("source_sample", ""));
    if (!std::filesystem::exists(sample)) {
        AddDiagnostic(report, "ASTRA_REPLAY_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Replay source sample is missing.", sample);
        return report;
    }
    Astra::Core::DiagnosticSink diagnostics;
    auto phase4 = Phase4ScriptVnSmoke(sample, diagnostics);
    AppendDiagnostics(report, diagnostics);
    if (phase4.value("status", "failed") != "passed") {
        AddDiagnostic(report, "ASTRA_REPLAY_RUN_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Replay comparison could not re-run NativeVN headless evidence.", sample);
        return report;
    }
    const auto expected_json = replay.at("expected_hashes");
    const auto actual_json = phase4["native"]["hashes"];
    Astra::Runtime::RuntimeHashes expected{
        expected_json.value("state_hash", ""),
        expected_json.value("event_hash", ""),
        expected_json.value("presentation_hash", ""),
    };
    Astra::Runtime::RuntimeHashes actual{
        actual_json.value("state_hash", ""),
        actual_json.value("event_hash", ""),
        actual_json.value("presentation_hash", ""),
    };
    auto comparison = Astra::Runtime::CompareReplayHashes(expected, actual);
    report.artifacts["comparison"] = Astra::Runtime::ToJson(comparison);
    if (!comparison.passed) {
        AddDiagnostic(report, "ASTRA_REPLAY_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Replay hashes do not match.", path);
    }
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
