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



TEST_CASE("Core logging supports structured memory and rotating file sinks") {
    Astra::Core::LogEvent event;
    event.monotonic_ns = 1;
    event.sequence = 1;
    event.channel = "test";
    event.component = "core-test";
    event.level = Astra::Core::LogLevel::Debug;
    event.message = "hello";
    event.fields["key"] = "value";

    Astra::Core::MemoryLogSink memory;
    memory.Write(event);
    REQUIRE(memory.Events().size() == 1);
    REQUIRE(Astra::Core::ToJson(memory.Events()[0])["fields"]["key"] == "value");
    REQUIRE(Astra::Core::ToJson(memory.Events()[0])["schema"] == "astra.log.event.v1");

    const auto log_path = std::filesystem::temp_directory_path() / "astra_phase1_log.jsonl";
    const auto rotated_path = std::filesystem::path(log_path.string() + ".1");
    std::filesystem::remove(log_path);
    std::filesystem::remove(rotated_path);
    Astra::Core::FileLogSink file(log_path.string(), 1);
    file.Write(event);
    file.Write(event);
    REQUIRE(std::filesystem::exists(rotated_path));

    Astra::Core::ResetLoggingForTests();
    const auto configured_log = std::filesystem::temp_directory_path() / "astra_configured_log.jsonl";
    std::filesystem::remove(configured_log);
    Astra::Core::LogConfig config;
    config.log_file = configured_log;
    config.async = false;
    config.console_enabled = false;
    config.file_enabled = true;
    config.capture_memory = true;
    config.file_level = Astra::Core::LogLevel::Trace;
    Astra::Core::ConfigureLogging(config);
    Astra::Core::DefaultLogger().Write(event);
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_TEST_LOG_DIAGNOSTIC";
    diagnostic.category = "test";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
    diagnostic.message = "diagnostic mirrored";
    Astra::Core::LogDiagnostic(diagnostic, "core.diagnostics", "test");
    Astra::Core::FlushLogs();

    REQUIRE(std::filesystem::exists(configured_log));
    REQUIRE(Astra::Core::DefaultLogger().MemoryEvents().size() == 2);
    REQUIRE(Astra::Core::DefaultLogger().MemoryEvents()[1].diagnostic_code == "ASTRA_TEST_LOG_DIAGNOSTIC");
    REQUIRE(Astra::Core::DefaultLogger().MemoryEvents()[1].level == Astra::Core::LogLevel::Error);
    std::ifstream configured(configured_log, std::ios::binary);
    const std::string contents(std::istreambuf_iterator<char>(configured), {});
    REQUIRE(contents.find("astra.log.event.v1") != std::string::npos);
    REQUIRE(contents.find("ASTRA_TEST_LOG_DIAGNOSTIC") != std::string::npos);
    Astra::Core::ResetLoggingForTests();
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



