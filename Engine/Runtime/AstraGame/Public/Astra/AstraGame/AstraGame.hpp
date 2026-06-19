#pragma once

#include <Astra/AstraGame/Export.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Platform/Platform.hpp>
#include <Astra/Runtime/Runtime.hpp>
#include <nlohmann/json.hpp>

#include <filesystem>
#include <memory>
#include <string>

namespace Astra::Game {

constexpr const char* GameRunReportSchema = "astra.game.run_report.v1";

enum class GameBackend {
    Headless,
    Sdl,
    Mobile,
    Web
};

struct GameLaunchDesc {
    std::filesystem::path package_path;
    std::string target_platform;
    GameBackend backend = GameBackend::Headless;
    bool auto_close = false;
    std::filesystem::path scripted_input;
    std::filesystem::path save_out;
    std::filesystem::path load;
    Astra::Core::u64 max_frames = 1;
};

struct GameRunReport {
    std::string schema = GameRunReportSchema;
    std::string status = "passed";
    std::filesystem::path package_path;
    std::string target_platform;
    std::string backend;
    nlohmann::json package_manifest = nlohmann::json::object();
    nlohmann::json package_mount = nlohmann::json::object();
    nlohmann::json runtime = nlohmann::json::object();
    nlohmann::json platform = nlohmann::json::object();
    nlohmann::json launch = nlohmann::json::object();
};

class ASTRA_GAME_API GameSession {
  public:
    GameSession();
    ~GameSession();

    GameSession(GameSession&&) noexcept;
    GameSession& operator=(GameSession&&) noexcept;
    GameSession(const GameSession&) = delete;
    GameSession& operator=(const GameSession&) = delete;

    [[nodiscard]] Astra::Core::Result<void> Launch(GameLaunchDesc desc,
                                                   Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<GameRunReport>
    Run(Astra::Core::DiagnosticSink& diagnostics);
    void Close();

  private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] ASTRA_GAME_API std::string ToString(GameBackend backend);
[[nodiscard]] ASTRA_GAME_API GameBackend GameBackendFromString(std::string_view backend);
[[nodiscard]] ASTRA_GAME_API nlohmann::json ToJson(const GameRunReport& report);

} // namespace Astra::Game

