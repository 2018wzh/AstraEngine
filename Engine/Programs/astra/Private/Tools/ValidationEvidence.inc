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


