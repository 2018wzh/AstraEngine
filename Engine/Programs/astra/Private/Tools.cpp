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
#include <Astra/Tools/Tools.hpp>
#include <algorithm>
#include <array>
#include <cctype>
#include <chrono>
#include <cstdlib>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <openssl/evp.h>
#include <optional>
#include <regex>
#include <set>
#include <sstream>
#include <thread>
#include <vector>
#include <yaml-cpp/yaml.h>

namespace Astra::Tools {

#include "Tools/Evidence.inc"

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
    Astra::Core::DefaultLogger().Log("tools.lifecycle", report.command,
                                     report.Passed() ? Astra::Core::LogLevel::Info
                                                     : Astra::Core::LogLevel::Error,
                                     "command finished",
                                     {{"command", report.command},
                                      {"status", report.status},
                                      {"diagnostics", std::to_string(report.diagnostics.size())}});
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
        report.artifacts["phase7_media_backend"] = Phase7MediaBackendEvidence(diagnostics);
        if (IsVnValidationSample(absolute)) {
            report.artifacts["phase8_script_vn"] = Phase8ScriptVnValidation(absolute, diagnostics);
            report.artifacts["playable_vn"] = BuildPlayableVnEvidence(
                absolute, report.artifacts["phase8_script_vn"], registry, {}, false, diagnostics);
            ValidatePhase4AssetReferences(report.artifacts["phase8_script_vn"], registry, report,
                                          absolute);
            if (report.artifacts["phase8_script_vn"].value("status", "failed") != "passed") {
                AddDiagnostic(report, "ASTRA_PHASE8_SCRIPT_VN_FAILED",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Phase 8 Script/AstraVN playable launch failed.", absolute);
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
                    report.artifacts["payload_read"] = {
                        {"asset_id", manifest.Value().payloads.front().asset_id.ToString()},
                        {"size_bytes", payload.Value().size()},
                        {"hash", manifest.Value().payloads.front().hash},
                        {"encoding", manifest.Value().payloads.front().encoding},
                        {"compression", manifest.Value().payloads.front().compression},
                    };
                }
            }
            if (manifest.Value().runtime_evidence.contains("playable_vn")) {
                report.artifacts["playable_vn"] = manifest.Value().runtime_evidence["playable_vn"];
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
                        report.artifacts["payload_read"] = {
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
            }
        }
    } else {
        AddDiagnostic(
            report, "ASTRA_INSPECT_TARGET_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking,
            "Foundation inspect supports plugin YAML and foundation JSON reports only.", absolute);
    }
    return report;
}

CommandReport Import(const std::filesystem::path& project, const std::filesystem::path& source,
                     const CommandOptions& options) {
    auto report = MakeReport("astra import");
    const auto project_path = ResolveToolTarget(project);
    const auto source_path = ResolveToolTarget(source);
    Astra::Core::DiagnosticSink diagnostics;
    auto asset_id = Astra::Asset::ParseAssetUri(options.import_asset_id);
    if (!asset_id) {
        AddDiagnostic(report, "ASTRA_IMPORT_TARGET_INVALID",
                      Astra::Core::DiagnosticSeverity::Blocking, asset_id.Message(), source_path);
        return report;
    }
    const auto extension = source_path.extension().string();
    std::string provider_id = "astra.importer.text";
    if (extension == ".png" || extension == ".jpg" || extension == ".jpeg" ||
        extension == ".webp") {
        provider_id = "astra.importer.image";
    } else if (extension == ".ogg" || extension == ".wav" || extension == ".flac" ||
               extension == ".mp3") {
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
                                  {"phase3_validation", Phase3FoundationValidation(diagnostics)}};
    if (IsVnValidationSample(sample)) {
        cook_report["phase8_script_vn"] = Phase8ScriptVnValidation(sample, diagnostics);
        cook_report["playable_vn"] = BuildPlayableVnEvidence(
            sample, cook_report["phase8_script_vn"], registry, {}, false, diagnostics);
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
            {"save_restore", cook_report["phase8_script_vn"].value("status", "failed") == "passed"},
        };
        ValidatePhase4AssetReferences(cook_report["phase8_script_vn"], registry, report, sample);
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
    pipeline.provider_feature_hash =
        Astra::Asset::ComputeProviderFeatureHash(pipeline.selected_providers);
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
        ddc_status["rebuilt"] =
            ddc_status["rebuilt"].get<Astra::Core::u64>() + (entry.rebuilt ? 1 : 0);
        ddc_status["reused"] =
            ddc_status["reused"].get<Astra::Core::u64>() + (!entry.rebuilt ? 1 : 0);
        ddc_status["corruption_recovered"] =
            ddc_status["corruption_recovered"].get<Astra::Core::u64>() +
            (entry.corruption_recovered ? 1 : 0);
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

std::string LowerAscii(std::string value) {
    for (auto& c : value) {
        c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
    return value;
}

bool IsPlatformDynamicLibrary(const std::filesystem::path& path,
                              const Astra::Platform::TargetPlatformDesc& spec) {
    const auto name = path.filename().string();
    if (spec.id == "linux-x64") {
        return name.find(".so") != std::string::npos;
    }
    return path.extension().string() == spec.dynamic_library_extension;
}

nlohmann::json CopyFileWithHash(const std::filesystem::path& source,
                                const std::filesystem::path& destination,
                                const std::filesystem::path& bundle_root, CommandReport& report) {
    if (!std::filesystem::exists(source)) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Distribution source file is missing.", source);
        return nlohmann::json::object();
    }
    std::filesystem::create_directories(destination.parent_path());
    std::filesystem::copy_file(source, destination,
                               std::filesystem::copy_options::overwrite_existing);
    return {
        {"path", destination.lexically_relative(bundle_root).generic_string()},
        {"sha256", Sha256File(destination)},
        {"size_bytes", std::filesystem::file_size(destination)},
    };
}

void WriteDistributionScript(const std::filesystem::path& script,
                             const Astra::Platform::TargetPlatformDesc& spec, const std::string& sample_name) {
    std::filesystem::create_directories(script.parent_path());
    std::ofstream file(script, std::ios::binary);
    if (spec.id == "win64") {
        file << "@echo off\r\n";
        file << "\"%~dp0" << sample_name << std::filesystem::path(spec.launcher_name).extension().string()
             << "\" %*\r\n";
    } else {
        file << "#!/usr/bin/env sh\n";
        file << "DIR=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\n";
        file << "\"$DIR/" << sample_name << "\" \"$@\"\n";
    }
#if !defined(_WIN32)
    if (spec.id != "win64") {
        std::filesystem::permissions(script,
                                     std::filesystem::perms::owner_exec |
                                         std::filesystem::perms::group_exec |
                                         std::filesystem::perms::others_exec,
                                     std::filesystem::perm_options::add);
    }
#endif
}

void WriteShippingPlayScript(const std::filesystem::path& script,
                             const Astra::Platform::TargetPlatformDesc& spec, const std::string& sample_name) {
    std::filesystem::create_directories(script.parent_path());
    std::ofstream file(script, std::ios::binary);
    if (spec.id == "win64") {
        file << "@echo off\r\n";
        file << "\"%~dp0" << sample_name << std::filesystem::path(spec.launcher_name).extension().string()
             << "\" %*\r\n";
    } else {
        file << "#!/usr/bin/env sh\n";
        file << "DIR=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\n";
        file << "\"$DIR/" << sample_name << "\" \"$@\"\n";
    }
}

nlohmann::json JsonAtDottedPath(const nlohmann::json& json, const std::string& path) {
    const nlohmann::json* current = &json;
    std::stringstream stream(path);
    std::string part;
    while (std::getline(stream, part, '.')) {
        if (!current->is_object() || !current->contains(part)) {
            return nullptr;
        }
        current = &(*current)[part];
    }
    return *current;
}

nlohmann::json YamlScalarToJson(const YAML::Node& node) {
    if (!node) {
        return nullptr;
    }
    const auto text = node.as<std::string>("");
    if (text == "true") {
        return true;
    }
    if (text == "false") {
        return false;
    }
    return text;
}

nlohmann::json YamlToJson(const YAML::Node& node) {
    if (!node) {
        return nullptr;
    }
    if (node.IsScalar()) {
        return YamlScalarToJson(node);
    }
    if (node.IsSequence()) {
        nlohmann::json array = nlohmann::json::array();
        for (const auto& item : node) {
            array.push_back(YamlToJson(item));
        }
        return array;
    }
    if (node.IsMap()) {
        nlohmann::json object = nlohmann::json::object();
        for (const auto& item : node) {
            object[item.first.as<std::string>("")] = YamlToJson(item.second);
        }
        return object;
    }
    return nullptr;
}

nlohmann::json EvaluateShippingGate(const std::filesystem::path& sample) {
    nlohmann::json checks = nlohmann::json::array();
    bool passed = true;
    const auto descriptor_path = SampleDescriptor(sample);
    YAML::Node descriptor;
    try {
        descriptor = YAML::LoadFile(descriptor_path.string());
    } catch (const std::exception&) {
        return {{"schema", "astra.shipping.gate.v1"}, {"passed", true}, {"checks", checks}};
    }
    const auto required = descriptor["shipping"]["required_json"];
    for (const auto& item : required) {
        const auto file = sample / item["file"].as<std::string>("");
        const auto path = item["path"].as<std::string>("");
        const auto expected = YamlScalarToJson(item["equals"]);
        nlohmann::json actual = nullptr;
        bool check_passed = false;
        if (std::filesystem::exists(file)) {
            try {
                actual = JsonAtDottedPath(nlohmann::json::parse(ReadText(file)), path);
                check_passed = actual == expected;
            } catch (const std::exception&) {
                actual = nullptr;
            }
        }
        checks.push_back({{"file", file.lexically_relative(sample).generic_string()},
                          {"path", path},
                          {"expected", expected},
                          {"actual", actual},
                          {"passed", check_passed}});
        passed = passed && check_passed;
    }
    return {{"schema", "astra.shipping.gate.v1"}, {"passed", passed}, {"checks", checks}};
}

void BuildDistributionBundle(const std::filesystem::path& sample,
                             const std::filesystem::path& package,
                             const Astra::Asset::PackageManifest& manifest,
                             const CommandOptions& options, CommandReport& report) {
    if (options.no_distribution) {
        return;
    }

    const auto sample_name = sample.filename().string();
    const auto platform_id = options.target_platform.empty() ? Astra::Platform::CurrentHostTargetPlatform().id : options.target_platform;
    const auto spec_result = Astra::Platform::FindTargetPlatform(platform_id);
    if (!spec_result) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_PLATFORM_UNSUPPORTED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Unsupported distribution target platform.", platform_id);
        return;
    }
    const auto spec = spec_result.Value();

    const auto bin_root = BinaryRoot() / "Bin";
    const auto launcher_source = bin_root / spec.launcher_name;
    if (!options.shipping && !std::filesystem::exists(launcher_source)) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Distribution target launcher is missing.", launcher_source);
        return;
    }
    const auto wrapper_name = "astra-shipping-wrapper" + std::filesystem::path(spec.launcher_name).extension().string();
    const auto wrapper_source = bin_root / wrapper_name;
    if (!std::filesystem::exists(wrapper_source)) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "AstraGame launcher is missing.", wrapper_source);
        return;
    }

    bool has_dynamic_library = false;
    for (const auto& entry : std::filesystem::directory_iterator(bin_root)) {
        if (entry.is_regular_file() && IsPlatformDynamicLibrary(entry.path(), spec)) {
            has_dynamic_library = true;
            break;
        }
    }
    if (!has_dynamic_library) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Distribution target dynamic libraries are missing.", bin_root);
        return;
    }

    const auto release_root = options.distribution_root.empty()
                                  ? BinaryRoot() / (options.shipping ? "Saved/Shipping"
                                                                      : "Saved/Releases")
                                  : options.distribution_root;
    const auto bundle_root = options.shipping ? release_root / sample_name / spec.id
                                              : release_root / sample_name / (sample_name + "-" + spec.id);
    std::filesystem::remove_all(bundle_root);
    std::filesystem::create_directories(bundle_root);

    nlohmann::json runtime_files = nlohmann::json::array();
    nlohmann::json plugin_files = nlohmann::json::array();

    const auto engine_root = bundle_root;
    std::filesystem::create_directories(engine_root);
    const auto launcher_name = spec.launcher_name;
    const auto root_launcher_name =
        sample_name + std::filesystem::path(spec.launcher_name).extension().string();
    if (options.shipping) {
        runtime_files.push_back(
            CopyFileWithHash(wrapper_source, bundle_root / root_launcher_name, bundle_root, report));
    }
    if (!options.shipping) {
        runtime_files.push_back(
            CopyFileWithHash(wrapper_source, bundle_root / root_launcher_name, bundle_root, report));
        runtime_files.push_back(
            CopyFileWithHash(launcher_source, engine_root / launcher_name, bundle_root, report));
    }
    for (const auto& entry : std::filesystem::directory_iterator(bin_root)) {
        if (entry.is_regular_file() && IsPlatformDynamicLibrary(entry.path(), spec)) {
            runtime_files.push_back(CopyFileWithHash(
                entry.path(), engine_root / entry.path().filename(), bundle_root, report));
        }
    }

    const auto package_destination = bundle_root / "Packages" / package.filename();
    runtime_files.push_back(CopyFileWithHash(package, package_destination, bundle_root, report));

    for (const auto& module : manifest.modules) {
        if (!module.runtime_safe) {
            continue;
        }
        const std::filesystem::path descriptor = module.binary;
        const auto plugin_root = descriptor.parent_path();
        if (!std::filesystem::exists(descriptor)) {
            AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Packaged plugin descriptor is missing.", descriptor);
            return;
        }
        const auto plugin_dest_root = engine_root / "Plugins" / plugin_root.filename();
        plugin_files.push_back(CopyFileWithHash(
            descriptor, plugin_dest_root / descriptor.filename(), bundle_root, report));

        Astra::Core::DiagnosticSink plugin_diagnostics;
        auto descriptor_data =
            Astra::ModuleRuntime::LoadPluginDescriptor(descriptor, plugin_diagnostics);
        if (!descriptor_data) {
            AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Packaged plugin descriptor could not be read.", descriptor);
            return;
        }
        bool copied_plugin_binary = false;
        for (const auto& plugin_module : descriptor_data.Value().modules) {
            if (!plugin_module.packaged || plugin_module.id != module.id) {
                continue;
            }
            const auto plugin_binary = plugin_root / plugin_module.entrypoint;
            if (!std::filesystem::exists(plugin_binary)) {
                AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Packaged plugin target binary is missing.", plugin_binary);
                return;
            }
            plugin_files.push_back(CopyFileWithHash(
                plugin_binary, plugin_dest_root / plugin_module.entrypoint, bundle_root, report));
            copied_plugin_binary = true;
        }
        if (!copied_plugin_binary) {
            AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Packaged plugin target library is missing.", plugin_root);
            return;
        }
    }

    std::filesystem::path script_path;
    if (!options.shipping) {
        const auto script_name =
            spec.id == "win64"
                ? spec.script_name_prefix + sample_name + spec.script_extension
                : spec.script_name_prefix + LowerAscii(sample_name) + spec.script_extension;
        script_path = bundle_root / script_name;
        WriteDistributionScript(script_path, spec, sample_name);
        runtime_files.push_back({{"path", script_path.lexically_relative(bundle_root).generic_string()},
                                 {"sha256", Sha256File(script_path)},
                                 {"size_bytes", std::filesystem::file_size(script_path)}});
    }
    if (options.shipping) {
        std::filesystem::create_directories(bundle_root / "Saved/Config");
        std::filesystem::create_directories(bundle_root / "Saved/Logs");
        const auto readme_path = bundle_root / "README.txt";
        {
            std::ofstream readme(readme_path, std::ios::binary);
            readme << sample_name << " shipping package\n\n";
            readme << "Run " << root_launcher_name << " to play.\n";
            readme << "This launcher uses AstraGame directly.\n";
        }
        runtime_files.push_back({{"path", "README.txt"},
                                 {"sha256", Sha256File(readme_path)},
                                 {"size_bytes", std::filesystem::file_size(readme_path)}});
        if (options.allow_unsigned_shipping) {
            const auto marker_path = bundle_root / "UNSIGNED_NOT_PRODUCTION.txt";
            {
                std::ofstream marker(marker_path, std::ios::binary);
                marker << "This package was built with --allow-unsigned-shipping.\n";
                marker << "It is for internal playtest only and is not production-signed.\n";
            }
            runtime_files.push_back({{"path", "UNSIGNED_NOT_PRODUCTION.txt"},
                                     {"sha256", Sha256File(marker_path)},
                                     {"size_bytes", std::filesystem::file_size(marker_path)}});
        }
    }

    if (!report.Passed()) {
        return;
    }

    nlohmann::json distribution_manifest = {
        {"schema", options.shipping ? "astra.shipping.manifest.v1" : "astra.distribution.manifest.v1"},
        {"sample", sample_name},
        {"platform", spec.id},
        {"profile", manifest.profile},
        {"shipping", options.shipping},
        {"unsigned_override", options.shipping && options.allow_unsigned_shipping},
        {"shipping_gate", report.artifacts.value("shipping_gate", nlohmann::json::object())},
        {"package",
         {{"path", ("Packages/" + package.filename().generic_string())},
          {"manifest_hash", manifest.package_hash},
          {"file_sha256", Sha256File(package_destination)}}},
        {"launcher", root_launcher_name},
        {"engine_launcher", options.shipping ? root_launcher_name : launcher_name},
        {"runtime_files", runtime_files},
        {"plugin_files", plugin_files},
        {"build_info", report.build_info},
        {"release_report", report.artifacts.value("release_report", nlohmann::json::object())},
    };
    const auto manifest_path = bundle_root / (options.shipping ? "shipping-manifest.json" : "distribution-manifest.json");
    WriteJsonFile(manifest_path, distribution_manifest);

    nlohmann::json all_files = runtime_files;
    for (const auto& file : plugin_files) {
        all_files.push_back(file);
    }
    all_files.push_back({{"path", manifest_path.filename().generic_string()},
                         {"sha256", Sha256File(manifest_path)},
                         {"size_bytes", std::filesystem::file_size(manifest_path)}});
    if (options.shipping) {
        const auto checksums_path = bundle_root / "checksums.sha256";
        {
            std::ofstream checksums(checksums_path, std::ios::binary);
            for (const auto& file : all_files) {
                checksums << file.value("sha256", "") << "  " << file.value("path", "") << "\n";
            }
        }
        all_files.push_back({{"path", "checksums.sha256"},
                             {"sha256", Sha256File(checksums_path)},
                             {"size_bytes", std::filesystem::file_size(checksums_path)}});
    }

    report.artifacts["distribution_bundle"] = bundle_root.string();
    report.artifacts["distribution_manifest"] = manifest_path.string();
    report.artifacts["distribution_platform"] = spec.id;
    report.artifacts["distribution_files"] = all_files;
    if (options.shipping) {
        report.artifacts["shipping_bundle"] = bundle_root.string();
        report.artifacts["shipping_manifest"] = manifest_path.string();
        report.artifacts["shipping_files"] = all_files;
    }
}

CommandReport Package(const std::filesystem::path& sample, const CommandOptions& options) {
    auto report = MakeReport("astra package");
    if (!IsFoundationSample(sample, report)) {
        return report;
    }
    if (options.shipping && options.no_distribution) {
        AddDiagnostic(report, "ASTRA_SHIPPING_REQUIRES_DISTRIBUTION",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Shipping package requires a distribution folder.", sample);
        return report;
    }
    if (options.shipping) {
        report.artifacts["shipping_gate"] = EvaluateShippingGate(sample);
        if (!report.artifacts["shipping_gate"].value("passed", false)) {
            if (!options.allow_unsigned_shipping) {
                AddDiagnostic(report, "ASTRA_SHIPPING_GATE_INCOMPLETE",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Sample shipping gate is incomplete.", SampleDescriptor(sample));
                return report;
            }
            AddDiagnostic(report, "ASTRA_SHIPPING_UNSIGNED_OVERRIDE",
                          Astra::Core::DiagnosticSeverity::Warning,
                          "Shipping gate is incomplete; unsigned shipping override is enabled.",
                          SampleDescriptor(sample));
        }
    }
    const auto package = PackagePathForSample(sample);
    const auto plugin_descriptor = BinaryRoot() / "Plugins/Phase1Example/Phase1Example.plugin.yaml";
    Astra::Core::DiagnosticSink diagnostics;
    const auto registry = ScanSampleRegistry(sample, diagnostics);
    const auto phase3 = Phase3FoundationValidation(diagnostics);
    nlohmann::json phase4;
    nlohmann::json playable;
    if (IsVnValidationSample(sample)) {
        phase4 = Phase8ScriptVnValidation(sample, diagnostics);
        playable = BuildPlayableVnEvidence(sample, phase4, registry, {}, false, diagnostics);
    }

    Astra::Asset::CookPipelineOptions pipeline;
    pipeline.project_id = "package:/" + sample.filename().string();
    pipeline.profile = (options.compare || options.shipping) ? "deterministic" : options.profile;
    pipeline.content_root = sample / "Content";
    pipeline.cooked_root = CookManifestPathForSample(sample).parent_path() / "Artifacts";
    pipeline.ddc_root = DdcRootForSample(sample);
    pipeline.selected_providers = {
        {"astra.asset.import", "astra.importer.builtin"},
        {"astra.asset.cook", "astra.cook.builtin"},
    };
    pipeline.provider_feature_hash =
        Astra::Asset::ComputeProviderFeatureHash(pipeline.selected_providers);
    auto cooked = Astra::Asset::CookAssetRegistry(registry, pipeline, diagnostics);
    Astra::Asset::CookManifest cook_manifest;
    if (cooked) {
        cook_manifest = cooked.Value();
    }

    Astra::Asset::PackageManifest manifest;
    manifest.package_id = "package:/" + sample.filename().string();
    manifest.profile = (options.compare || options.shipping) ? "deterministic" : options.profile;
    manifest.project_hash = Sha256Text(ReadText(SampleDescriptor(sample)));
    manifest.cook_manifest = cook_manifest;
    auto phase8_evidence = phase4.is_null() ? nlohmann::json::object() : phase4;
    auto compact_phase8 = CompactPhase8Evidence(phase8_evidence);
    manifest.runtime_evidence = {
        {"source_sample", std::filesystem::absolute(sample).lexically_normal().generic_string()},
        {"build_info", report.build_info},
        {"engine_binaries", EngineDllEvidence(report)},
        {"phase3_headless", phase3},
        {"phase8_script_vn", compact_phase8},
        {"playable_vn", playable.is_null() ? nlohmann::json::object() : playable},
        {"asset_registry", Astra::Asset::ToJson(registry)},
        {"asset_dependency_graph",
         Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry))},
        {"provider_feature_hash", pipeline.provider_feature_hash},
        {"package_profile", manifest.profile},
    };
    if (IsVnValidationSample(sample)) {
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

    AppendDiagnostics(report, diagnostics);
    report.artifacts["package"] = package.string();
    report.artifacts["package_manifest"] = package_json;
    if (manifest.profile == "deterministic") {
        CommandOptions gate_options = options;
        gate_options.profile = manifest.profile;
        auto gate = ReleaseGate(sample, gate_options);
        report.artifacts["release_report"] =
            gate.artifacts.value("release_report", nlohmann::json::object());
        for (const auto& diagnostic : gate.diagnostics) {
            report.diagnostics.push_back(diagnostic);
        }
        if (!gate.Passed()) {
            report.status = "failed";
        }
    }
    if (report.Passed()) {
        BuildDistributionBundle(sample, package, manifest, options, report);
    }
    return report;
}

CommandReport ReleaseGate(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra release-gate");
    const auto absolute = ResolveToolTarget(target);
    const auto profile = options.profile.empty() ? "deterministic" : options.profile;
    const bool deterministic = profile == "deterministic";
    std::filesystem::path sample = absolute;
    std::filesystem::path package_path;
    Astra::Asset::PackageManifest manifest;
    Astra::Core::DiagnosticSink diagnostics;

    if (std::filesystem::is_regular_file(absolute) && absolute.extension() == ".astrapkg") {
        package_path = absolute;
        Astra::Asset::PackageReader reader;
        auto read = reader.ReadManifest(absolute, diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
        if (!read) {
            return report;
        }
        manifest = read.Value();
        const auto source_sample = manifest.runtime_evidence.value("source_sample", "");
        if (!source_sample.empty()) {
            sample = source_sample;
        }
    } else if (std::filesystem::is_directory(absolute) && IsFoundationSample(absolute, report)) {
        package_path = PackagePathForSample(absolute);
        if (std::filesystem::exists(package_path)) {
            Astra::Asset::PackageReader reader;
            auto read = reader.ReadManifest(package_path, diagnostics);
            AppendDiagnostics(report, diagnostics);
            diagnostics.Clear();
            if (read) {
                manifest = read.Value();
            }
        }
    } else {
        AddDiagnostic(report, "ASTRA_RELEASE_TARGET_UNSUPPORTED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Release gate expects a sample directory or .astrapkg package.", absolute);
        return report;
    }

    if (package_path.empty() || !std::filesystem::exists(package_path)) {
        AddDiagnostic(
            report, "ASTRA_RELEASE_PACKAGE_MISSING", Astra::Core::DiagnosticSeverity::Blocking,
            "Release gate requires an existing packaged .astrapkg artifact.", package_path);
    }

    auto registry = std::filesystem::is_directory(sample) ? ScanSampleRegistry(sample, diagnostics)
                                                          : Astra::Asset::AssetRegistry{};
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();

    Astra::Asset::AssetReleaseGateReport asset_gate;
    if (!manifest.package_id.empty()) {
        Astra::Asset::AssetReleaseGateRequest asset_request;
        asset_request.registry = registry;
        asset_request.cook_manifest = manifest.cook_manifest;
        asset_request.package_manifest = manifest;
        asset_request.profile = profile;
        asset_gate = Astra::Asset::ValidateAssetReleaseGate(asset_request, diagnostics);
        AppendDiagnostics(report, diagnostics);
        diagnostics.Clear();
    }

    Astra::Media::MediaReleaseGateRequest media_request;
    media_request.providers = Astra::Media::ProductionMediaProviders();
    media_request.require_available_backends = deterministic;
    for (const auto& provider : media_request.providers) {
        media_request.selected_providers[provider.slot_id] = provider.provider_id;
    }
    auto media_gate = Astra::Media::ValidateMediaReleaseGate(media_request, diagnostics);
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();

    Astra::Media::RenderGraph graph;
    graph.frame_index = 1;
    graph.layers = {"background", "character", "ui", "text", "final"};
    auto launch_asset = Astra::Asset::ParseAssetUri("native:/ReleaseGate/Launch");
    if (launch_asset) {
        graph.draws.push_back({"draw.release", launch_asset.Value(), "ui", 0, {}, {}});
    }
    graph.text_requests.push_back({"text.release", "Release Gate", "en", "text", 0, {}});
    graph.audio_commands.push_back({"audio.release", "play",
                                    launch_asset ? launch_asset.Value() : Astra::Asset::AssetUri{},
                                    "sfx", 1.0, false});

    auto headless = Astra::Media::CreateHeadlessRenderer2DProvider();
    (void)headless->BeginFrame({1, 64, 64, "srgb"}, diagnostics);
    (void)headless->Execute(graph, diagnostics);
    auto headless_capture = headless->Capture(diagnostics);
    auto production = Astra::Media::CreateProductionRenderer2DProvider();
    (void)production->BeginFrame({1, 64, 64, "srgb"}, diagnostics);
    (void)production->Execute(graph, diagnostics);
    auto production_capture = production->Capture(diagnostics);
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();

    auto capabilities = Astra::Media::ProbeMediaBackendCapabilities();
    auto driver_diff = (headless_capture && production_capture)
                           ? Astra::Media::CompareDriverCaptures(headless_capture.Value(),
                                                                 production_capture.Value(),
                                                                 capabilities, deterministic)
                           : Astra::Media::DriverDiffReport{};
    if (deterministic && !driver_diff.passed) {
        AddDiagnostic(report, "ASTRA_RELEASE_DRIVER_DIFF_FAILED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Deterministic release requires driver diff hardening evidence.", sample);
    }

    const bool editor_module_present =
        std::ranges::any_of(manifest.modules, [](const auto& module) {
            return module.id.find("editor") != std::string::npos ||
                   module.binary.find("Editor") != std::string::npos;
        });
    if (editor_module_present) {
        AddDiagnostic(report, "ASTRA_RELEASE_EDITOR_MODULE_INCLUDED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Packaged runtime includes an Editor module.", package_path);
    }

    nlohmann::json trace = nlohmann::json::array({
        {{"schema", "astra.trace.event.v1"},
         {"frame_index", 1},
         {"channel", "runtime.tick"},
         {"name", "release_gate_tick"},
         {"duration_ns", 0},
         {"objects", nlohmann::json::array()},
         {"fields", nlohmann::json::object()}},
        {{"schema", "astra.trace.event.v1"},
         {"frame_index", 1},
         {"channel", "media.render"},
         {"name", "driver_diff"},
         {"duration_ns", 0},
         {"objects", nlohmann::json::array()},
         {"fields", {{"profile", profile}}}},
    });
    nlohmann::json crash_bundle = {
        {"schema", "astra.crash.bundle.v1"},
        {"build_info", report.build_info},
        {"package_or_project_hash", manifest.package_hash},
        {"frame_index", 1},
        {"thread_id", "release-gate"},
        {"last_runtime_hashes", nlohmann::json::object()},
        {"recent_diagnostics", nlohmann::json::array()},
        {"recent_logs", nlohmann::json::array()},
        {"last_events", nlohmann::json::array()},
        {"provider_states", nlohmann::json::array()},
        {"minidump_path", ""},
    };

    nlohmann::json diagnostics_json = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics_json.push_back(Astra::Core::ToJson(diagnostic));
    }
    const auto passed = !std::ranges::any_of(report.diagnostics, [](const auto& diagnostic) {
        return diagnostic.severity == Astra::Core::DiagnosticSeverity::Blocking ||
               diagnostic.severity == Astra::Core::DiagnosticSeverity::Fatal;
    });
    report.artifacts["release_report"] = {
        {"schema", "astra.release.report.v1"},
        {"target", absolute.string()},
        {"profile", profile},
        {"passed", passed},
        {"reports", {{"package", package_path.string()}}},
        {"diagnostics", diagnostics_json},
        {"provider_policy_hash", manifest.runtime_evidence.value("provider_feature_hash", "")},
        {"package_manifest_hash", manifest.package_hash},
        {"asset_release_gate", Astra::Asset::ToJson(asset_gate)},
        {"media_release_gate",
         media_gate
             ? Astra::Media::ToJson(media_gate.Value())
             : nlohmann::json{{"schema", Astra::Media::MediaReleaseGateSchema}, {"passed", false}}},
        {"media_backend_capabilities", Astra::Media::ToJson(capabilities)},
        {"driver_diff", Astra::Media::ToJson(driver_diff)},
        {"trace_events", trace},
        {"crash_bundle", crash_bundle},
        {"blocking_policy",
         {{"schema", "astra.release.blocking_policy.v1"},
          {"profile", profile},
          {"deny_runtime_ai_provider", deterministic},
          {"require_provider_binary_hash", deterministic},
          {"require_save_migration", deterministic},
          {"allow_media_fallback", !deterministic}}},
    };
    if (!passed) {
        report.status = "failed";
    }
    return report;
}

} // namespace Astra::Tools


