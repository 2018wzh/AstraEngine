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
    Astra::Core::LogDiagnostic(diagnostic, "tools.diagnostics", report.command);
    report.diagnostics.push_back(std::move(diagnostic));
    if (severity == Astra::Core::DiagnosticSeverity::Blocking ||
        severity == Astra::Core::DiagnosticSeverity::Fatal) {
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
    const auto sample_id = descriptor["id"].as<std::string>("");
    const auto phase = descriptor["phase"].as<int>(0);
    const auto native_vn_phase8 = sample_id == "astra.sample.nativevn" && phase == 8;
    if ((descriptor["foundation_only"].as<bool>(false) != true || phase > 6) &&
        !native_vn_phase8) {
        AddDiagnostic(report, "ASTRA_SAMPLE_NOT_FOUNDATION",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Foundation tools accept Phase 1-6 foundation/evidence sample descriptors plus the Phase 8 NativeVN playable demo.",
                      descriptor_path);
        return false;
    }
    return true;
}

YAML::Node TryLoadSampleDescriptor(const std::filesystem::path& sample) {
    const auto descriptor_path = SampleDescriptor(sample);
    if (!std::filesystem::exists(descriptor_path)) {
        return {};
    }
    try {
        return YAML::LoadFile(descriptor_path.string());
    } catch (const YAML::Exception&) {
        return {};
    }
}

std::string SampleDescriptorString(const std::filesystem::path& sample, std::string_view key) {
    const auto descriptor = TryLoadSampleDescriptor(sample);
    if (!descriptor || !descriptor[std::string(key)]) {
        return {};
    }
    return descriptor[std::string(key)].as<std::string>("");
}

bool IsNativeVnSample(const std::filesystem::path& sample) {
    const auto id = SampleDescriptorString(sample, "id");
    return id == "astra.sample.nativevn" ||
           (id.empty() && sample.filename().string() == "NativeVN");
}

bool IsVnSmokeSample(const std::filesystem::path& sample) {
    return IsNativeVnSample(sample);
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

nlohmann::json Phase7MediaBackendEvidence(Astra::Core::DiagnosticSink& diagnostics) {
    auto providers = Astra::Media::ProductionMediaProviders();
    Astra::Media::MediaReleaseGateRequest request;
    request.providers = providers;
    request.filter_profile = MakePhase3FoundationFilterProfile();
    for (const auto& provider : providers) {
        request.selected_providers[provider.slot_id] = provider.provider_id;
    }
    auto release = Astra::Media::ValidateMediaReleaseGate(request, diagnostics);

    std::vector<Astra::Media::PresentationCommand> commands;
    auto background = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room");
    if (background) {
        Astra::Media::PresentationCommand draw;
        draw.kind = Astra::Media::PresentationCommandKind::Sprite;
        draw.command_id = "phase7.draw.background";
        draw.frame_index = 7;
        draw.layer = "background";
        draw.asset = background.Value();
        commands.push_back(std::move(draw));
    }
    auto graph = Astra::Media::ExtractRenderGraph(commands, &request.filter_profile.value(), diagnostics);
    auto renderer = Astra::Media::CreateHeadlessRenderer2DProvider();
    (void)renderer->BeginFrame({7, 1280, 720, "srgb"}, diagnostics);
    (void)renderer->Execute(graph, diagnostics);
    auto capture = renderer->Capture(diagnostics);
    const auto filter_execution = Astra::Media::ExecuteFilterGraphHeadless(request.filter_profile.value(), graph);

    const nlohmann::json timeline_json = {
        {"schema", "astra.media.timeline.v1"},
        {"id", "native:/Timelines/Opening"},
        {"duration_seconds", 3.0},
        {"tracks", nlohmann::json::array({
            {{"id", "camera.main"}, {"type", "camera"}, {"keys", nlohmann::json::array({
                {{"t", 0.0}, {"value", {{"x", 0}, {"y", 0}, {"zoom", 1.0}}}},
                {{"t", 1.0}, {"value", {{"x", 24}, {"y", 0}, {"zoom", 1.05}}}, {"easing", "ease_out"}}
            })}},
            {{"id", "audio.bgm"}, {"type", "audio"}, {"audio_events", nlohmann::json::array({
                {{"command_id", "timeline.bgm.opening"}, {"asset", "native:/Music/opening_theme"}, {"bus", "music"}, {"volume", 0.8}, {"loop", true}}
            })}}
        })},
    };
    auto timeline = Astra::Media::TimelineFromJson(timeline_json, diagnostics);

    return {
        {"schema", "astra.media.phase7_backend_evidence.v1"},
        {"release_gate", release ? Astra::Media::ToJson(release.Value()) : nlohmann::json({{"schema", Astra::Media::MediaReleaseGateSchema}, {"passed", false}})},
        {"renderer_capture", capture ? Astra::Media::ToJson(capture.Value()) : nlohmann::json::object()},
        {"filter_graph", Astra::Media::ToJson(filter_execution)},
        {"timeline_state", timeline ? Astra::Media::ToJson(Astra::Media::EvaluateTimeline(timeline.Value(), 1500000000ull)) : nlohmann::json::object()},
        {"providers", providers.size()},
    };
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
        Astra::Core::LogDiagnostic(diagnostic, "tools.diagnostics", report.command);
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
        {"phase7_media_backend", Phase7MediaBackendEvidence(diagnostics)},
    };
}
#include "ValidationEvidence.inc"

Astra::Asset::AssetRegistry ScanSampleRegistry(const std::filesystem::path& sample,
                                               Astra::Core::DiagnosticSink& diagnostics) {
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

#include "PlayableEvidence.inc"

CommandReport MakeReport(std::string command) {
    CommandReport report;
    report.command = std::move(command);
    report.build_info = BuildInfoJson();
    Astra::Core::DefaultLogger().Log(
        "tools.lifecycle",
        report.command,
        Astra::Core::LogLevel::Info,
        "command started",
        {{"command", report.command}});
    return report;
}

} // namespace


