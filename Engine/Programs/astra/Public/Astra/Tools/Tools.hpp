#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Tools/Export.hpp>
#include <nlohmann/json.hpp>

#include <filesystem>
#include <string>
#include <vector>

namespace Astra::Tools {

struct CommandOptions {
    bool json = false;
    bool strict = false;
    bool headless_smoke = false;
    bool windowed_smoke = false;
    bool gpu_smoke = false;
    bool auto_close = false;
    bool compare = false;
    bool log_async = true;
    bool log_sync = false;
    std::string config = "Debug";
    std::string profile = "development";
    std::string log_level = "debug";
    std::filesystem::path diagnostics_out;
    std::filesystem::path log_dir;
    std::filesystem::path log_file;
    std::filesystem::path scripted_input;
    std::filesystem::path save_out;
    std::filesystem::path load;
    std::string import_asset_id;
    std::string import_asset_type = "image";
    std::string import_preset = "default";
    std::string import_license_owner = "project";
    std::string import_license_usage = "internal";
};

struct CommandReport {
    std::string command;
    std::string status = "passed";
    nlohmann::json build_info;
    std::vector<Astra::Core::Diagnostic> diagnostics;
    nlohmann::json artifacts = nlohmann::json::object();

    [[nodiscard]] ASTRA_TOOLS_API bool Passed() const;
};

[[nodiscard]] ASTRA_TOOLS_API nlohmann::json ToJson(const CommandReport& report);
ASTRA_TOOLS_API void WriteDiagnosticsIfRequested(const CommandReport& report,
                                                 const CommandOptions& options);
ASTRA_TOOLS_API void PrintReport(const CommandReport& report, const CommandOptions& options);
ASTRA_TOOLS_API void ConfigureToolLogging(const CommandOptions& options);

[[nodiscard]] ASTRA_TOOLS_API CommandReport VersionReport();
[[nodiscard]] ASTRA_TOOLS_API CommandReport DocCheck(const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Validate(const std::filesystem::path& target,
                                                     const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Inspect(const std::filesystem::path& target,
                                                    const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Import(const std::filesystem::path& project,
                                                   const std::filesystem::path& source,
                                                   const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Cook(const std::filesystem::path& sample,
                                                 const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Package(const std::filesystem::path& sample,
                                                    const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport ReleaseGate(const std::filesystem::path& target,
                                                        const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Run(const std::filesystem::path& target,
                                                const CommandOptions& options);
[[nodiscard]] ASTRA_TOOLS_API CommandReport Replay(const std::filesystem::path& target,
                                                   const CommandOptions& options);

[[nodiscard]] ASTRA_TOOLS_API std::string Sha256File(const std::filesystem::path& path);

} // namespace Astra::Tools
