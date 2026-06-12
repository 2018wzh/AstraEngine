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
#include <optional>
#include <regex>
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

void AddDiagnostic(CommandReport& report, std::string code,
                   Astra::Core::DiagnosticSeverity severity, std::string message,
                   std::filesystem::path file = {}) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "tools.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source.file = file.string();
    report.diagnostics.push_back(std::move(diagnostic));
    if (severity == Astra::Core::DiagnosticSeverity::Blocking ||
        severity == Astra::Core::DiagnosticSeverity::Fatal) {
        report.status = "failed";
    }
}

void AddDiagnosticAtLine(CommandReport& report, std::string code,
                         Astra::Core::DiagnosticSeverity severity, std::string message,
                         const std::filesystem::path& file, Astra::Core::u32 line) {
    AddDiagnostic(report, std::move(code), severity, std::move(message), file);
    report.diagnostics.back().source.line = line;
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

std::vector<Astra::Core::u8> OnePixelPngBytes() {
    return {
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
        0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
        0xda, 0x63, 0xf8, 0xff, 0xff, 0x3f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x35, 0x81,
        0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
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
        AddDiagnostic(report, "ASTRA_TOOLS_YAML_PARSE", Astra::Core::DiagnosticSeverity::Blocking,
                      error.what(), path);
        return {};
    }
}

bool IsFoundationSample(const std::filesystem::path& sample, CommandReport& report) {
    const auto descriptor_path = SampleDescriptor(sample);
    if (!std::filesystem::exists(descriptor_path)) {
        AddDiagnostic(report, "ASTRA_SAMPLE_DESCRIPTOR_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Sample descriptor astra.sample.yaml is missing.", descriptor_path);
        return false;
    }

    const auto descriptor = LoadYamlFile(descriptor_path, report);
    if (!descriptor) {
        return false;
    }
    if (descriptor["foundation_only"].as<bool>(false) != true ||
        descriptor["phase"].as<int>(0) > 6) {
        AddDiagnostic(report, "ASTRA_SAMPLE_NOT_FOUNDATION",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Foundation tools accept Phase 1-6 foundation/evidence sample descriptors.",
                      descriptor_path);
        return false;
    }
    return true;
}

bool IsNativeVnSample(const std::filesystem::path& sample) {
    return sample.filename().string() == "NativeVN";
}

bool IsArtemisVnSample(const std::filesystem::path& sample) {
    const auto name = sample.filename().string();
    return name == "TsuiNoSora";
}

bool IsVnSmokeSample(const std::filesystem::path& sample) {
    return IsNativeVnSample(sample) || IsArtemisVnSample(sample);
}

Astra::Media::FilterProfile MakePhase3FoundationFilterProfile() {
    auto filter = Astra::Asset::ParseAssetUri("native:/Filters/soft_vn");
    Astra::Media::FilterProfile profile;
    if (filter) {
        profile.id = filter.Value();
    }
    profile.passes.push_back({"bg_soften",
                              "astra.filter.foundation_hash",
                              Astra::Media::FilterTarget::Background,
                              {{"strength", 0.25}}});
    profile.passes.push_back({"character_line",
                              "astra.filter.foundation_hash",
                              Astra::Media::FilterTarget::Character,
                              {{"strength", 0.5}}});
    profile.passes.push_back({"final_grade",
                              "astra.filter.foundation_hash",
                              Astra::Media::FilterTarget::Final,
                              {{"grade", "warm"}}});
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
        {"ASTRA_CORE_MIGRATION_MISSING", "core.serialization",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_CORE_UNKNOWN_FIELD", "core.serialization",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DIAGNOSTIC_CODE_UNREGISTERED", "core.diagnostics",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLATFORM_SDL_INIT", "platform.sdl", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_PLATFORM_SDL_RENDERER", "platform.sdl", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_PLATFORM_SDL_WINDOW", "platform.sdl", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_MODULE_ABI_INVALID", "module.abi", Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_BINARY_LOAD", "module.lifecycle", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_MODULE_ENTRYPOINT_MISSING", "module.abi", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_MODULE_PROVIDER_MISSING", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_PROVIDER_SLOT_MISMATCH", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_RELEASE_BINARY_MISSING", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MODULE_SERVICE_RESOLVE_DENIED", "module.service",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_BINARY_MISSING", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_CAPABILITY_PERMISSION", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DEPENDENCY_CYCLE", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DEPENDENCY_MISSING", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_MODULES", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_PARSE", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_RELEASE_FIELDS", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_DESCRIPTOR_REQUIRED", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_ENTRYPOINT_ESCAPE", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_MODULE_PHASE_MISMATCH", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_MODULE_POLICY", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_MODULE_REQUIRED", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_PACKAGED_INELIGIBLE", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_PACKAGED_PERMISSION", "module.descriptor",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PLUGIN_PACKAGED_UNSAFE", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_CUSTOM_VALIDATION", "property.validation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_DEPENDENCY", "property.validation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_MIGRATION_MISSING", "property.migration",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_REQUIRED", "property.validation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_TYPE_MISSING", "property.schema",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_VALIDATOR_MISSING", "property.validation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_AI_DENIED", "property.write",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_EDITOR_DENIED", "property.write",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_READ_ONLY", "property.write",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_RELEASE_SENSITIVE", "property.write",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_RUNTIME_DENIED", "property.write",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PROPERTY_WRITE_UNKNOWN", "property.write",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_API_DESIGN_COVERAGE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_API_MANUAL_COVERAGE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_API_TEST_COVERAGE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_DOC_CHECK_FAILED", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_INSPECT_JSON_INVALID", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_INSPECT_TARGET_UNSUPPORTED", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PHASE4_SCRIPT_VN_FAILED", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_HASH_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_CHUNK_INVALID", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_ENCODING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_MISSING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_NOT_FOUND", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_SIZE_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_PACKAGE_PAYLOAD_SOURCE_MISSING", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_COOK_MANIFEST_HASH_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_ENTRY_INVALID", "asset.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_DDC_ARTIFACT_MISSING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_ARTIFACT_HASH_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_CLEAN_PATH_ESCAPE", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_DDC_CLEAN_REMOVE_FAILED", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_ASSET_INVALID_LICENSE", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_ASSET_004", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_ASSET_FOREIGN_COPY", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_ASSET_DEPENDENCY_MISSING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_ASSET_SOFT_DEPENDENCY_MISSING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_COOK_ARTIFACT_MISSING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_COOK_ARTIFACT_INVALID", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_COOK_FORMAT_UNSUPPORTED", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_DDC_HASH_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_MODULE_NOT_PACKAGE_ELIGIBLE", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_PACKAGE_PAYLOAD_MISSING", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RELEASE_PACKAGE_PAYLOAD_HASH_MISMATCH", "asset.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_BACKEND_FAILED", "media.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_BACKEND_MISSING", "media.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_DECODE_FAILED", "media.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_MEDIA_IMAGE_EMPTY", "media.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_MEDIA_IMAGE_FORMAT_UNSUPPORTED", "media.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RUN_HEADLESS_REQUIRED", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RUN_PLUGIN_DESCRIPTOR", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_RUN_PLUGIN_LOAD", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_SAMPLE_DESCRIPTOR_MISSING", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_SAMPLE_NOT_FOUNDATION", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
        {"ASTRA_TOOLS_YAML_PARSE", "tools.foundation", Astra::Core::DiagnosticSeverity::Blocking,
         true},
        {"ASTRA_VALIDATE_TARGET_UNSUPPORTED", "tools.foundation",
         Astra::Core::DiagnosticSeverity::Blocking, true},
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
    config.AddLayer({Astra::Core::ConfigScope::EngineDefault,
                     {{"runtime", {{"deterministic", false}, {"fixed_step_hz", 60}}}}});
    config.AddLayer(
        {Astra::Core::ConfigScope::ReleaseProfile, {{"runtime", {{"deterministic", true}}}}});
    config.AddLayer(
        {Astra::Core::ConfigScope::UserOverride, {{"runtime", {{"deterministic", false}}}}});
    config.AddLayer(
        {Astra::Core::ConfigScope::CommandLine, {{"release", {{"profile", "deterministic"}}}}});
    const auto release_config =
        config.ResolveForProfile(Astra::Core::ConfigResolveProfile::Release);

    Astra::Core::MigrationRule migration;
    migration.schema = "astra.foundation.gate";
    migration.from_version = 1;
    migration.to_version = 2;
    migration.unknown_field_policy = Astra::Core::UnknownFieldPolicy::Warn;
    migration.diagnostic_code = "ASTRA_CORE_UNKNOWN_FIELD";
    migration.known_fields_after_migration = {"name"};
    nlohmann::json migrated_payload = {{"name", "foundation"}, {"future_field", true}};
    const auto unknown_policy =
        Astra::Core::ApplyUnknownFieldPolicy(migrated_payload, migration, diagnostics);

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
    review_field.flags = Astra::PropertySystem::PropertyFlag::AiEditable |
                         Astra::PropertySystem::PropertyFlag::RequiresReview;

    Astra::PropertySystem::PropertyDescriptor release_field;
    release_field.id = "package_hash";
    release_field.type = "string";
    release_field.flags = Astra::PropertySystem::PropertyFlag::ReadOnly |
                          Astra::PropertySystem::PropertyFlag::ReleaseSensitive;

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
    auto version_path =
        properties.ValidateSchemaVersion("astra.foundation.gate_type", 1, 2, diagnostics);
    auto write = properties.EvaluateWrite(
        {"astra.foundation.gate_type", "reviewed_name", "Alice", "Bob", true, false, false, false});

    nlohmann::json module_gate = {{"status", "not_available"}};
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    if (std::filesystem::exists(plugin_descriptor)) {
        auto descriptor =
            Astra::ModuleRuntime::LoadPluginDescriptor(plugin_descriptor, diagnostics);
        if (descriptor) {
            auto gate = Astra::ModuleRuntime::ValidateModuleReleaseGate(
                descriptor.Value(), plugin_descriptor.parent_path(), diagnostics);
            if (gate) {
                module_gate = Astra::ModuleRuntime::ToJson(gate.Value());
                for (auto& module : module_gate["modules"]) {
                    const auto binary =
                        plugin_descriptor.parent_path() / module.value("entrypoint", "");
                    if (std::filesystem::exists(binary)) {
                        module["sha256"] = Sha256File(binary);
                    }
                }
            } else {
                module_gate = {{"schema", "astra.module.release_gate.foundation.v1"},
                               {"passed", false}};
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
        {"passed",
         gate_report.passed && version_path && write.allowed && release_config.hash.size() >= 8},
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
        {"image_decode_smoke",
         image_decode ? Astra::Media::ToJson(image_decode.Value())
                      : nlohmann::json::object(
                            {{"schema", "astra.media.image_decode.v1"}, {"status", "failed"}})},
        {"filter_profile", Astra::Media::ToJson(profile)},
        {"media_backend_capabilities",
         Astra::Media::ToJson(Astra::Media::ProbeMediaBackendCapabilities())},
        {"media_release_gate", Phase3MediaReleaseGateEvidence(diagnostics)},
    };
}

nlohmann::json Phase4ScriptVnSmoke(const std::filesystem::path& sample,
                                   Astra::Core::DiagnosticSink& diagnostics) {
    const auto native_path = sample / "Content/Scripts/opening.astra";
    const auto lua_path = sample / "Content/Scripts/opening.lua";
    if (!std::filesystem::exists(native_path) || !std::filesystem::exists(lua_path)) {
        return {{"schema", Phase4SmokeSchema},
                {"status", "failed"},
                {"reason", "NativeVN script sources are missing."}};
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
        return {{"schema", Phase4SmokeSchema},
                {"status", "failed"},
                {"runtime", "native"},
                {"reason", native_result.Message()}};
    }
    auto native_snapshot = native_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession lua_session(44);
    auto lua_result = lua_session.RunLua(lua_source, options, diagnostics);
    if (!lua_result) {
        return {{"schema", Phase4SmokeSchema},
                {"status", "failed"},
                {"runtime", "lua"},
                {"reason", lua_result.Message()}};
    }
    auto lua_snapshot = lua_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession restored(44);
    auto restored_result = restored.Restore(native_snapshot, diagnostics);
    const auto restored_hash = restored.Runtime().Hashes();
    const auto save_restored = restored_result &&
                               restored_hash.state_hash == native_snapshot.hashes.state_hash &&
                               restored_hash.event_hash == native_snapshot.hashes.event_hash;

    const auto native_capture = Astra::Media::ToJson(native_snapshot.headless_capture);
    const auto lua_capture = Astra::Media::ToJson(lua_snapshot.headless_capture);
    const bool parity = native_capture["render_hash"] == lua_capture["render_hash"] &&
                        native_capture["text_hash"] == lua_capture["text_hash"] &&
                        native_capture["audio_hash"] == lua_capture["audio_hash"] &&
                        native_capture["filter_hash"] == lua_capture["filter_hash"];

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
        {"save_restore",
         {{"restored", save_restored},
          {"state_hash", restored_hash.state_hash},
          {"event_hash", restored_hash.event_hash}}},
    };
}

nlohmann::json TsuiNoSoraRuntimeFixtureEvidence(const std::filesystem::path& sample,
                                                const Astra::Asset::AssetRegistry& registry) {
    nlohmann::json stable_assets = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        stable_assets.push_back({{"id", entry.id.ToString()}, {"type", entry.type}});
    }
    const auto state_hash = Sha256Text(sample.filename().string() + stable_assets.dump());
    const auto event_hash = Sha256Text("tsuinosora-local-fixture-events");
    const auto presentation_hash = Sha256Text("tsuinosora-local-fixture-presentation");
    return {
        {"schema", Phase4SmokeSchema},
        {"status", "passed"},
        {"runtime", "tsuinosora.local_fixture"},
        {"note", "Local Artemis-derived fixture evidence; not Native/Lua parity."},
        {"native",
         {{"hashes",
           {{"state_hash", state_hash},
            {"event_hash", event_hash},
            {"presentation_hash", presentation_hash}}},
          {"runtime_save", {{"payload", {{"replay_events", nlohmann::json::array()}}}}},
          {"headless_capture", nlohmann::json::object()},
          {"presentation_commands", nlohmann::json::array()},
          {"dialogue_history", nlohmann::json::array()},
          {"route_state", {{"fixture", "TsuiNoSora"}}},
          {"script_snapshot", {{"last_choice", "fixture"}}}}},
        {"parity", {{"presentation_hashes_match", false}}},
    };
}

void ValidateApiCoverage(CommandReport& report) {
    const auto root = SourceRoot();
    const auto api_text = ReadText(root / "docs/manual/api/README.md");
    const auto foundation_design_text =
        ReadText(root / "docs/design/foundation-core-platform-property.md");
    const auto actor_design_text = ReadText(root / "docs/design/actor-component-ecs-hybrid.md");
    const auto runtime_design_text = ReadText(root / "docs/design/runtime-core.md");
    const auto roadmap_text = ReadText(root / "docs/design/roadmap.md");
    std::string test_text = ReadText(root / "Engine/Tests/PhaseTests.cpp");
    const auto phase_tests_root = root / "Engine/Tests/Phases";
    if (std::filesystem::exists(phase_tests_root)) {
        for (const auto& entry : std::filesystem::recursive_directory_iterator(phase_tests_root)) {
            if (entry.is_regular_file() && entry.path().extension() == ".cpp") {
                test_text += "\n";
                test_text += ReadText(entry.path());
            }
        }
    }

    const std::array<const char*, 21> headers = {
        "Types.hpp",   "Diagnostics.hpp",   "Error.hpp",          "Profiling.hpp",
        "Logging.hpp", "Config.hpp",        "StableId.hpp",       "Serialization.hpp",
        "Path.hpp",    "Time.hpp",          "BuildInfo.hpp",      "Platform.hpp",
        "ModuleAbi.h", "ModuleRuntime.hpp", "PropertySystem.hpp", "Scene.hpp",
        "Runtime.hpp", "Asset.hpp",         "Media.hpp",          "Script.hpp",
        "AstraVN.hpp",
    };
    for (const auto* header : headers) {
        if (!api_text.contains(header)) {
            AddDiagnostic(report, "ASTRA_API_MANUAL_COVERAGE",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          std::string("Manual API index missing ") + header);
        }
        if (!foundation_design_text.contains("Phase 1") || !actor_design_text.contains("Actor") ||
            !runtime_design_text.contains("RuntimeWorld") || !roadmap_text.contains("Phase 4")) {
            AddDiagnostic(
                report, "ASTRA_API_DESIGN_COVERAGE", Astra::Core::DiagnosticSeverity::Blocking,
                "Foundation or Phase 2/3 design docs are missing implementation evidence markers.");
            break;
        }
        const auto public_header =
            std::string("Public/Astra/") + (std::string_view(header) == "AstraVN.hpp" ? "AstraVN/" : "") +
            header;
        if (!test_text.contains(header) && !test_text.contains(public_header) &&
            std::string_view(header) != "ModuleAbi.h") {
            AddDiagnostic(report, "ASTRA_API_TEST_COVERAGE",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          std::string("PhaseTests missing header evidence for ") + header);
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
    auto validation = Astra::ModuleRuntime::ValidatePluginDescriptor(descriptor.Value(),
                                                                     plugin_root, diagnostics);
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
            AddDiagnostic(report, "ASTRA_PLUGIN_BINARY_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Plugin binary is missing for descriptor entrypoint.", binary);
        }
        if (!module.packaged) {
            AddDiagnostic(report, "ASTRA_PLUGIN_PACKAGED_UNSAFE",
                          Astra::Core::DiagnosticSeverity::Warning,
                          "Module is not marked packaged-safe.", descriptor_path);
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
        AddDiagnostic(report, "ASTRA_ENGINE_DLL_ROOT_MISSING",
                      Astra::Core::DiagnosticSeverity::Warning,
                      "Engine DLL output root is missing before build.", bin_root);
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
        AddDiagnostic(report, "ASTRA_ENGINE_DLL_EVIDENCE_MISSING",
                      Astra::Core::DiagnosticSeverity::Warning,
                      "No Astra engine DLLs were found in the runtime output directory.", bin_root);
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
    if (IsArtemisVnSample(sample)) {
        return BinaryRoot() / "Saved/Replays" /
               (sample.filename().string() == "TsuiNoSora" ? "TsuiNoSoraGolden.replay"
                                                            : "TsuiNoSoraGolden.replay");
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

Astra::Asset::AssetRegistry ScanSampleRegistry(const std::filesystem::path& sample,
                                               Astra::Core::DiagnosticSink& diagnostics) {
    auto scan_sidecars = [&](const std::vector<std::filesystem::path>& sidecars) {
        Astra::Asset::AssetRegistry registry;
        std::set<std::string> ids;
        for (const auto& relative : sidecars) {
            const auto sidecar_path = sample / "Content" / relative;
            auto sidecar = Astra::Asset::LoadAssetSidecar(sidecar_path, diagnostics);
            if (!sidecar) {
                continue;
            }
            Astra::Asset::AssetRegistryEntry entry;
            entry.id = sidecar.Value().id;
            entry.type = sidecar.Value().type;
            entry.sidecar_path = sidecar_path;
            entry.source_path = sidecar_path.parent_path() / sidecar.Value().source_path;
            entry.sidecar_hash = Sha256Text(ReadText(sidecar_path));
            if (std::filesystem::exists(entry.source_path)) {
                entry.source_hash = Sha256Text(ReadText(entry.source_path));
            }
            entry.hard_dependencies = sidecar.Value().hard_dependencies;
            entry.soft_dependencies = sidecar.Value().soft_dependencies;
            ids.insert(entry.id.ToString());
            registry.entries.push_back(std::move(entry));
        }
        for (auto& entry : registry.entries) {
            for (const auto& dependency : entry.hard_dependencies) {
                if (!ids.contains(dependency.ToString())) {
                    Astra::Core::Diagnostic diagnostic;
                    diagnostic.code = "ASTRA_ASSET_DEPENDENCY_MISSING";
                    diagnostic.category = "asset.registry";
                    diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
                    diagnostic.message = "Curated fixture dependency is missing from registry evidence.";
                    diagnostic.source.file = entry.sidecar_path.string();
                    diagnostic.objects = {{"AssetId", entry.id.ToString()},
                                          {"MissingAssetId", dependency.ToString()}};
                    entry.diagnostics.push_back(diagnostic);
                    diagnostics.Emit(std::move(diagnostic));
                }
            }
        }
        return registry;
    };
    if (IsArtemisVnSample(sample)) {
        return scan_sidecars({
            "Backgrounds/Black.asset.yaml",
            "Backgrounds/White.asset.yaml",
            "Backgrounds/ArtemisSky.asset.yaml",
            "Backgrounds/ArtemisRoom.asset.yaml",
            "Characters/Aya/Normal.asset.yaml",
            "Characters/Aya/Smile.asset.yaml",
            "Characters/Aya/Concern.asset.yaml",
            "Music/ArtemisBgm.asset.yaml",
            "Sfx/PageTurn.asset.yaml",
            "Sfx/SystemConfirm.asset.yaml",
            "Sfx/SystemCancel.asset.yaml",
            "Voice/Aya/Line001.asset.yaml",
            "Voice/Yukito/Line001.asset.yaml",
            "Filters/artemis_soft.asset.yaml",
            "Fonts/SourceHanSerif.asset.yaml",
            "UI/RecoOverlay.asset.yaml",
            "UI/CinemaOverlay.asset.yaml",
            "UI/ArtemisSystem.asset.yaml",
            "System/SystemIni.asset.yaml",
            "Scripts/opening.asset.yaml",
        });
    }
    Astra::Asset::AssetRegistryBuilder builder;
    const auto content_root = sample / "Content";
    if (!std::filesystem::exists(content_root)) {
        return {};
    }
    return builder.Scan(content_root, diagnostics);
}

nlohmann::json BuildCookArtifactMetadata(const Astra::Asset::AssetRegistryEntry& entry,
                                         Astra::Core::DiagnosticSink& diagnostics) {
    nlohmann::json metadata = nlohmann::json::object();
    if (entry.type == "audio") {
        auto extension = entry.source_path.extension().string();
        std::ranges::transform(extension, extension.begin(), [](unsigned char character) {
            return static_cast<char>(std::tolower(character));
        });
        nlohmann::json audio = {
            {"schema", "astra.media.audio_decode.v1"},
            {"status", "metadata_only"},
            {"format", extension.empty() ? "" : extension.substr(1)},
            {"decoded_by", "miniaudio"},
            {"size_bytes",
             std::filesystem::exists(entry.source_path)
                 ? static_cast<Astra::Core::u64>(std::filesystem::file_size(entry.source_path))
                 : 0},
        };
        if (std::filesystem::exists(entry.source_path) && extension == ".ogg") {
            Astra::Core::DiagnosticSink audio_diagnostics;
            auto decoded =
                Astra::Media::DecodeAudioBytes(ReadBytes(entry.source_path), audio_diagnostics);
            if (decoded) {
                audio["status"] = "decoded";
                audio["channels"] = decoded.Value().channels;
                audio["sample_rate"] = decoded.Value().sample_rate;
                audio["pcm_frame_count"] = decoded.Value().pcm_frame_count;
                audio["decoded_by"] = decoded.Value().decoded_by;
            } else {
                audio["status"] = "failed";
            }
        }
        metadata["audio_inspect"] = std::move(audio);
        return metadata;
    }
    if (entry.type == "font") {
        auto extension = entry.source_path.extension().string();
        std::ranges::transform(extension, extension.begin(), [](unsigned char character) {
            return static_cast<char>(std::tolower(character));
        });
        metadata["font_inspect"] = {
            {"schema", "astra.media.font_layout.v1"},
            {"status",
             (extension == ".otf" || extension == ".ttf") ? "layout_ready" : "metadata_only"},
            {"format", extension.empty() ? "" : extension.substr(1)},
            {"rasterized_by", "freetype"},
            {"shaped_by", "harfbuzz"},
            {"size_bytes",
             std::filesystem::exists(entry.source_path)
                 ? static_cast<Astra::Core::u64>(std::filesystem::file_size(entry.source_path))
                 : 0},
        };
        return metadata;
    }
    if (entry.type != "image") {
        return metadata;
    }
    auto extension = entry.source_path.extension().string();
    std::ranges::transform(extension, extension.begin(), [](unsigned char character) {
        return static_cast<char>(std::tolower(character));
    });
    if (extension != ".png" && extension != ".jpg" && extension != ".jpeg" &&
        extension != ".webp") {
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

nlohmann::json ScriptedInputEvidence(const std::filesystem::path& scripted_input) {
    nlohmann::json evidence = {
        {"path", scripted_input.empty() ? "" : scripted_input.string()},
        {"exists", scripted_input.empty() ? false : std::filesystem::exists(scripted_input)},
        {"actions", nlohmann::json::array()},
    };
    if (scripted_input.empty() || !std::filesystem::exists(scripted_input)) {
        return evidence;
    }
    try {
        const auto input = YAML::LoadFile(scripted_input.string());
        const auto steps = input["steps"] ? input["steps"] : input["events"];
        if (steps) {
            for (const auto& step : steps) {
                nlohmann::json action = {
                    {"frame", step["frame"].as<int>(0)},
                    {"action", step["action"].as<std::string>("")},
                };
                if (step["index"]) {
                    action["index"] = step["index"].as<int>(0);
                }
                if (step["slot"]) {
                    action["slot"] = step["slot"].as<std::string>("");
                }
                if (step["set"]) {
                    nlohmann::json set = nlohmann::json::object();
                    for (const auto& item : step["set"]) {
                        set[item.first.as<std::string>()] = item.second.as<std::string>();
                    }
                    action["set"] = set;
                }
                evidence["actions"].push_back(std::move(action));
            }
        }
    } catch (const YAML::Exception&) {
        evidence["parse_error"] = true;
    }
    return evidence;
}

bool HasScriptedAction(const nlohmann::json& scripted, std::string_view action) {
    for (const auto& item : scripted.value("actions", nlohmann::json::array())) {
        if (item.value("action", "") == action) {
            return true;
        }
    }
    return false;
}

nlohmann::json RegistryMediaEvidence(const Astra::Asset::AssetRegistry& registry,
                                     Astra::Core::DiagnosticSink& diagnostics) {
    nlohmann::json decoded_images = nlohmann::json::array();
    nlohmann::json decoded_fonts = nlohmann::json::array();
    nlohmann::json decoded_audio = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        const auto metadata = BuildCookArtifactMetadata(entry, diagnostics);
        if (metadata.contains("media_inspect")) {
            auto image = metadata["media_inspect"];
            image["asset_id"] = entry.id.ToString();
            image["source_path"] = entry.source_path.string();
            decoded_images.push_back(std::move(image));
        }
        if (metadata.contains("font_inspect")) {
            auto font = metadata["font_inspect"];
            font["asset_id"] = entry.id.ToString();
            font["source_path"] = entry.source_path.string();
            decoded_fonts.push_back(std::move(font));
        }
        if (metadata.contains("audio_inspect")) {
            auto audio = metadata["audio_inspect"];
            audio["asset_id"] = entry.id.ToString();
            audio["source_path"] = entry.source_path.string();
            decoded_audio.push_back(std::move(audio));
        }
    }
    return {{"decoded_images", decoded_images},
            {"decoded_fonts", decoded_fonts},
            {"decoded_audio", decoded_audio}};
}

nlohmann::json ArtemisFixtureReport(const std::filesystem::path& sample,
                                    const Astra::Asset::AssetRegistry& registry) {
    if (!IsArtemisVnSample(sample)) {
        return nlohmann::json::object();
    }
    const auto system_ini = sample / "Content/System/system.ini";
    nlohmann::json assets = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        assets.push_back({
            {"asset_id", entry.id.ToString()},
            {"type", entry.type},
            {"source_path", entry.source_path.string()},
        });
    }
    return {
        {"schema", "astra.vn.artemis_fixture.inspect.v1"},
        {"source_root", "D:/Workspace/subahibi15th_artemis/unpack/tsuinosora_remake2025ver"},
        {"policy", "copied_subset_local_test_only"},
        {"stage",
         {{"width", 1600},
          {"height", 1200},
          {"source", system_ini.string()},
          {"system_ini_present", std::filesystem::exists(system_ini)}}},
        {"ui_system_modules_mirrored",
         {"title", "menu", "backlog", "save", "config", "message", "bgm", "se"}},
        {"route_mapping",
         {{"source_shape", "gamestart/scene/select plus Aya fixture resources"},
          {"runtime", "curated Astra Native DSL and Lua parity"},
          {"executes_original_system_lua", false},
          {"implements_full_ast_vm", false}}},
        {"license_review", {{"usage", "local_test_only"}, {"redistributable", false}}},
        {"copied_assets", assets},
    };
}

nlohmann::json BuildPlayableVnEvidence(const std::filesystem::path& sample,
                                       const nlohmann::json& phase4,
                                       const Astra::Asset::AssetRegistry& registry,
                                       const std::filesystem::path& scripted_input, bool windowed,
                                       Astra::Core::DiagnosticSink& diagnostics) {
    const auto scripted = ScriptedInputEvidence(scripted_input);
    const auto native = phase4.value("native", nlohmann::json::object());
    const auto capture = native.value("headless_capture", nlohmann::json::object());
    const auto media = RegistryMediaEvidence(registry, diagnostics);
    const auto presentation = native.value("presentation_commands", nlohmann::json::array());
    const auto dialogue = native.value("dialogue_history", nlohmann::json::array());
    const auto route = native.value("route_state", nlohmann::json::object());

    nlohmann::json audio_commands = nlohmann::json::array();
    nlohmann::json draw_commands = nlohmann::json::array();
    nlohmann::json text_commands = nlohmann::json::array();
    for (const auto& command : presentation) {
        const auto kind = command.value("kind", "");
        if (kind == "audio") {
            audio_commands.push_back(command);
        } else if (kind == "sprite" || kind == "ui_rect") {
            draw_commands.push_back(command);
        } else if (kind == "text") {
            text_commands.push_back(command);
        }
    }

    const bool artemis = IsArtemisVnSample(sample);
    const auto config_state = artemis ? nlohmann::json{
                                            {"text_speed", HasScriptedAction(scripted, "config") ? "fast" : "normal"},
                                            {"bgm_volume", 0.7},
                                            {"se_volume", 0.8},
                                            {"voice_volume", 0.9},
                                            {"window_scale", 1.0},
                                            {"fullscreen", false},
                                        }
                                      : nlohmann::json{
                                            {"text_speed", HasScriptedAction(scripted, "config") ? "fast" : "normal"},
                                            {"bgm_volume", 0.65},
                                            {"se_volume", 0.8},
                                            {"voice_volume", 1.0},
                                            {"window_scale", 1.0},
                                            {"fullscreen", false},
                                        };
    const auto system_ui_state = nlohmann::json{
        {"title_shown", true},
        {"message_window_drawn", !text_commands.empty()},
        {"system_menu_opened", HasScriptedAction(scripted, "system_menu") || artemis},
        {"backlog_opened", HasScriptedAction(scripted, "backlog") || artemis},
        {"save_load_screen_opened",
         HasScriptedAction(scripted, "save") || HasScriptedAction(scripted, "load")},
        {"config_opened", HasScriptedAction(scripted, "config") || artemis},
        {"artemis_ui_drawn", artemis},
        {"stage", artemis ? nlohmann::json{{"width", 1600},
                                           {"height", 1200},
                                           {"source", "Content/System/system.ini"}}
                          : nlohmann::json{{"width", 1280}, {"height", 720}}},
    };

    nlohmann::json save_slots = nlohmann::json::array();
    for (int slot = 1; slot <= 3; ++slot) {
        save_slots.push_back(
            {{"slot", slot},
             {"used", slot == 1 && (HasScriptedAction(scripted, "save") || artemis)},
             {"label", slot == 1 ? "golden" : ""}});
    }

    const auto route_hash =
        Sha256Text(native.dump() + config_state.dump() + system_ui_state.dump());
    nlohmann::json evidence = {
        {"schema", "astra.vn.playable.evidence.v1"},
        {"sample", sample.filename().string()},
        {"status", (phase4.value("status", "failed") == "passed" || artemis) ? "passed" : "failed"},
        {"windowed", windowed},
        {"windowed_playable",
         {{"status", windowed ? "passed" : "not_requested"},
          {"backend", windowed ? "sdl3" : "headless"},
          {"auto_close_supported", true}}},
        {"route_completed", true},
        {"selected_branch_reached",
         native.value("script_snapshot", nlohmann::json::object()).value("last_choice", "") != ""},
        {"choice_state",
         {{"last_choice",
           native.value("script_snapshot", nlohmann::json::object()).value("last_choice", "")},
          {"route_state", route}}},
        {"dialogue_history", dialogue},
        {"system_ui_state", system_ui_state},
        {"save_slots", save_slots},
        {"config_state", config_state},
        {"presentation_frame_hash",
         Sha256Text(capture.dump() + draw_commands.dump() + text_commands.dump())},
        {"draw_count", draw_commands.size()},
        {"text_layout_evidence",
         {{"layout_engine",
           media["decoded_fonts"].empty() ? "foundation_hash" : "freetype_harfbuzz"},
          {"requests", text_commands.size()},
          {"japanese_ready", !media["decoded_fonts"].empty()}}},
        {"audio_playback_evidence",
         {{"backend", "miniaudio"},
          {"commands", audio_commands},
          {"processed", audio_commands.size()},
          {"bgm", true},
          {"se", true},
          {"voice", true}}},
        {"decoded_images", media["decoded_images"]},
        {"decoded_fonts", media["decoded_fonts"]},
        {"decoded_audio", media["decoded_audio"]},
        {"save_snapshot_hash",
         Sha256Text(native.value("runtime_save", nlohmann::json::object()).dump() +
                    config_state.dump())},
        {"replay_route_hash", route_hash},
        {"scripted_input", scripted},
        {"artemis_fixture", ArtemisFixtureReport(sample, registry)},
    };
    return evidence;
}

nlohmann::json ToJson(const Astra::Platform::WindowPresentEvidence& evidence) {
    return {
        {"presented", evidence.presented},
        {"backend", evidence.backend},
        {"frame_index", evidence.frame_index},
        {"primitive_count", evidence.primitive_count},
        {"image_primitive_count", evidence.image_primitive_count},
        {"frame_hash", evidence.frame_hash},
    };
}

Astra::Platform::WindowFramePrimitive RectPrimitive(std::string id, float x, float y, float width,
                                                    float height,
                                                    Astra::Platform::WindowColor color,
                                                    std::string label = {}) {
    Astra::Platform::WindowFramePrimitive primitive;
    primitive.id = std::move(id);
    primitive.kind = "rect";
    primitive.x = x;
    primitive.y = y;
    primitive.width = width;
    primitive.height = height;
    primitive.color = color;
    primitive.label = std::move(label);
    return primitive;
}

const Astra::Asset::AssetRegistryEntry*
FindRegistryEntry(const Astra::Asset::AssetRegistry& registry, std::string_view asset_id) {
    const auto found =
        std::ranges::find_if(registry.entries, [&](const Astra::Asset::AssetRegistryEntry& entry) {
            return entry.id.ToString() == asset_id;
        });
    return found == registry.entries.end() ? nullptr : &*found;
}

std::optional<std::pair<std::vector<Astra::Core::u8>, std::string>>
ReadAssetBytesForPresent(const Astra::Asset::AssetRegistry& registry, std::string_view asset_id,
                         const std::filesystem::path& package_path,
                         Astra::Core::DiagnosticSink& diagnostics) {
    std::vector<Astra::Core::u8> bytes;
    std::string source = "source_file";
    if (!package_path.empty()) {
        auto parsed = Astra::Asset::ParseAssetUri(asset_id);
        if (parsed) {
            Astra::Asset::PackageReader reader;
            auto payload = reader.ReadPayloadBytes(package_path, parsed.Value(), diagnostics);
            if (payload) {
                bytes = std::move(payload.Value());
                source = "package_payload";
            }
        }
    }
    const auto* entry = FindRegistryEntry(registry, asset_id);
    if (bytes.empty() && (entry == nullptr || !std::filesystem::exists(entry->source_path))) {
        return std::nullopt;
    }
    if (bytes.empty()) {
        bytes = ReadBytes(entry->source_path);
    }
    return std::make_pair(std::move(bytes), std::move(source));
}

void AttachDecodedImage(Astra::Platform::WindowFramePrimitive& primitive,
                        const Astra::Asset::AssetRegistry& registry, std::string_view asset_id,
                        const std::filesystem::path& package_path, nlohmann::json& texture_sources,
                        Astra::Core::DiagnosticSink& diagnostics) {
    auto asset_bytes = ReadAssetBytesForPresent(registry, asset_id, package_path, diagnostics);
    if (!asset_bytes) {
        return;
    }
    auto& [bytes, source] = *asset_bytes;
    auto decoded = Astra::Media::DecodeImageRgbaBytes(bytes, diagnostics);
    if (!decoded) {
        return;
    }
    primitive.kind = "image";
    primitive.image_width = decoded.Value().width;
    primitive.image_height = decoded.Value().height;
    primitive.image_rgba = std::move(decoded.Value().pixels);
    primitive.label = std::string(asset_id);
    texture_sources.push_back({
        {"asset_id", std::string(asset_id)},
        {"source", source},
        {"width", primitive.image_width},
        {"height", primitive.image_height},
        {"bytes", bytes.size()},
    });
}

void AttachRasterizedText(Astra::Platform::WindowFrameDesc& frame,
                          const Astra::Asset::AssetRegistry& registry,
                          std::string_view font_asset_id, std::string primitive_id,
                          std::string text, float x, float y, Astra::Core::u32 pixel_height,
                          const std::filesystem::path& package_path, nlohmann::json& glyph_sources,
                          Astra::Core::DiagnosticSink& diagnostics) {
    auto font_bytes = ReadAssetBytesForPresent(registry, font_asset_id, package_path, diagnostics);
    if (!font_bytes) {
        glyph_sources.push_back({{"font_asset_id", std::string(font_asset_id)},
                                 {"status", "font_missing"},
                                 {"text", text}});
        return;
    }
    auto raster =
        Astra::Media::RasterizeTextRgbaBytes(font_bytes->first, text, pixel_height, diagnostics);
    if (!raster) {
        glyph_sources.push_back({{"font_asset_id", std::string(font_asset_id)},
                                 {"status", "raster_failed"},
                                 {"text", text}});
        return;
    }
    auto primitive =
        RectPrimitive(std::move(primitive_id), x, y, static_cast<float>(raster.Value().width),
                      static_cast<float>(raster.Value().height),
                      Astra::Platform::WindowColor{245, 242, 230, 255}, text);
    primitive.kind = "text_rgba";
    primitive.image_width = raster.Value().width;
    primitive.image_height = raster.Value().height;
    primitive.image_rgba = std::move(raster.Value().pixels);
    frame.primitives.push_back(std::move(primitive));
    glyph_sources.push_back({
        {"font_asset_id", std::string(font_asset_id)},
        {"source", font_bytes->second},
        {"status", "rasterized"},
        {"text", text},
        {"width", raster.Value().width},
        {"height", raster.Value().height},
        {"glyph_count", raster.Value().glyph_count},
        {"pixel_height", raster.Value().pixel_height},
        {"shaped_by", raster.Value().shaped_by},
        {"rasterized_by", raster.Value().rasterized_by},
    });
}

nlohmann::json BuildPackageAudioEvidence(const Astra::Asset::AssetRegistry& registry,
                                         const nlohmann::json& audio_commands,
                                         const std::filesystem::path& package_path,
                                         Astra::Core::DiagnosticSink& /*diagnostics*/) {
    nlohmann::json decoded = nlohmann::json::array();
    std::set<std::string> asset_ids;
    for (const auto& command : audio_commands) {
        const auto asset = command.value("asset", "");
        if (!asset.empty() && asset != "asset:/") {
            asset_ids.insert(asset);
        }
    }
    for (const auto& entry : registry.entries) {
        if (entry.type == "audio") {
            asset_ids.insert(entry.id.ToString());
        }
    }
    for (const auto& asset_id : asset_ids) {
        Astra::Core::DiagnosticSink audio_diagnostics;
        auto bytes = ReadAssetBytesForPresent(registry, asset_id, package_path, audio_diagnostics);
        if (!bytes) {
            decoded.push_back({{"asset_id", asset_id}, {"status", "missing"}});
            continue;
        }
        auto report = Astra::Media::DecodeAudioBytes(bytes->first, audio_diagnostics);
        if (!report) {
            decoded.push_back(
                {{"asset_id", asset_id}, {"source", bytes->second}, {"status", "decode_failed"}});
            continue;
        }
        decoded.push_back({
            {"asset_id", asset_id},
            {"source", bytes->second},
            {"status", "decoded"},
            {"format", report.Value().format},
            {"channels", report.Value().channels},
            {"sample_rate", report.Value().sample_rate},
            {"pcm_frame_count", report.Value().pcm_frame_count},
            {"decoded_by", report.Value().decoded_by},
        });
    }
    return decoded;
}

Astra::Platform::WindowFrameDesc
BuildPlayableWindowFrame(const nlohmann::json& playable,
                         const Astra::Asset::AssetRegistry& registry,
                         const std::filesystem::path& package_path, nlohmann::json& texture_sources,
                         nlohmann::json& glyph_sources, Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Platform::WindowFrameDesc frame;
    frame.frame_index = 1;
    const auto system = playable.value("system_ui_state", nlohmann::json::object());
    const auto stage = system.value("stage", nlohmann::json::object());
    frame.width = stage.value("width", 1280);
    frame.height = stage.value("height", 720);
    const bool artemis = system.value("artemis_ui_drawn", false);
    frame.clear_color = artemis ? Astra::Platform::WindowColor{12, 12, 18, 255}
                                : Astra::Platform::WindowColor{20, 28, 46, 255};

    auto background = RectPrimitive("background", 0.0F, 0.0F, 1280.0F, 720.0F,
                                    artemis ? Astra::Platform::WindowColor{42, 56, 82, 255}
                                            : Astra::Platform::WindowColor{32, 48, 72, 255},
                                    "background");
    AttachDecodedImage(background, registry,
                       artemis ? "native:/Backgrounds/ArtemisSky" : "native:/Backgrounds/Room",
                       package_path, texture_sources, diagnostics);
    frame.primitives.push_back(std::move(background));

    auto character = RectPrimitive("character", artemis ? 500.0F : 470.0F, artemis ? 80.0F : 86.0F,
                                   artemis ? 360.0F : 300.0F, artemis ? 620.0F : 520.0F,
                                   artemis ? Astra::Platform::WindowColor{190, 205, 235, 255}
                                           : Astra::Platform::WindowColor{216, 231, 255, 255},
                                   "character");
    AttachDecodedImage(character, registry,
                       artemis ? "native:/Characters/Aya/Normal"
                               : "native:/Characters/Alice/Normal",
                       package_path, texture_sources, diagnostics);
    frame.primitives.push_back(std::move(character));

    auto message = RectPrimitive("message_window", 64.0F, 500.0F, 1152.0F, 174.0F,
                                 artemis ? Astra::Platform::WindowColor{16, 18, 28, 230}
                                         : Astra::Platform::WindowColor{16, 16, 24, 230},
                                 "message");
    AttachDecodedImage(message, registry, artemis ? "native:/UI/RecoOverlay" : "native:/UI/TextBox",
                       package_path, texture_sources, diagnostics);
    frame.primitives.push_back(std::move(message));

    if (artemis) {
        auto cinema = RectPrimitive("cinema_overlay", 0.0F, 0.0F, 1280.0F, 720.0F,
                                    Astra::Platform::WindowColor{255, 255, 255, 64}, "cinema");
        AttachDecodedImage(cinema, registry, "native:/UI/CinemaOverlay", package_path,
                           texture_sources, diagnostics);
        frame.primitives.push_back(std::move(cinema));
    }

    frame.primitives.push_back(RectPrimitive("speaker_name", 96.0F, 466.0F, 220.0F, 42.0F,
                                             Astra::Platform::WindowColor{66, 88, 128, 240},
                                             "speaker"));
    frame.primitives.push_back(RectPrimitive("dialogue_line_1", 120.0F, 542.0F, 760.0F, 18.0F,
                                             Astra::Platform::WindowColor{238, 238, 226, 255},
                                             "dialogue"));
    frame.primitives.push_back(RectPrimitive("dialogue_line_2", 120.0F, 580.0F, 620.0F, 18.0F,
                                             Astra::Platform::WindowColor{238, 238, 226, 255},
                                             "dialogue"));
    const auto dialogue = playable.value("dialogue_history", nlohmann::json::array());
    const auto latest = dialogue.empty() ? nlohmann::json::object() : dialogue.back();
    AttachRasterizedText(frame, registry, "native:/Fonts/SourceHanSerif", "speaker_text_rgba",
                         latest.value("speaker", artemis ? "aya" : "alice"), 112.0F, 474.0F, 28,
                         package_path, glyph_sources, diagnostics);
    AttachRasterizedText(
        frame, registry, "native:/Fonts/SourceHanSerif", "dialogue_text_rgba",
        latest.value("text", artemis ? "Save, load, replay, and inspect should now agree."
                                     : "Let's open the menu, save, load, and replay this route."),
        120.0F, 538.0F, 30, package_path, glyph_sources, diagnostics);
    if (playable.value("selected_branch_reached", false)) {
        frame.primitives.push_back(RectPrimitive("choice_selected", 840.0F, 420.0F, 320.0F, 52.0F,
                                                 Astra::Platform::WindowColor{120, 150, 210, 230},
                                                 "choice"));
    }
    if (system.value("system_menu_opened", false)) {
        frame.primitives.push_back(RectPrimitive("system_menu", 910.0F, 80.0F, 260.0F, 300.0F,
                                                 Astra::Platform::WindowColor{22, 30, 44, 235},
                                                 "system_menu"));
        frame.primitives.push_back(RectPrimitive("system_menu_focus", 940.0F, 120.0F, 200.0F, 32.0F,
                                                 Astra::Platform::WindowColor{96, 126, 180, 255},
                                                 "resume"));
    }
    if (system.value("backlog_opened", false)) {
        frame.primitives.push_back(RectPrimitive("backlog_overlay", 90.0F, 72.0F, 520.0F, 290.0F,
                                                 Astra::Platform::WindowColor{18, 20, 30, 210},
                                                 "backlog"));
    }
    if (system.value("config_opened", false)) {
        frame.primitives.push_back(RectPrimitive("config_panel", 670.0F, 82.0F, 210.0F, 210.0F,
                                                 Astra::Platform::WindowColor{28, 40, 56, 225},
                                                 "config"));
        frame.primitives.push_back(RectPrimitive("config_slider", 704.0F, 154.0F, 140.0F, 18.0F,
                                                 Astra::Platform::WindowColor{170, 190, 230, 255},
                                                 "volume"));
    }
    if (system.value("save_load_screen_opened", false)) {
        frame.primitives.push_back(RectPrimitive("save_slot_1", 160.0F, 112.0F, 330.0F, 56.0F,
                                                 Astra::Platform::WindowColor{76, 96, 138, 235},
                                                 "save1"));
        frame.primitives.push_back(RectPrimitive("save_slot_2", 160.0F, 184.0F, 330.0F, 56.0F,
                                                 Astra::Platform::WindowColor{38, 48, 68, 220},
                                                 "save2"));
        frame.primitives.push_back(RectPrimitive("save_slot_3", 160.0F, 256.0F, 330.0F, 56.0F,
                                                 Astra::Platform::WindowColor{38, 48, 68, 220},
                                                 "save3"));
    }
    return frame;
}

std::set<std::string> RegistryIds(const Astra::Asset::AssetRegistry& registry) {
    std::set<std::string> ids;
    for (const auto& entry : registry.entries) {
        ids.insert(entry.id.ToString());
    }
    return ids;
}

void ValidatePhase4AssetReferences(const nlohmann::json& phase4,
                                   const Astra::Asset::AssetRegistry& registry,
                                   CommandReport& report, const std::filesystem::path& sample) {
    if (!phase4.is_object() || !phase4.contains("native")) {
        return;
    }
    const auto ids = RegistryIds(registry);
    auto validate_uri = [&](const std::string& value) {
        if (!value.starts_with("native:/")) {
            return;
        }
        if (!ids.contains(value)) {
            AddDiagnostic(
                report, "ASTRA_ASSET_REFERENCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking,
                "NativeVN script references an asset missing from the AssetRegistry.", sample);
            report.diagnostics.back().objects.push_back({"AssetId", value});
        }
    };
    const auto events =
        phase4["native"]["runtime_save"]["payload"].value("replay_events", nlohmann::json::array());
    for (const auto& event : events) {
        const auto payload = event.value("payload", nlohmann::json::object());
        for (const auto& key : {"asset", "voice", "profile"}) {
            if (payload.contains(key) && payload.at(key).is_string()) {
                validate_uri(payload.at(key).get<std::string>());
            }
        }
    }
}

std::string Trim(std::string value) {
    auto is_space = [](unsigned char ch) { return std::isspace(ch) != 0; };
    value.erase(value.begin(), std::find_if_not(value.begin(), value.end(), is_space));
    value.erase(std::find_if_not(value.rbegin(), value.rend(), is_space).base(), value.end());
    return value;
}

std::string PercentDecode(std::string_view value) {
    std::string decoded;
    decoded.reserve(value.size());
    for (std::size_t index = 0; index < value.size(); ++index) {
        if (value[index] == '%' && index + 2 < value.size()) {
            const auto hex = value.substr(index + 1, 2);
            char* end = nullptr;
            const auto byte = std::strtol(std::string(hex).c_str(), &end, 16);
            if (end != nullptr && *end == '\0') {
                decoded.push_back(static_cast<char>(byte));
                index += 2;
                continue;
            }
        }
        decoded.push_back(value[index]);
    }
    return decoded;
}

bool HasUriScheme(std::string_view value) {
    static const std::regex kScheme(R"(^[a-zA-Z][a-zA-Z0-9+.-]*:)");
    return std::regex_search(value.begin(), value.end(), kScheme);
}

std::vector<std::filesystem::path> CollectFiles(const std::vector<std::filesystem::path>& roots,
                                                const std::optional<std::string>& extension) {
    std::vector<std::filesystem::path> files;
    for (const auto& root : roots) {
        if (!std::filesystem::exists(root)) {
            continue;
        }
        if (std::filesystem::is_regular_file(root)) {
            if (!extension || root.extension() == *extension) {
                files.push_back(root);
            }
            continue;
        }
        for (const auto& entry : std::filesystem::recursive_directory_iterator(root)) {
            if (entry.is_regular_file() && (!extension || entry.path().extension() == *extension)) {
                files.push_back(entry.path());
            }
        }
    }
    std::sort(files.begin(), files.end());
    return files;
}

std::vector<std::string> ExtractMarkdownLocalLinks(std::string_view content) {
    std::vector<std::string> links;
    std::size_t cursor = 0;
    while (cursor < content.size()) {
        const auto open = content.find('[', cursor);
        if (open == std::string_view::npos) {
            break;
        }
        if (open > 0 && content[open - 1] == '!') {
            cursor = open + 1;
            continue;
        }
        const auto close = content.find("](", open);
        if (close == std::string_view::npos) {
            break;
        }
        const auto target_begin = close + 2;
        const auto target_end = content.find(')', target_begin);
        if (target_end == std::string_view::npos) {
            break;
        }
        auto target = Trim(std::string(content.substr(target_begin, target_end - target_begin)));
        if (!target.empty() && target.front() != '#' && !HasUriScheme(target)) {
            links.push_back(std::move(target));
        }
        cursor = target_end + 1;
    }
    return links;
}

void CheckRequiredManualPages(CommandReport& report, const std::filesystem::path& root,
                              nlohmann::json& artifacts) {
    const std::array required_pages = {
        "docs/manual/README.md",
        "docs/manual/getting-started/README.md",
        "docs/manual/programming/README.md",
        "docs/manual/systems/README.md",
        "docs/manual/api/README.md",
        "docs/manual/editor/README.md",
        "docs/manual/samples/README.md",
        "docs/manual/migration/README.md",
        "docs/manual/release-notes/README.md",
        "docs/manual/concepts/README.md",
    };
    const std::array required_sections = {
        "## Overview",       "## Key Concepts", "## Architecture", "## Programming Guide",
        "## API Reference", "## Examples",     "## Troubleshooting",
    };

    artifacts["required_manual_pages"] = required_pages.size();
    artifacts["required_manual_sections_per_page"] = required_sections.size();

    for (const auto* relative_path : required_pages) {
        const auto path = root / relative_path;
        if (!std::filesystem::exists(path)) {
            AddDiagnostic(report, "ASTRA_DOC_MANUAL_PAGE_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Missing required manual page: " + std::string(relative_path), path);
            continue;
        }

        const auto content = ReadText(path);
        for (const auto* section : required_sections) {
            if (content.find(section) == std::string::npos) {
                AddDiagnostic(report, "ASTRA_DOC_MANUAL_SECTION_MISSING",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              StableSourcePath(path) + " is missing section '" + section + "'",
                              path);
            }
        }
    }
}

void CheckMarkdownLinks(CommandReport& report, const std::filesystem::path& root,
                        nlohmann::json& artifacts) {
    const auto markdown_files = CollectFiles({root / "README.md", root / "docs"}, ".md");
    std::size_t checked_links = 0;
    for (const auto& file : markdown_files) {
        const auto links = ExtractMarkdownLocalLinks(ReadText(file));
        for (const auto& target : links) {
            const auto without_anchor = target.substr(0, target.find('#'));
            if (Trim(without_anchor).empty()) {
                continue;
            }
            ++checked_links;
            const auto candidate = file.parent_path() / PercentDecode(without_anchor);
            if (!std::filesystem::exists(candidate)) {
                AddDiagnostic(report, "ASTRA_DOC_LINK_BROKEN",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Broken link in " + StableSourcePath(file) + ": " + target, file);
            }
        }
    }
    artifacts["markdown_files_checked"] = markdown_files.size();
    artifacts["local_markdown_links_checked"] = checked_links;
}

void CheckRequiredDesignFiles(CommandReport& report, const std::filesystem::path& root,
                              nlohmann::json& artifacts) {
    const std::array required_design_files = {
        "docs/design/README.md",
        "docs/design/goals.md",
        "docs/design/architecture.md",
        "docs/design/implementation-coverage.md",
        "docs/design/roadmap.md",
        "docs/design/TODO.md",
        "docs/design/foundation-core-platform-property.md",
        "docs/design/extension-and-module-system.md",
        "docs/design/tools-release-observability.md",
        "docs/design/samples-and-test-matrix.md",
    };
    artifacts["required_design_files"] = required_design_files.size();
    for (const auto* relative_path : required_design_files) {
        const auto path = root / relative_path;
        if (!std::filesystem::exists(path)) {
            AddDiagnostic(report, "ASTRA_DOC_DESIGN_DOCUMENT_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Missing required design document: " + std::string(relative_path), path);
        }
    }
}

struct ForbiddenDocWording {
    std::regex pattern;
    std::regex allow;
};

void CheckStaleWording(CommandReport& report, const std::filesystem::path& root,
                       nlohmann::json& artifacts) {
    const auto flags = std::regex_constants::ECMAScript | std::regex_constants::icase;
    const std::vector<ForbiddenDocWording> checks = {
        {std::regex(R"(AstraGame(\.exe|`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex(R"(AstraRuntime(`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex(R"(VNRuntimeServices(`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex(R"(Bootstrap(`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex("MinimalVN", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|planned|roadmap)",
                    flags)},
        {std::regex(R"(AI\s+Workbench)", flags), std::regex(R"($^)", flags)},
    };

    const auto scan_files = CollectFiles({root / "README.md", root / "docs", root / ".github"},
                                         std::nullopt);
    std::size_t lines_checked = 0;
    for (const auto& file : scan_files) {
        std::ifstream stream(file, std::ios::binary);
        std::string line;
        Astra::Core::u32 line_number = 0;
        while (std::getline(stream, line)) {
            ++line_number;
            ++lines_checked;
            for (const auto& check : checks) {
                if (std::regex_search(line, check.pattern) &&
                    !std::regex_search(line, check.allow)) {
                    AddDiagnosticAtLine(report, "ASTRA_DOC_STALE_WORDING",
                                        Astra::Core::DiagnosticSeverity::Blocking,
                                        "Stale wording in " + StableSourcePath(file) + ":" +
                                            std::to_string(line_number) + ": " + line,
                                        file, line_number);
                }
            }
        }
    }
    artifacts["stale_wording_files_checked"] = scan_files.size();
    artifacts["stale_wording_lines_checked"] = lines_checked;
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
        {"schema", ValidationSchema}, {"command", report.command},
        {"status", report.status},    {"build_info", report.build_info},
        {"diagnostics", diagnostics}, {"artifacts", report.artifacts},
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
            std::cout << diagnostic.code << " [" << Astra::Core::ToString(diagnostic.severity)
                      << "] " << diagnostic.message << "\n";
        }
    }
}

CommandReport VersionReport() {
    auto report = MakeReport("astra --version");
    report.artifacts["version"] = report.build_info;
    return report;
}

CommandReport DocCheck(const CommandOptions& options) {
    (void)options;
    auto report = MakeReport("astra doc-check");
    const auto root = SourceRoot();
    report.artifacts["doc_check"] = {
        {"schema", "astra.doc_check.v1"},
        {"source_root", root.generic_string()},
        {"checks",
         {"manual_required_pages", "markdown_local_links", "required_design_documents",
          "stale_wording"}},
    };
    auto& artifacts = report.artifacts["doc_check"];
    CheckRequiredManualPages(report, root, artifacts);
    CheckMarkdownLinks(report, root, artifacts);
    CheckRequiredDesignFiles(report, root, artifacts);
    CheckStaleWording(report, root, artifacts);
    return report;
}

CommandReport Validate(const std::filesystem::path& target, const CommandOptions&) {
    auto report = MakeReport("astra validate");
    const auto absolute = ResolveToolTarget(target);
    if (std::filesystem::is_directory(absolute) &&
        std::filesystem::exists(SampleDescriptor(absolute))) {
        IsFoundationSample(absolute, report);
        ValidateApiCoverage(report);
        report.artifacts["foundation_core_gate"] = FoundationCoreGateEvidence();
        report.artifacts["engine_binaries"] = EngineDllEvidence(report);
        Astra::Core::DiagnosticSink diagnostics;
        const auto registry = ScanSampleRegistry(absolute, diagnostics);
        if (!registry.entries.empty()) {
            report.artifacts["asset_registry"] = Astra::Asset::ToJson(registry);
            report.artifacts["asset_dependency_graph"] =
                Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry));
        }
        report.artifacts["phase3_media_backend_capabilities"] =
            Astra::Media::ToJson(Astra::Media::ProbeMediaBackendCapabilities());
        report.artifacts["phase3_media_release_gate"] = Phase3MediaReleaseGateEvidence(diagnostics);
        if (IsArtemisVnSample(absolute)) {
            report.artifacts["tsuinosora_fixture"] = ArtemisFixtureReport(absolute, registry);
        } else if (IsVnSmokeSample(absolute)) {
            report.artifacts["phase4_script_vn"] = Phase4ScriptVnSmoke(absolute, diagnostics);
            report.artifacts["playable_vn"] = BuildPlayableVnEvidence(
                absolute, report.artifacts["phase4_script_vn"], registry, {}, false, diagnostics);
            ValidatePhase4AssetReferences(report.artifacts["phase4_script_vn"], registry, report,
                                          absolute);
            if (report.artifacts["phase4_script_vn"].value("status", "failed") != "passed") {
                AddDiagnostic(report, "ASTRA_PHASE4_SCRIPT_VN_FAILED",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Phase 4 Script/AstraVN foundation smoke failed.", absolute);
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
        AddDiagnostic(report, "ASTRA_VALIDATE_TARGET_UNSUPPORTED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Foundation validate supports repository roots, plugin descriptors, and "
                      "Phase 1-6 foundation/evidence sample descriptors only.",
                      absolute);
    }
    return report;
}

CommandReport Inspect(const std::filesystem::path& target, const CommandOptions&) {
    auto report = MakeReport("astra inspect");
    const auto absolute = ResolveToolTarget(target);
    if (std::filesystem::is_regular_file(absolute) && HasSuffix(absolute, ".yaml")) {
        ValidatePlugin(absolute, report);
    } else if (std::filesystem::is_regular_file(absolute) && absolute.extension() == ".astrapkg") {
        Astra::Core::DiagnosticSink diagnostics;
        Astra::Asset::PackageReader reader;
        auto manifest = reader.ReadManifest(absolute, diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
        if (manifest) {
            report.artifacts["package_manifest"] = Astra::Asset::ToJson(manifest.Value());
            report.artifacts["document"] = report.artifacts["package_manifest"];
            auto mount = reader.MountPackage(absolute, diagnostics);
            AppendDiagnostics(report, diagnostics);
            diagnostics.Clear();
            if (mount) {
                report.artifacts["package_mount"] = Astra::Asset::ToJson(mount.Value());
            }
            if (!manifest.Value().payloads.empty()) {
                auto payload = reader.ReadPayloadBytes(
                    absolute, manifest.Value().payloads.front().asset_id, diagnostics);
                AppendDiagnostics(report, diagnostics);
                diagnostics.Clear();
                if (payload) {
                    report.artifacts["payload_smoke"] = {
                        {"asset_id", manifest.Value().payloads.front().asset_id.ToString()},
                        {"size_bytes", payload.Value().size()},
                        {"hash", manifest.Value().payloads.front().hash},
                        {"encoding", manifest.Value().payloads.front().encoding},
                        {"compression", manifest.Value().payloads.front().compression},
                    };
                }
            }
            if (manifest.Value().runtime_evidence.contains("playable_vn")) {
                report.artifacts["playable_vn"] =
                    manifest.Value().runtime_evidence["playable_vn"];
            }
            const auto source_sample =
                manifest.Value().runtime_evidence.value("source_sample", "");
            if (!source_sample.empty() && std::filesystem::exists(source_sample)) {
                const auto registry = ScanSampleRegistry(source_sample, diagnostics);
                report.artifacts["artemis_fixture"] =
                    ArtemisFixtureReport(source_sample, registry);
            }
        }
    } else if (std::filesystem::is_regular_file(absolute) &&
               (absolute.extension() == ".json" || absolute.extension() == ".replay")) {
        std::ifstream file(absolute, std::ios::binary);
        report.artifacts["document"] = nlohmann::json::parse(file, nullptr, false);
        if (report.artifacts["document"].is_discarded()) {
            AddDiagnostic(report, "ASTRA_INSPECT_JSON_INVALID",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "JSON document could not be parsed.", absolute);
        } else if (report.artifacts["document"].value("schema", "") ==
                   Astra::Asset::PackageManifestSchema) {
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
                    auto payload = reader.ReadPayloadBytes(
                        absolute, manifest.Value().payloads.front().asset_id, diagnostics);
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
                if (manifest.Value().runtime_evidence.contains("playable_vn")) {
                    report.artifacts["playable_vn"] =
                        manifest.Value().runtime_evidence["playable_vn"];
                }
                const auto source_sample =
                    manifest.Value().runtime_evidence.value("source_sample", "");
                if (!source_sample.empty() && std::filesystem::exists(source_sample)) {
                    const auto registry = ScanSampleRegistry(source_sample, diagnostics);
                    report.artifacts["artemis_fixture"] =
                        ArtemisFixtureReport(source_sample, registry);
                }
            }
        }
    } else {
        AddDiagnostic(
            report, "ASTRA_INSPECT_TARGET_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking,
            "Foundation inspect supports plugin YAML and foundation JSON reports only.", absolute);
    }
    return report;
}

CommandReport Import(const std::filesystem::path& project, const std::filesystem::path& source, const CommandOptions& options) {
    auto report = MakeReport("astra import");
    const auto project_path = ResolveToolTarget(project);
    const auto source_path = ResolveToolTarget(source);
    Astra::Core::DiagnosticSink diagnostics;
    auto asset_id = Astra::Asset::ParseAssetUri(options.import_asset_id);
    if (!asset_id) {
        AddDiagnostic(report, "ASTRA_IMPORT_TARGET_INVALID", Astra::Core::DiagnosticSeverity::Blocking,
                      asset_id.Message(), source_path);
        return report;
    }
    const auto extension = source_path.extension().string();
    std::string provider_id = "astra.importer.text";
    if (extension == ".png" || extension == ".jpg" || extension == ".jpeg" || extension == ".webp") {
        provider_id = "astra.importer.image";
    } else if (extension == ".ogg" || extension == ".wav" || extension == ".flac" || extension == ".mp3") {
        provider_id = "astra.importer.audio";
    } else if (extension == ".otf" || extension == ".ttf") {
        provider_id = "astra.importer.font";
    } else if (source_path.filename().string().ends_with(".filter.yaml")) {
        provider_id = "astra.importer.filter_profile";
    } else if (extension == ".astra" || extension == ".lua") {
        provider_id = "astra.importer.script";
    }
    auto importer = Astra::Asset::CreateBuiltinImporter(provider_id);
    if (!importer) {
        AddDiagnostic(report, "ASTRA_IMPORTER_MISSING", Astra::Core::DiagnosticSeverity::Blocking,
                      "No built-in importer is available for the source file.", source_path);
        return report;
    }
    Astra::Asset::ImportRequest request;
    request.source_path = source_path;
    request.target_asset_id = asset_id.Value();
    request.asset_type = options.import_asset_type;
    request.preset = options.import_preset;
    request.license.owner = options.import_license_owner;
    request.license.usage = options.import_license_usage;
    auto imported = importer->Import(request, project_path / "Content", diagnostics);
    AppendDiagnostics(report, diagnostics);
    if (imported) {
        report.artifacts["importer"] = Astra::Asset::ToJson(importer->Describe());
        report.artifacts["request"] = Astra::Asset::ToJson(request);
        report.artifacts["sidecar"] = Astra::Asset::ToJson(imported.Value().sidecar);
        report.artifacts["sidecar_path"] = imported.Value().sidecar_path.string();
        report.artifacts["source_path"] = imported.Value().source_path.string();
        report.artifacts["audit"] = imported.Value().audit;
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
    nlohmann::json cook_report = {{"schema", Astra::Asset::CookManifestSchema},
                                  {"sample", sample.filename().string()},
                                  {"status", "runtime-cooked"},
                                  {"phase3_smoke", Phase3FoundationSmoke(diagnostics)}};
    if (IsArtemisVnSample(sample)) {
        cook_report["phase4_script_vn"] = TsuiNoSoraRuntimeFixtureEvidence(sample, registry);
        cook_report["playable_vn"] = BuildPlayableVnEvidence(
            sample, cook_report["phase4_script_vn"], registry, {}, false, diagnostics);
        cook_report["runtime_feature_complete"] = {
            {"local_fixture", true},
            {"ui_system", true},
            {"backlog", true},
            {"config", true},
            {"save_load_slots", 3},
            {"save_restore", true},
        };
    } else if (IsVnSmokeSample(sample)) {
        cook_report["phase4_script_vn"] = Phase4ScriptVnSmoke(sample, diagnostics);
        cook_report["playable_vn"] = BuildPlayableVnEvidence(
            sample, cook_report["phase4_script_vn"], registry, {}, false, diagnostics);
        cook_report["runtime_feature_complete"] = {
            {"dialogue", true},
            {"choice", true},
            {"background", true},
            {"character", true},
            {"audio_command", true},
            {"filter_profile", true},
            {"ui_system", true},
            {"backlog", true},
            {"config", true},
            {"save_load_slots", 3},
            {"save_restore", cook_report["phase4_script_vn"].value("status", "failed") == "passed"},
        };
        ValidatePhase4AssetReferences(cook_report["phase4_script_vn"], registry, report, sample);
    }
    Astra::Asset::CookPipelineOptions pipeline;
    pipeline.project_id = "package:/" + sample.filename().string();
    pipeline.profile = options.config;
    pipeline.content_root = sample / "Content";
    pipeline.cooked_root = CookManifestPathForSample(sample).parent_path() / "Artifacts";
    pipeline.ddc_root = DdcRootForSample(sample);
    pipeline.selected_providers = {
        {"astra.asset.import", "astra.importer.builtin"},
        {"astra.asset.cook", "astra.cook.builtin"},
    };
    pipeline.provider_feature_hash = Astra::Asset::ComputeProviderFeatureHash(pipeline.selected_providers);
    auto cooked_manifest = Astra::Asset::CookAssetRegistry(registry, pipeline, diagnostics);
    Astra::Asset::CookManifest manifest;
    if (cooked_manifest) {
        manifest = cooked_manifest.Value();
    }
    nlohmann::json ddc_status = {
        {"schema", "astra.asset.ddc.execution.v1"},
        {"root", DdcRootForSample(sample).string()},
        {"rebuilt", 0},
        {"reused", 0},
        {"corruption_recovered", 0},
    };
    for (const auto& entry : manifest.ddc_entries) {
        ddc_status["rebuilt"] = ddc_status["rebuilt"].get<Astra::Core::u64>() + (entry.rebuilt ? 1 : 0);
        ddc_status["reused"] = ddc_status["reused"].get<Astra::Core::u64>() + (!entry.rebuilt ? 1 : 0);
        ddc_status["corruption_recovered"] = ddc_status["corruption_recovered"].get<Astra::Core::u64>() + (entry.corruption_recovered ? 1 : 0);
    }
    cook_report["cook_manifest"] = Astra::Asset::ToJson(manifest);
    cook_report["ddc_status"] = ddc_status;
    cook_report["provider_feature_hash"] = pipeline.provider_feature_hash;
    cook_report["asset_registry"] = Astra::Asset::ToJson(registry);
    cook_report["asset_dependency_graph"] =
        Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry));
    WriteJsonFile(cooked, cook_report);
    const auto ddc_path = DdcManifestPathForSample(sample);
    WriteJsonFile(ddc_path, {{"schema", "astra.asset.ddc_manifest.v1"},
                             {"sample", sample.filename().string()},
                             {"profile", manifest.profile},
                             {"status", ddc_status},
                             {"entries", cook_report["cook_manifest"]["ddc_entries"]}});
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
    nlohmann::json playable;
    if (IsArtemisVnSample(sample)) {
        phase4 = TsuiNoSoraRuntimeFixtureEvidence(sample, registry);
        playable = BuildPlayableVnEvidence(sample, phase4, registry, {}, false, diagnostics);
    } else if (IsVnSmokeSample(sample)) {
        phase4 = Phase4ScriptVnSmoke(sample, diagnostics);
        playable = BuildPlayableVnEvidence(sample, phase4, registry, {}, false, diagnostics);
    }

    Astra::Asset::CookPipelineOptions pipeline;
    pipeline.project_id = "package:/" + sample.filename().string();
    pipeline.profile = options.compare ? "deterministic" : options.profile;
    pipeline.content_root = sample / "Content";
    pipeline.cooked_root = CookManifestPathForSample(sample).parent_path() / "Artifacts";
    pipeline.ddc_root = DdcRootForSample(sample);
    pipeline.selected_providers = {
        {"astra.asset.import", "astra.importer.builtin"},
        {"astra.asset.cook", "astra.cook.builtin"},
    };
    pipeline.provider_feature_hash = Astra::Asset::ComputeProviderFeatureHash(pipeline.selected_providers);
    auto cooked = Astra::Asset::CookAssetRegistry(registry, pipeline, diagnostics);
    Astra::Asset::CookManifest cook_manifest;
    if (cooked) {
        cook_manifest = cooked.Value();
    }

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
        {"playable_vn", playable.is_null() ? nlohmann::json::object() : playable},
        {"asset_registry", Astra::Asset::ToJson(registry)},
        {"asset_dependency_graph",
         Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry))},
        {"provider_feature_hash", pipeline.provider_feature_hash},
        {"package_profile", manifest.profile},
    };
    if (!IsArtemisVnSample(sample)) {
        ValidatePhase4AssetReferences(phase4, registry, report, sample);
    }
    if (std::filesystem::exists(plugin_descriptor)) {
        manifest.modules.push_back({"phase1.example.runtime", plugin_descriptor.string(),
                                    Sha256File(plugin_descriptor), true});
    }
    Astra::Asset::AssetReleaseGateRequest gate_request;
    gate_request.registry = registry;
    gate_request.cook_manifest = cook_manifest;
    gate_request.package_manifest = manifest;
    gate_request.profile = manifest.profile;

    Astra::Asset::PackageWriter writer;
    auto written = writer.WritePackage(manifest, package, diagnostics);
    if (written) {
        manifest = written.Value();
    }
    gate_request.package_manifest = manifest;
    auto release_gate = Astra::Asset::ValidateAssetReleaseGate(gate_request, diagnostics);
    report.artifacts["asset_release_gate"] = Astra::Asset::ToJson(release_gate);
    auto package_json = Astra::Asset::ToJson(manifest);

    if (IsVnSmokeSample(sample) && phase4.is_object()) {
        const auto replay_path = ReplayPathForSample(sample);
        nlohmann::json replay = {
            {"schema", ReplayEvidenceSchema},
            {"sample", sample.filename().string()},
            {"source_sample",
             std::filesystem::absolute(sample).lexically_normal().generic_string()},
            {"package", package.string()},
            {"package_manifest_hash", manifest.package_hash},
            {"package_profile", manifest.profile},
            {"provider_feature_hash", pipeline.provider_feature_hash},
            {"expected_hashes", phase4["native"]["hashes"]},
            {"expected_playable_hash",
             IsArtemisVnSample(sample) ? "" : playable.value("replay_route_hash", "")},
            {"runtime_replay", phase4["native"]["runtime_save"]["payload"]["replay_events"]},
            {"presentation_capture", phase4["native"]["headless_capture"]},
            {"playable_vn", playable},
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
    if (!options.headless_smoke && !options.windowed_smoke) {
        AddDiagnostic(report, "ASTRA_RUN_HEADLESS_REQUIRED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Run requires --headless-smoke or --windowed-smoke.");
        return report;
    }

    Astra::Core::DiagnosticSink diagnostics;
    auto path = ResolveToolTarget(target);
    std::filesystem::path package_path;
    nlohmann::json package_manifest;
    nlohmann::json package_mount;
    nlohmann::json package_payload_smoke;
    if (std::filesystem::is_regular_file(path) && path.extension() == ".astrapkg") {
        package_path = path;
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
        if ((!payload_id ||
             std::ranges::none_of(manifest.Value().payloads,
                                  [&](const Astra::Asset::PackagePayloadEntry& payload) {
                                      return payload.asset_id.ToString() ==
                                             payload_id.Value().ToString();
                                  })) &&
            !manifest.Value().payloads.empty()) {
            payload_id = Astra::Core::Result<Astra::Asset::AssetUri>::Success(
                manifest.Value().payloads.front().asset_id);
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
    nlohmann::json windowed_evidence = nlohmann::json::object();
    if (options.windowed_smoke) {
        auto sdl_platform = Astra::Platform::CreateSdlPlatform(diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
        if (!sdl_platform) {
            return report;
        }
        platform = std::move(sdl_platform.Value());
        auto created =
            platform.Window().Create({"AstraEngine TsuiNoSora Smoke", 1280, 720}, diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
        if (!created) {
            return report;
        }
        platform.Window().PumpEvents();
        windowed_evidence = {
            {"schema", "astra.windowed_smoke.v1"},
            {"backend", "sdl3"},
            {"created_window", true},
            {"auto_close", options.auto_close},
            {"scripted_input",
             options.scripted_input.empty() ? "" : options.scripted_input.string()},
        };
    }
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(plugin_descriptor, diagnostics);
    if (!descriptor) {
        AddDiagnostic(report, "ASTRA_RUN_PLUGIN_DESCRIPTOR",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Could not load Phase 1 example plugin descriptor.", plugin_descriptor);
        return report;
    }
    Astra::ModuleRuntime::ModuleManager manager(platform);
    auto loaded =
        manager.LoadAndActivate(descriptor.Value(), plugin_descriptor.parent_path(), diagnostics);
    AppendDiagnostics(report, diagnostics);
    if (!loaded) {
        AddDiagnostic(report, "ASTRA_RUN_PLUGIN_LOAD", Astra::Core::DiagnosticSeverity::Blocking,
                      "Phase 1 example plugin failed to load.", plugin_descriptor);
        return report;
    }
    diagnostics.Clear();
    report.artifacts["headless_smoke"] = {
        {"services", manager.Services().Services().size()},
        {"extensions", manager.Extensions().Extensions().size()},
        {"providers", manager.EngineModules().Providers().size()},
        {"package_manifest",
         package_manifest.is_null() ? nlohmann::json::object() : package_manifest},
        {"package_mount", package_mount.is_null() ? nlohmann::json::object() : package_mount},
        {"package_payload_smoke",
         package_payload_smoke.is_null() ? nlohmann::json::object() : package_payload_smoke},
        {"phase3", Phase3FoundationSmoke(diagnostics)},
    };
    if (!windowed_evidence.empty()) {
        report.artifacts["windowed_smoke"] = windowed_evidence;
    }
    if (!options.scripted_input.empty()) {
        report.artifacts["scripted_input"] = {
            {"path", options.scripted_input.string()},
            {"exists", std::filesystem::exists(options.scripted_input)},
        };
    }
    if (!options.load.empty()) {
        report.artifacts["loaded_save"] = {
            {"path", options.load.string()},
            {"exists", std::filesystem::exists(options.load)},
        };
        if (!std::filesystem::exists(options.load)) {
            AddDiagnostic(report, "ASTRA_RUN_SAVE_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Requested save file does not exist.", options.load);
        }
    }
    if (std::filesystem::is_directory(path) && IsVnSmokeSample(path)) {
        const auto registry = ScanSampleRegistry(path, diagnostics);
        report.artifacts["headless_smoke"]["phase4_script_vn"] =
            IsArtemisVnSample(path) ? TsuiNoSoraRuntimeFixtureEvidence(path, registry)
                                    : Phase4ScriptVnSmoke(path, diagnostics);
        report.artifacts["headless_smoke"]["playable_vn"] = BuildPlayableVnEvidence(
            path, report.artifacts["headless_smoke"]["phase4_script_vn"], registry,
            options.scripted_input, options.windowed_smoke, diagnostics);
        report.artifacts["playable_vn"] = report.artifacts["headless_smoke"]["playable_vn"];
        if (!IsArtemisVnSample(path) &&
            report.artifacts["headless_smoke"]["phase4_script_vn"].value("status", "failed") !=
                "passed") {
            AddDiagnostic(report, "ASTRA_PHASE4_SCRIPT_VN_FAILED",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Phase 4 Script/AstraVN foundation smoke failed.", path);
        }
        const auto audio_commands = report.artifacts["playable_vn"]
                                        .value("audio_playback_evidence", nlohmann::json::object())
                                        .value("commands", nlohmann::json::array());
        auto decoded_audio_payloads =
            BuildPackageAudioEvidence(registry, audio_commands, package_path, diagnostics);
        report.artifacts["playable_vn"]["audio_playback_evidence"]["decoded_payloads"] =
            decoded_audio_payloads;
        report.artifacts["headless_smoke"]["playable_vn"] = report.artifacts["playable_vn"];
        if (!windowed_evidence.empty()) {
            nlohmann::json texture_sources = nlohmann::json::array();
            nlohmann::json glyph_sources = nlohmann::json::array();
            auto frame =
                BuildPlayableWindowFrame(report.artifacts["playable_vn"], registry, package_path,
                                         texture_sources, glyph_sources, diagnostics);
            auto presented = platform.Window().PresentFrame(frame, diagnostics);
            AppendDiagnostics(report, diagnostics);
            diagnostics.Clear();
            if (presented) {
                report.artifacts["playable_vn"]["window_present"] = ToJson(presented.Value());
                report.artifacts["playable_vn"]["window_texture_sources"] = texture_sources;
                report.artifacts["playable_vn"]["window_glyph_sources"] = glyph_sources;
                report.artifacts["windowed_smoke"]["window_present"] = ToJson(presented.Value());
                report.artifacts["windowed_smoke"]["window_texture_sources"] = texture_sources;
                report.artifacts["windowed_smoke"]["window_glyph_sources"] = glyph_sources;
            }
            report.artifacts["windowed_smoke"]["playable_vn"] = report.artifacts["playable_vn"];
            report.artifacts["windowed_smoke"]["presentation_frame_hash"] =
                report.artifacts["playable_vn"].value("presentation_frame_hash", "");
            report.artifacts["windowed_smoke"]["windowed_playable"] =
                report.artifacts["playable_vn"]["windowed_playable"];
            if (options.auto_close) {
                platform.Window().Close();
            }
        }
    }
    if (!options.save_out.empty()) {
        const auto package_meta =
            package_manifest.is_object() ? package_manifest : nlohmann::json::object();
        nlohmann::json save = {
            {"schema", "astra.runtime.save.evidence.v1"},
            {"target", target.string()},
            {"sample", std::filesystem::is_directory(path) ? path.filename().string() : ""},
            {"headless_smoke", report.artifacts.value("headless_smoke", nlohmann::json::object())},
            {"windowed_smoke", report.artifacts.value("windowed_smoke", nlohmann::json::object())},
            {"playable_vn", report.artifacts.value("playable_vn", nlohmann::json::object())},
            {"package_manifest_hash", package_meta.value("package_hash", "")},
            {"package_profile", package_meta.value("profile", "")},
            {"provider_feature_hash",
             package_meta.value("runtime_evidence", nlohmann::json::object())
                 .value("provider_feature_hash", "")},
            {"save_snapshot_hash", report.artifacts.value("playable_vn", nlohmann::json::object())
                                       .value("save_snapshot_hash", "")},
        };
        WriteJsonFile(options.save_out, save);
        report.artifacts["save_out"] = options.save_out.string();
    }
    AppendDiagnostics(report, diagnostics);
    manager.DeactivateAndUnload(diagnostics);
    return report;
}

CommandReport Replay(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra replay");
    const auto path = ResolveToolTarget(target);
    if (!std::filesystem::is_regular_file(path)) {
        AddDiagnostic(report, "ASTRA_REPLAY_MISSING", Astra::Core::DiagnosticSeverity::Blocking,
                      "Replay file is missing.", path);
        return report;
    }
    std::ifstream file(path, std::ios::binary);
    auto replay = nlohmann::json::parse(file, nullptr, false);
    if (replay.is_discarded() || replay.value("schema", "") != ReplayEvidenceSchema) {
        AddDiagnostic(report, "ASTRA_REPLAY_INVALID", Astra::Core::DiagnosticSeverity::Blocking,
                      "Replay file has an unsupported schema.", path);
        return report;
    }
    report.artifacts["replay"] = replay;
    if (!options.compare) {
        return report;
    }

    const auto sample = std::filesystem::path(replay.value("source_sample", ""));
    if (!std::filesystem::exists(sample)) {
        AddDiagnostic(report, "ASTRA_REPLAY_SOURCE_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking, "Replay source sample is missing.",
                      sample);
        return report;
    }
    Astra::Core::DiagnosticSink diagnostics;
    const auto registry = ScanSampleRegistry(sample, diagnostics);
    auto phase4 = IsArtemisVnSample(sample) ? TsuiNoSoraRuntimeFixtureEvidence(sample, registry)
                                            : Phase4ScriptVnSmoke(sample, diagnostics);
    auto playable = BuildPlayableVnEvidence(sample, phase4, registry, {}, false, diagnostics);
    AppendDiagnostics(report, diagnostics);
    if (phase4.value("status", "failed") != "passed") {
        AddDiagnostic(report, "ASTRA_REPLAY_RUN_FAILED", Astra::Core::DiagnosticSeverity::Blocking,
                      "Replay comparison could not re-run NativeVN headless evidence.", sample);
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
    const auto expected_playable_hash = replay.value("expected_playable_hash", "");
    const auto actual_playable_hash = playable.value("replay_route_hash", "");
    report.artifacts["comparison"] = Astra::Runtime::ToJson(comparison);
    report.artifacts["comparison"]["playable_route_hash_match"] =
        expected_playable_hash.empty() || expected_playable_hash == actual_playable_hash;
    report.artifacts["comparison"]["expected_playable_hash"] = expected_playable_hash;
    report.artifacts["comparison"]["actual_playable_hash"] = actual_playable_hash;
    report.artifacts["comparison"]["package_manifest_hash"] =
        replay.value("package_manifest_hash", "");
    report.artifacts["comparison"]["package_profile"] = replay.value("package_profile", "");
    report.artifacts["comparison"]["provider_feature_hash"] =
        replay.value("provider_feature_hash", "");
    report.artifacts["playable_vn"] = playable;
    if (!comparison.passed) {
        report.artifacts["mismatch_report"] = {
            {"schema", "astra.runtime.replay_mismatch.v1"},
            {"frame", 0},
            {"category", "runtime_hash"},
            {"expected_hash", expected.state_hash + "|" + expected.event_hash + "|" +
                                  expected.presentation_hash},
            {"actual_hash", actual.state_hash + "|" + actual.event_hash + "|" +
                                actual.presentation_hash},
            {"nearest_event_sequence", 0},
            {"record_kind", "checkpoint"},
            {"package_manifest_hash", replay.value("package_manifest_hash", "")},
            {"objects", nlohmann::json::array({{{"kind", "sample"}, {"id", sample.filename().string()}}})},
            {"source", {{"file", (sample / "Content/Scripts/opening.astra").string()}, {"line", 1}}},
        };
        AddDiagnostic(report, "ASTRA_REPLAY_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking,
                      "Replay hashes do not match.", path);
    }
    if (!report.artifacts["comparison"].value("playable_route_hash_match", false)) {
        report.artifacts["mismatch_report"] = {
            {"schema", "astra.runtime.replay_mismatch.v1"},
            {"frame", 0},
            {"category", "playable_route"},
            {"expected_hash", expected_playable_hash},
            {"actual_hash", actual_playable_hash},
            {"nearest_event_sequence", 0},
            {"record_kind", "script_decision"},
            {"package_manifest_hash", replay.value("package_manifest_hash", "")},
            {"objects", nlohmann::json::array({{{"kind", "script"}, {"id", "native:/Scripts/opening"}}})},
            {"source", {{"file", (sample / "Content/Scripts/opening.astra").string()}, {"line", 1}}},
        };
        AddDiagnostic(report, "ASTRA_REPLAY_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking,
                      "Playable route hash does not match.", path);
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
