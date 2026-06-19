#include <Astra/AstraGame/AstraGame.hpp>

#include <Astra/Asset/Asset.hpp>
#include <Astra/AstraVN/AstraVN.hpp>
#include <Astra/Core/BuildInfo.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/ModuleRuntime/ModuleRuntime.hpp>

#include <fstream>

namespace Astra::Game {
namespace {

void AddDiagnostic(Astra::Core::DiagnosticSink& diagnostics, std::string code,
                   Astra::Core::DiagnosticSeverity severity, std::string message,
                   const std::filesystem::path& file = {}) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "game.launch";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source.file = file.string();
    diagnostics.Emit(std::move(diagnostic));
}

Astra::Platform::BackendKind PlatformBackend(GameBackend backend) {
    switch (backend) {
    case GameBackend::Headless:
        return Astra::Platform::BackendKind::Headless;
    case GameBackend::Sdl:
        return Astra::Platform::BackendKind::Sdl;
    case GameBackend::Mobile:
        return Astra::Platform::BackendKind::Mobile;
    case GameBackend::Web:
        return Astra::Platform::BackendKind::Web;
    }
    return Astra::Platform::BackendKind::Headless;
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

nlohmann::json DiagnosticsJson(const Astra::Core::DiagnosticSink& diagnostics) {
    nlohmann::json out = nlohmann::json::array();
    for (const auto& diagnostic : diagnostics.Diagnostics()) {
        out.push_back(Astra::Core::ToJson(diagnostic));
    }
    return out;
}

std::string ReadText(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), {});
}

nlohmann::json RunVnSourceIfAvailable(const nlohmann::json& runtime_evidence,
                                      Astra::Core::DiagnosticSink& diagnostics) {
    const auto source_sample = runtime_evidence.value("source_sample", "");
    if (source_sample.empty()) {
        return nlohmann::json::object();
    }
    const std::filesystem::path sample = source_sample;
    const auto native_path = sample / "Content/Scripts/opening.astra";
    if (!std::filesystem::exists(native_path)) {
        return nlohmann::json::object();
    }

    Astra::Script::ScriptSource source;
    source.runtime_id = Astra::Script::NativeRuntimeId;
    source.source_id = "native:/Scripts/opening";
    source.file = native_path.generic_string();
    source.text = ReadText(native_path);

    Astra::AstraVN::VnSession session(44);
    Astra::Script::ScriptExecutionOptions options;
    options.entry_label = "title";
    options.choice_index = 0;
    auto result = session.RunNative(source, options, diagnostics);
    if (!result) {
        AddDiagnostic(diagnostics, "ASTRA_GAME_VN_LAUNCH_FAILED",
                      Astra::Core::DiagnosticSeverity::Blocking, result.Message(), native_path);
        return {{"status", "failed"}, {"reason", result.Message()}};
    }
    auto snapshot = session.CaptureSnapshot(diagnostics);
    return {
        {"status", "passed"},
        {"hashes", Astra::Runtime::ToJson(snapshot.hashes)},
        {"snapshot", Astra::AstraVN::ToJson(snapshot)},
    };
}

} // namespace

class GameSession::Impl {
  public:
    GameLaunchDesc desc;
    GameRunReport report;
};

GameSession::GameSession() : impl_(std::make_unique<Impl>()) {}
GameSession::~GameSession() = default;
GameSession::GameSession(GameSession&&) noexcept = default;
GameSession& GameSession::operator=(GameSession&&) noexcept = default;

Astra::Core::Result<void> GameSession::Launch(GameLaunchDesc desc,
                                              Astra::Core::DiagnosticSink& diagnostics) {
    if (desc.package_path.empty() || !std::filesystem::exists(desc.package_path)) {
        AddDiagnostic(diagnostics, "ASTRA_GAME_PACKAGE_MISSING",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Game package is missing.", desc.package_path);
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound,
                                                  "game package is missing");
    }
    impl_->desc = std::move(desc);
    impl_->report = {};
    impl_->report.package_path = impl_->desc.package_path;
    impl_->report.backend = ToString(impl_->desc.backend);
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<GameRunReport> GameSession::Run(Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Asset::PackageReader reader;
    auto manifest = reader.ReadManifest(impl_->desc.package_path, diagnostics);
    if (!manifest) {
        impl_->report.status = "failed";
        return Astra::Core::Result<GameRunReport>::Failure(manifest.Error(), manifest.Message());
    }
    impl_->report.package_manifest = Astra::Asset::ToJson(manifest.Value());

    auto mount = reader.MountPackage(impl_->desc.package_path, diagnostics);
    if (mount) {
        impl_->report.package_mount = Astra::Asset::ToJson(mount.Value());
    }

    const auto platform_id = impl_->desc.target_platform.empty()
                                 ? Astra::Platform::CurrentHostTargetPlatform().id
                                 : impl_->desc.target_platform;
    impl_->report.target_platform = platform_id;
    auto target = Astra::Platform::FindTargetPlatform(platform_id);
    if (!target) {
        AddDiagnostic(diagnostics, "ASTRA_GAME_TARGET_PLATFORM_UNSUPPORTED",
                      Astra::Core::DiagnosticSeverity::Blocking,
                      "Target platform is unsupported.", platform_id);
        impl_->report.status = "failed";
        return Astra::Core::Result<GameRunReport>::Success(impl_->report);
    }
    impl_->report.platform = {
        {"target", target.Value().id},
        {"is_host", target.Value().is_host},
        {"capabilities",
         {{"window", target.Value().capabilities.window},
          {"input", target.Value().capabilities.input},
          {"filesystem", target.Value().capabilities.filesystem},
          {"dynamic_library", target.Value().capabilities.dynamic_library}}},
    };

    auto platform = Astra::Platform::CreatePlatform(
        {PlatformBackend(impl_->desc.backend), platform_id}, diagnostics);
    if (!platform) {
        impl_->report.status = "failed";
        return Astra::Core::Result<GameRunReport>::Success(impl_->report);
    }

    if (impl_->desc.backend == GameBackend::Sdl) {
        auto created = platform.Value().Window().Create({"AstraGame", 1280, 720}, diagnostics);
        if (!created) {
            impl_->report.status = "failed";
            return Astra::Core::Result<GameRunReport>::Success(impl_->report);
        }
        platform.Value().Window().PumpEvents();
    }

    Astra::ModuleRuntime::ModuleManager manager(platform.Value());
    for (const auto& module : manifest.Value().modules) {
        if (!module.runtime_safe) {
            continue;
        }
        auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(module.binary, diagnostics);
        if (descriptor) {
            manager.LoadAndActivate(descriptor.Value(),
                                    std::filesystem::path(module.binary).parent_path(),
                                    diagnostics);
        }
    }

    Astra::Runtime::RuntimeWorld world(44);
    Astra::Runtime::RuntimeTickInput input;
    input.frame_index = 1;
    input.fixed_step_index = 1;
    auto frame = world.Tick(input, diagnostics);
    if (frame) {
        impl_->report.runtime["frame"] = Astra::Runtime::ToJson(frame.Value());
    }
    impl_->report.runtime["hashes"] = Astra::Runtime::ToJson(world.Hashes());
    impl_->report.runtime["vn"] =
        RunVnSourceIfAvailable(manifest.Value().runtime_evidence, diagnostics);
    impl_->report.launch = {
        {"build_info", BuildInfoJson()},
        {"auto_close", impl_->desc.auto_close},
        {"scripted_input", impl_->desc.scripted_input.empty() ? "" : impl_->desc.scripted_input.string()},
        {"load", impl_->desc.load.empty() ? "" : impl_->desc.load.string()},
        {"save_out", impl_->desc.save_out.empty() ? "" : impl_->desc.save_out.string()},
        {"diagnostics", DiagnosticsJson(diagnostics)},
    };
    if (impl_->desc.auto_close && impl_->desc.backend == GameBackend::Sdl) {
        platform.Value().Window().Close();
    }
    manager.DeactivateAndUnload(diagnostics);
    impl_->report.status = diagnostics.HasBlocking() ? "failed" : "passed";
    return Astra::Core::Result<GameRunReport>::Success(impl_->report);
}

void GameSession::Close() {}

std::string ToString(GameBackend backend) {
    switch (backend) {
    case GameBackend::Headless:
        return "headless";
    case GameBackend::Sdl:
        return "sdl";
    case GameBackend::Mobile:
        return "mobile";
    case GameBackend::Web:
        return "web";
    }
    return "headless";
}

GameBackend GameBackendFromString(std::string_view backend) {
    if (backend == "sdl") {
        return GameBackend::Sdl;
    }
    if (backend == "mobile") {
        return GameBackend::Mobile;
    }
    if (backend == "web") {
        return GameBackend::Web;
    }
    return GameBackend::Headless;
}

nlohmann::json ToJson(const GameRunReport& report) {
    return {
        {"schema", report.schema},
        {"status", report.status},
        {"package", report.package_path.string()},
        {"target_platform", report.target_platform},
        {"backend", report.backend},
        {"package_manifest", report.package_manifest},
        {"package_mount", report.package_mount},
        {"runtime", report.runtime},
        {"platform", report.platform},
        {"launch", report.launch},
    };
}

} // namespace Astra::Game
