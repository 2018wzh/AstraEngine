#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <nlohmann/json.hpp>

#include <filesystem>
#include <string>
#include <vector>

namespace Astra::Tools {

struct CommandOptions {
    bool json = false;
    bool strict = false;
    bool headless_smoke = false;
    std::string profile = "development";
    std::filesystem::path diagnostics_out;
};

struct CommandReport {
    std::string command;
    std::string status = "passed";
    nlohmann::json build_info;
    std::vector<Astra::Core::Diagnostic> diagnostics;
    nlohmann::json artifacts = nlohmann::json::object();

    [[nodiscard]] bool Passed() const;
};

[[nodiscard]] nlohmann::json ToJson(const CommandReport& report);
void WriteDiagnosticsIfRequested(const CommandReport& report, const CommandOptions& options);
void PrintReport(const CommandReport& report, const CommandOptions& options);

[[nodiscard]] CommandReport VersionReport();
[[nodiscard]] CommandReport DocCheck(const CommandOptions& options);
[[nodiscard]] CommandReport Validate(const std::filesystem::path& target, const CommandOptions& options);
[[nodiscard]] CommandReport Inspect(const std::filesystem::path& target, const CommandOptions& options);
[[nodiscard]] CommandReport Cook(const std::filesystem::path& sample, const CommandOptions& options);
[[nodiscard]] CommandReport Package(const std::filesystem::path& sample, const CommandOptions& options);
[[nodiscard]] CommandReport Run(const std::filesystem::path& target, const CommandOptions& options);

[[nodiscard]] std::string Sha256File(const std::filesystem::path& path);

} // namespace Astra::Tools
