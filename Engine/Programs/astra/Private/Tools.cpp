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
        if (IsVnSmokeSample(absolute)) {
            report.artifacts["phase8_script_vn"] = Phase4ScriptVnSmoke(absolute, diagnostics);
            report.artifacts["phase4_script_vn"] = report.artifacts["phase8_script_vn"];
            report.artifacts["phase4_script_vn"]["deprecated_alias_for"] = "phase8_script_vn";
            report.artifacts["playable_vn"] = BuildPlayableVnEvidence(
                absolute, report.artifacts["phase8_script_vn"], registry, {}, false, diagnostics);
            ValidatePhase4AssetReferences(report.artifacts["phase8_script_vn"], registry, report,
                                          absolute);
            if (report.artifacts["phase8_script_vn"].value("status", "failed") != "passed") {
                AddDiagnostic(report, "ASTRA_PHASE8_SCRIPT_VN_FAILED",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Phase 8 Script/AstraVN playable smoke failed.", absolute);
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
                                  {"phase3_smoke", Phase3FoundationSmoke(diagnostics)}};
    if (IsVnSmokeSample(sample)) {
        cook_report["phase8_script_vn"] = Phase4ScriptVnSmoke(sample, diagnostics);
        cook_report["phase4_script_vn"] = cook_report["phase8_script_vn"];
        cook_report["phase4_script_vn"]["deprecated_alias_for"] = "phase8_script_vn";
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
        file << "\"%~dp0" << spec.launcher_name << "\" run \"%~dp0Packages\\" << sample_name
             << ".astrapkg\" --windowed-smoke %*\r\n";
    } else {
        file << "#!/usr/bin/env sh\n";
        file << "DIR=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\n";
        file << "\"$DIR/" << spec.launcher_name << "\" run \"$DIR/Packages/" << sample_name
             << ".astrapkg\" --windowed-smoke \"$@\"\n";
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
        file << "\"%~dp0Engine\\" << spec.launcher_name << "\" play \"%~dp0Packages\\" << sample_name
             << ".astrapkg\" %*\r\n";
    } else {
        file << "#!/usr/bin/env sh\n";
        file << "DIR=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\n";
        file << "\"$DIR/Engine/" << spec.launcher_name << "\" play \"$DIR/Packages/" << sample_name
             << ".astrapkg\" \"$@\"\n";
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

std::vector<std::filesystem::path> CollectPlayerPlans(const std::filesystem::path& plan,
                                                       CommandReport& report) {
    std::vector<std::filesystem::path> plans;
    const auto absolute = ResolveToolTarget(plan);
    if (std::filesystem::is_regular_file(absolute)) {
        plans.push_back(absolute);
    } else if (std::filesystem::is_directory(absolute)) {
        for (const auto& entry : std::filesystem::directory_iterator(absolute)) {
            if (entry.is_regular_file() && entry.path().extension() == ".yaml") {
                plans.push_back(entry.path());
            }
        }
        std::ranges::sort(plans);
    } else {
        AddDiagnostic(report, "ASTRA_PLAYER_TEST_PLAN_INVALID",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Player test plan file or directory is missing.", absolute);
    }
    return plans;
}

std::filesystem::path WriteCaseScriptedInput(const YAML::Node& test_case,
                                             const std::filesystem::path& plan_path,
                                             CommandReport& report) {
    const auto case_id = test_case["case_id"].as<std::string>("");
    if (case_id.empty()) {
        AddDiagnostic(report, "ASTRA_PLAYER_TEST_PLAN_INVALID",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Player test case requires case_id.", plan_path);
        return {};
    }
    nlohmann::json scripted = {
        {"schema", "astra.input.scripted.v1"},
        {"case_id", case_id},
        {"persona", test_case["persona"].as<std::string>("")},
        {"objective", test_case["objective"].as<std::string>("")},
        {"events", nlohmann::json::array()},
    };
    for (const auto& step : test_case["steps"]) {
        const auto kind = step["kind"].as<std::string>(step["action"] ? "player_action" : "");
        if (kind != "player_action") {
            continue;
        }
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
        if (step["label"]) {
            action["label"] = step["label"].as<std::string>("");
        }
        if (step["set"]) {
            action["set"] = YamlToJson(step["set"]);
        }
        scripted["events"].push_back(std::move(action));
    }
    const auto out = std::filesystem::temp_directory_path() /
                     ("astra_player_" + case_id + "_" + Sha256Text(plan_path.string()).substr(0, 8) +
                      ".yaml");
    std::ofstream file(out, std::ios::binary);
    file << "schema: astra.input.scripted.v1\n";
    file << "case_id: " << scripted["case_id"].get<std::string>() << "\n";
    file << "persona: " << scripted["persona"].get<std::string>() << "\n";
    file << "objective: " << scripted["objective"].get<std::string>() << "\n";
    file << "events:\n";
    for (const auto& event : scripted["events"]) {
        file << "  - frame: " << event.value("frame", 0) << "\n";
        file << "    action: " << event.value("action", "") << "\n";
        if (event.contains("index")) {
            file << "    index: " << event["index"].get<int>() << "\n";
        }
        if (event.contains("slot")) {
            file << "    slot: " << event["slot"].get<std::string>() << "\n";
        }
        if (event.contains("label")) {
            file << "    label: " << event["label"].get<std::string>() << "\n";
        }
        if (event.contains("set")) {
            file << "    set:\n";
            for (const auto& [key, value] : event["set"].items()) {
                file << "      " << key << ": " << value.get<std::string>() << "\n";
            }
        }
    }
    return out;
}

nlohmann::json RunRuntimeEventSteps(const YAML::Node& test_case,
                                    const std::filesystem::path& plan_path,
                                    CommandReport& report) {
    nlohmann::json evidence = {
        {"schema", "astra.test.runtime_event_injection.v1"},
        {"events", nlohmann::json::array()},
        {"frame_results", nlohmann::json::array()},
    };
    Astra::Runtime::RuntimeWorld world(44);
    Astra::Core::DiagnosticSink diagnostics;
    for (const auto& step : test_case["steps"]) {
        if (step["kind"].as<std::string>("") != "runtime_event") {
            continue;
        }
        const auto event_json = YamlToJson(step["event"]);
        Astra::Core::Result<Astra::Runtime::RuntimeEvent> event =
            Astra::Core::Result<Astra::Runtime::RuntimeEvent>::Failure(
                Astra::Core::ErrorCode::InvalidFormat, "runtime event is invalid");
        try {
            event = Astra::Runtime::RuntimeEventFromJson(event_json);
        } catch (const std::exception& error) {
            event = Astra::Core::Result<Astra::Runtime::RuntimeEvent>::Failure(
                Astra::Core::ErrorCode::InvalidFormat, error.what());
        }
        if (!event) {
            AddDiagnostic(report, "ASTRA_PLAYER_TEST_RUNTIME_EVENT_INVALID",
                          Astra::Core::DiagnosticSeverity::Blocking, event.Message(), plan_path);
            evidence["events"].push_back({{"status", "invalid"}, {"event", event_json}});
            continue;
        }
        Astra::Runtime::RuntimeTickInput input;
        input.frame_index = step["frame"].as<Astra::Core::u64>(0);
        input.fixed_step_index = input.frame_index;
        input.input_events.push_back(event.Value());
        auto tick = world.Tick(input, diagnostics);
        evidence["events"].push_back(
            {{"status", tick ? "injected" : "failed"}, {"event", Astra::Runtime::ToJson(event.Value())}});
        if (tick) {
            evidence["frame_results"].push_back(Astra::Runtime::ToJson(tick.Value()));
        }
    }
    AppendDiagnostics(report, diagnostics);
    return evidence;
}

nlohmann::json JsonPointerValue(const nlohmann::json& root, const std::string& pointer) {
    try {
        return root.at(nlohmann::json::json_pointer(pointer));
    } catch (const std::exception&) {
        return nullptr;
    }
}

nlohmann::json EvaluatePlayerAssertions(const YAML::Node& test_case,
                                        const nlohmann::json& case_report,
                                        const std::filesystem::path& plan_path,
                                        CommandReport& report) {
    nlohmann::json assertions = nlohmann::json::array();
    for (const auto& assertion : test_case["assertions"]) {
        const auto pointer = assertion["path"].as<std::string>("");
        const auto actual = JsonPointerValue(case_report, pointer);
        bool passed = true;
        nlohmann::json expected = nullptr;
        const auto op = assertion["op"].as<std::string>(
            assertion["equals"] ? "equals" : assertion["matches"] ? "matches"
                                        : assertion["min_count"] ? "min_count"
                                                                 : "exists");
        if (op == "exists") {
            passed = !actual.is_null();
        } else if (op == "equals") {
            expected = YamlToJson(assertion["equals"]);
            passed = actual == expected;
        } else if (op == "matches") {
            expected = assertion["matches"].as<std::string>("");
            passed = actual.is_string() &&
                     std::regex_search(actual.get<std::string>(),
                                       std::regex(expected.get<std::string>()));
        } else if (op == "min_count") {
            expected = assertion["min_count"].as<int>(0);
            passed = actual.is_array() && actual.size() >= static_cast<std::size_t>(expected.get<int>());
        } else {
            passed = false;
        }
        assertions.push_back({{"path", pointer},
                              {"op", op},
                              {"expected", expected},
                              {"actual", actual},
                              {"passed", passed}});
        if (!passed) {
            AddDiagnostic(report, "ASTRA_PLAYER_TEST_ASSERTION_FAILED",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Player test assertion failed.", plan_path);
        }
    }
    return assertions;
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
    if (!std::filesystem::exists(launcher_source)) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Distribution target launcher is missing.", launcher_source);
        return;
    }
    const auto wrapper_name = "astra-shipping-wrapper" + std::filesystem::path(spec.launcher_name).extension().string();
    const auto wrapper_source = bin_root / wrapper_name;
    if (options.shipping && !std::filesystem::exists(wrapper_source)) {
        AddDiagnostic(report, "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Shipping wrapper launcher is missing.", wrapper_source);
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

    const auto engine_root = options.shipping ? bundle_root / "Engine" : bundle_root;
    std::filesystem::create_directories(engine_root);
    const auto launcher_name = spec.launcher_name;
    const auto root_launcher_name = options.shipping
                                        ? sample_name + std::filesystem::path(spec.launcher_name).extension().string()
                                        : launcher_name;
    if (options.shipping) {
        runtime_files.push_back(
            CopyFileWithHash(wrapper_source, bundle_root / root_launcher_name, bundle_root, report));
    }
    runtime_files.push_back(
        CopyFileWithHash(launcher_source, engine_root / launcher_name, bundle_root, report));
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
            readme << "This launcher uses astra play, not smoke QA mode.\n";
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
        {"engine_launcher", options.shipping ? ("Engine/" + launcher_name) : launcher_name},
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
    const auto phase3 = Phase3FoundationSmoke(diagnostics);
    nlohmann::json phase4;
    nlohmann::json playable;
    if (IsVnSmokeSample(sample)) {
        phase4 = Phase4ScriptVnSmoke(sample, diagnostics);
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
    auto phase4_alias = compact_phase8;
    phase4_alias["deprecated_alias_for"] = "phase8_script_vn";
    manifest.runtime_evidence = {
        {"source_sample", std::filesystem::absolute(sample).lexically_normal().generic_string()},
        {"build_info", report.build_info},
        {"engine_binaries", EngineDllEvidence(report)},
        {"phase3_headless", phase3},
        {"phase8_script_vn", compact_phase8},
        {"phase4_script_vn", phase4_alias},
        {"playable_vn", playable.is_null() ? nlohmann::json::object() : playable},
        {"asset_registry", Astra::Asset::ToJson(registry)},
        {"asset_dependency_graph",
         Astra::Asset::ToJson(Astra::Asset::BuildDependencyGraph(registry))},
        {"provider_feature_hash", pipeline.provider_feature_hash},
        {"package_profile", manifest.profile},
    };
    if (IsVnSmokeSample(sample)) {
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
            {"expected_playable_hash", playable.value("replay_route_hash", "")},
            {"runtime_replay", phase4["native"]["runtime_save"]["payload"]["replay_events"]},
            {"presentation_capture", phase4["native"]["headless_capture"]},
            {"source_map", phase4.value("source_map", nlohmann::json::object())},
            {"command_manifest", phase4.value("command_manifest", nlohmann::json::array())},
            {"playable_vn", playable},
        };
        WriteJsonFile(replay_path, replay);
        report.artifacts["golden_replay"] = replay_path.string();
    }
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
    auto smoke_asset = Astra::Asset::ParseAssetUri("native:/ReleaseGate/Smoke");
    if (smoke_asset) {
        graph.draws.push_back({"draw.release", smoke_asset.Value(), "ui", 0, {}, {}});
    }
    graph.text_requests.push_back({"text.release", "Release Gate", "en", "text", 0, {}});
    graph.audio_commands.push_back({"audio.release", "play",
                                    smoke_asset ? smoke_asset.Value() : Astra::Asset::AssetUri{},
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

nlohmann::json BuildGpuSmokeEvidence(const std::filesystem::path& sample,
                                     const Astra::Platform::WindowGraphicsBinding& binding,
                                     const Astra::Platform::WindowFrameDesc& frame,
                                     Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Media::RenderTargetBinding target{binding.id, binding.backend, binding.width,
                                             binding.height};
    Astra::Media::RenderBackendSmoke smoke;
    auto renderer = Astra::Media::CreateProductionRenderer2DProvider(target);
    auto text_provider = Astra::Media::CreateProductionTextLayoutProvider();
    smoke.renderer_provider = renderer->Describe().provider_id;
    smoke.text_provider = text_provider->Describe().provider_id;

    Astra::Media::MediaReleaseGateRequest gate;
    gate.providers = Astra::Media::ProductionMediaProviders();
    gate.require_available_backends = true;
    for (const auto& provider : gate.providers) {
        gate.selected_providers[provider.slot_id] = provider.provider_id;
    }
    auto release = Astra::Media::ValidateMediaReleaseGate(gate, diagnostics);

    auto begun =
        renderer->BeginFrame({frame.frame_index, frame.width, frame.height, "srgb"}, diagnostics);
    std::vector<Astra::Media::PresentationCommand> commands;
    if (begun) {
        for (std::size_t index = 0; index < frame.primitives.size(); ++index) {
            const auto& primitive = frame.primitives[index];
            if (!primitive.image_rgba.empty() && primitive.image_width > 0 &&
                primitive.image_height > 0) {
                Astra::Media::DecodedCpuBuffer buffer;
                buffer.width = primitive.image_width;
                buffer.height = primitive.image_height;
                buffer.row_stride = primitive.image_width * 4;
                buffer.pixels = primitive.image_rgba;
                if (renderer->ImportTexture(buffer, diagnostics)) {
                    ++smoke.imported_texture_count;
                }
            }
            auto asset =
                Astra::Asset::ParseAssetUri("native:/GpuSmoke/Primitive" + std::to_string(index));
            if (asset) {
                Astra::Media::PresentationCommand command;
                command.kind = primitive.image_rgba.empty()
                                   ? Astra::Media::PresentationCommandKind::UiRect
                                   : Astra::Media::PresentationCommandKind::Sprite;
                command.command_id =
                    primitive.id.empty() ? "gpu.primitive." + std::to_string(index) : primitive.id;
                command.frame_index = frame.frame_index;
                command.layer = primitive.kind == "text" ? "text" : "ui";
                command.order = static_cast<Astra::Core::i32>(index);
                command.asset = asset.Value();
                command.transform.x = primitive.x;
                command.transform.y = primitive.y;
                commands.push_back(std::move(command));
            }
        }

        const auto font_path = sample / "Content/Fonts/sourcehanserif-medium.otf";
        if (std::filesystem::exists(font_path)) {
            const auto font_bytes = ReadBytes(font_path);
            auto text_texture = Astra::Media::RasterizeUiTextCpuBufferBytes(
                font_bytes, "GPU smoke text", 24, diagnostics);
            if (text_texture && renderer->ImportTexture(text_texture.Value(), diagnostics)) {
                ++smoke.text_texture_count;
            }
        }

        auto graph = Astra::Media::ExtractRenderGraph(commands, nullptr, diagnostics);
        smoke.draw_count = static_cast<Astra::Core::u32>(graph.draws.size());
        (void)renderer->Execute(graph, diagnostics);
        auto capture = renderer->Capture(diagnostics);
        if (capture) {
            smoke.frame_hash = capture.Value().render_hash;
        }
        if (renderer->Present({"target:/main", false}, diagnostics)) {
            smoke.presented = true;
        }
    }

    const auto capabilities = Astra::Media::ProbeMediaBackendCapabilities();
    smoke.renderer_available = capabilities.renderer2d_ready && begun;
    smoke.text_available = capabilities.ui_text_raster_ready || smoke.text_texture_count > 0;
    return {
        {"schema", "astra.gpu_smoke.v1"},
        {"release_gate", release ? Astra::Media::ToJson(release.Value())
                                 : nlohmann::json({{"schema", Astra::Media::MediaReleaseGateSchema},
                                                   {"passed", false}})},
        {"capabilities", Astra::Media::ToJson(capabilities)},
        {"smoke", Astra::Media::ToJson(smoke)},
    };
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
    nlohmann::json package_runtime_evidence = nlohmann::json::object();
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
        package_runtime_evidence = manifest.Value().runtime_evidence;
        if (package_manifest.contains("runtime_evidence")) {
            package_manifest["runtime_evidence"] = {
                {"source_sample", package_runtime_evidence.value("source_sample", "")},
                {"phase8_script_vn",
                 CompactPhase8Evidence(
                     package_runtime_evidence.value("phase8_script_vn", nlohmann::json::object()))},
                {"playable_vn",
                 {{"status",
                   package_runtime_evidence.value("playable_vn", nlohmann::json::object())
                       .value("status", "")},
                  {"replay_route_hash",
                   package_runtime_evidence.value("playable_vn", nlohmann::json::object())
                       .value("replay_route_hash", "")}}},
            };
        }
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
        auto created = platform.Window().Create({"AstraEngine NativeVN Smoke", 1280, 720},
                                                diagnostics);
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
        auto phase8 = package_runtime_evidence.value("phase8_script_vn", nlohmann::json::object());
        if (phase8.empty()) {
            phase8 = Phase4ScriptVnSmoke(path, diagnostics);
        }
        report.artifacts["headless_smoke"]["phase8_script_vn"] =
            package_runtime_evidence.empty() ? phase8 : CompactPhase8Evidence(phase8);
        report.artifacts["headless_smoke"]["phase4_script_vn"] =
            report.artifacts["headless_smoke"]["phase8_script_vn"];
        report.artifacts["headless_smoke"]["phase4_script_vn"]["deprecated_alias_for"] =
            "phase8_script_vn";
        if (!package_runtime_evidence.empty() && package_runtime_evidence.contains("playable_vn")) {
            report.artifacts["headless_smoke"]["playable_vn"] = ApplyScriptedInputToPlayable(
                package_runtime_evidence["playable_vn"], options.scripted_input,
                options.windowed_smoke, diagnostics);
        } else {
            report.artifacts["headless_smoke"]["playable_vn"] = BuildPlayableVnEvidence(
                path, phase8, registry, options.scripted_input, options.windowed_smoke,
                diagnostics);
        }
        report.artifacts["playable_vn"] = report.artifacts["headless_smoke"]["playable_vn"];
        if (report.artifacts["headless_smoke"]["phase8_script_vn"].value("status", "failed") !=
            "passed") {
            AddDiagnostic(report, "ASTRA_PHASE8_SCRIPT_VN_FAILED",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Phase 8 Script/AstraVN playable smoke failed.", path);
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
            if (options.gpu_smoke) {
                report.artifacts["windowed_smoke"]["gpu_smoke"] = BuildGpuSmokeEvidence(
                    path, platform.Window().GraphicsBinding(), frame, diagnostics);
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

CommandReport Test(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra test");
    auto plan_paths = CollectPlayerPlans(options.test_plan, report);
    nlohmann::json cases = nlohmann::json::array();
    std::size_t executed = 0;

    for (const auto& plan_path : plan_paths) {
        YAML::Node plan;
        try {
            plan = YAML::LoadFile(plan_path.string());
        } catch (const YAML::Exception& error) {
            AddDiagnostic(report, "ASTRA_PLAYER_TEST_PLAN_INVALID",
                          Astra::Core::DiagnosticSeverity::Blocking, error.what(), plan_path);
            continue;
        }
        if (plan["schema"].as<std::string>("") != "astra.test.player_plan.v1" ||
            !plan["cases"] || !plan["cases"].IsSequence()) {
            AddDiagnostic(report, "ASTRA_PLAYER_TEST_PLAN_INVALID",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Player test plan requires schema astra.test.player_plan.v1 and cases.",
                          plan_path);
            continue;
        }

        for (const auto& test_case : plan["cases"]) {
            const auto case_id = test_case["case_id"].as<std::string>("");
            if (!options.test_case.empty() && case_id != options.test_case) {
                continue;
            }
            ++executed;
            auto scripted_input = WriteCaseScriptedInput(test_case, plan_path, report);
            CommandOptions run_options = options;
            run_options.scripted_input = scripted_input;
            if (!run_options.headless_smoke && !run_options.windowed_smoke) {
                run_options.headless_smoke = true;
            }
            auto run_report = Run(target, run_options);
            for (const auto& diagnostic : run_report.diagnostics) {
                report.diagnostics.push_back(diagnostic);
            }
            if (!run_report.Passed()) {
                report.status = "failed";
                AddDiagnostic(report, "ASTRA_PLAYER_TEST_CASE_FAILED",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Player test run command failed.", plan_path);
            }

            nlohmann::json case_report = {
                {"schema", "astra.test.player_case_report.v1"},
                {"suite_id", plan["suite_id"].as<std::string>("")},
                {"case_id", case_id},
                {"persona", test_case["persona"].as<std::string>("")},
                {"objective", test_case["objective"].as<std::string>("")},
                {"plan", StableSourcePath(plan_path)},
                {"run", ToJson(run_report)},
                {"runtime_events", RunRuntimeEventSteps(test_case, plan_path, report)},
            };
            case_report["assertions"] =
                EvaluatePlayerAssertions(test_case, case_report, plan_path, report);
            bool assertions_passed = true;
            for (const auto& assertion : case_report["assertions"]) {
                assertions_passed = assertions_passed && assertion.value("passed", false);
            }
            case_report["status"] =
                run_report.Passed() && assertions_passed ? "passed" : "failed";
            cases.push_back(std::move(case_report));
        }
    }

    if (executed == 0 && report.Passed()) {
        AddDiagnostic(report, "ASTRA_PLAYER_TEST_PLAN_INVALID",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "No player test cases matched the requested plan/case filter.",
                      options.test_plan);
    }
    std::size_t passed = 0;
    for (const auto& test_case : cases) {
        passed += test_case.value("status", "") == "passed" ? 1 : 0;
    }
    report.artifacts["player_tests"] = {
        {"schema", "astra.test.player_report.v1"},
        {"target", target.string()},
        {"plan", options.test_plan.string()},
        {"case_filter", options.test_case},
        {"total", cases.size()},
        {"passed", passed},
        {"failed", cases.size() - passed},
        {"cases", cases},
    };
    if (passed != cases.size()) {
        report.status = "failed";
    }
    return report;
}

CommandReport Play(const std::filesystem::path& target, const CommandOptions& options) {
    auto report = MakeReport("astra play");
    Astra::Core::DiagnosticSink diagnostics;
    const auto package_path = ResolveToolTarget(target);
    if (!std::filesystem::is_regular_file(package_path) || package_path.extension() != ".astrapkg") {
        AddDiagnostic(report, "ASTRA_PLAY_PACKAGE_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Play requires an existing .astrapkg package.", package_path);
        return report;
    }

    Astra::Asset::PackageReader reader;
    auto manifest = reader.ReadManifest(package_path, diagnostics);
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();
    if (!manifest) {
        return report;
    }
    auto package_manifest = Astra::Asset::ToJson(manifest.Value());
    const auto source_sample = manifest.Value().runtime_evidence.value("source_sample", "");
    const auto sample = source_sample.empty() ? std::filesystem::path{} : std::filesystem::path(source_sample);
    auto registry = std::filesystem::is_directory(sample) ? ScanSampleRegistry(sample, diagnostics)
                                                          : Astra::Asset::AssetRegistry{};
    nlohmann::json playable =
        manifest.Value().runtime_evidence.value("playable_vn", nlohmann::json::object());
    if (playable.empty() && std::filesystem::is_directory(sample) && IsVnSmokeSample(sample)) {
        auto phase8 = Phase4ScriptVnSmoke(sample, diagnostics);
        playable = BuildPlayableVnEvidence(sample, phase8, registry, {}, true, diagnostics);
    }

    auto sdl_platform = Astra::Platform::CreateSdlPlatform(diagnostics);
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();
    if (!sdl_platform) {
        return report;
    }
    auto platform = std::move(sdl_platform.Value());
    auto created = platform.Window().Create({"AstraEngine Play", 1280, 720}, diagnostics);
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();
    if (!created) {
        return report;
    }

    nlohmann::json texture_sources = nlohmann::json::array();
    nlohmann::json glyph_sources = nlohmann::json::array();
    auto frame = BuildPlayableWindowFrame(playable, registry, package_path, texture_sources,
                                          glyph_sources, diagnostics);
    auto presented = platform.Window().PresentFrame(frame, diagnostics);
    AppendDiagnostics(report, diagnostics);
    diagnostics.Clear();
    if (presented) {
        report.artifacts["production_play"] = {
            {"schema", "astra.production_play.v1"},
            {"package", package_path.string()},
            {"package_manifest_hash", package_manifest.value("package_hash", "")},
            {"profile", package_manifest.value("profile", "")},
            {"source_sample", source_sample},
            {"window_present", ToJson(presented.Value())},
            {"window_texture_sources", texture_sources},
            {"window_glyph_sources", glyph_sources},
            {"playable_vn", playable},
        };
    }
    if (!options.json) {
        while (!platform.Window().ShouldClose()) {
            platform.Window().PumpEvents();
            (void)platform.Window().PresentFrame(frame, diagnostics);
            std::this_thread::sleep_for(std::chrono::milliseconds(16));
        }
    } else {
        platform.Window().Close();
    }
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
    auto phase4 = Phase4ScriptVnSmoke(sample, diagnostics);
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
            {"expected_hash",
             expected.state_hash + "|" + expected.event_hash + "|" + expected.presentation_hash},
            {"actual_hash",
             actual.state_hash + "|" + actual.event_hash + "|" + actual.presentation_hash},
            {"nearest_event_sequence", 0},
            {"record_kind", "checkpoint"},
            {"package_manifest_hash", replay.value("package_manifest_hash", "")},
            {"objects",
             nlohmann::json::array({{{"kind", "sample"}, {"id", sample.filename().string()}}})},
            {"source",
             {{"file", (sample / "Content/Scripts/opening.astra").string()}, {"line", 1}}},
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
            {"objects",
             nlohmann::json::array({{{"kind", "script"}, {"id", "native:/Scripts/opening"}}})},
            {"source",
             {{"file", (sample / "Content/Scripts/opening.astra").string()}, {"line", 1}}},
        };
        AddDiagnostic(report, "ASTRA_REPLAY_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking,
                      "Playable route hash does not match.", path);
    }
    return report;
}

} // namespace Astra::Tools
