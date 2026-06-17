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

} // namespace Astra::Tools
