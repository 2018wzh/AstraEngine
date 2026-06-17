#include <Astra/Tools/Tools.hpp>

#include <Astra/Core/BuildInfo.hpp>

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <regex>
#include <string_view>
#include <vector>

namespace Astra::Tools {

namespace {

std::filesystem::path SourceRoot() {
#if defined(ASTRA_SOURCE_ROOT)
    return ASTRA_SOURCE_ROOT;
#else
    return std::filesystem::current_path();
#endif
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

CommandReport MakeReport(std::string command) {
    CommandReport report;
    report.command = std::move(command);
    report.build_info = BuildInfoJson();
    return report;
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
    report.diagnostics.push_back(std::move(diagnostic));
    if (severity == Astra::Core::DiagnosticSeverity::Blocking ||
        severity == Astra::Core::DiagnosticSeverity::Fatal) {
        report.status = "failed";
    }
}

void AddDiagnosticAtLine(CommandReport& report, std::string code,
                         Astra::Core::DiagnosticSeverity severity, std::string message,
                         const std::filesystem::path& file, Astra::Core::u32 line) {
    AddDiagnostic(report, std::move(code), severity, std::move(message), file);
    report.diagnostics.back().source.line = line;
}

std::string ReadText(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), {});
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

std::string Trim(std::string value) {
    auto is_space = [](unsigned char ch) { return std::isspace(ch) != 0; };
    value.erase(value.begin(), std::find_if_not(value.begin(), value.end(), is_space));
    value.erase(std::find_if_not(value.rbegin(), value.rend(), is_space).base(), value.end());
    return value;
}

std::string PercentDecode(std::string_view value) {
    std::string decoded;
    decoded.reserve(value.size());
    for (std::size_t index = 0; index < value.size(); ++index) {
        if (value[index] == '%' && index + 2 < value.size()) {
            const auto hex = value.substr(index + 1, 2);
            char* end = nullptr;
            const auto byte = std::strtol(std::string(hex).c_str(), &end, 16);
            if (end != nullptr && *end == '\0') {
                decoded.push_back(static_cast<char>(byte));
                index += 2;
                continue;
            }
        }
        decoded.push_back(value[index]);
    }
    return decoded;
}

bool HasUriScheme(std::string_view value) {
    static const std::regex kScheme(R"(^[a-zA-Z][a-zA-Z0-9+.-]*:)");
    return std::regex_search(value.begin(), value.end(), kScheme);
}

std::vector<std::filesystem::path> CollectFiles(const std::vector<std::filesystem::path>& roots,
                                                const std::optional<std::string>& extension) {
    std::vector<std::filesystem::path> files;
    for (const auto& root : roots) {
        if (!std::filesystem::exists(root)) {
            continue;
        }
        if (std::filesystem::is_regular_file(root)) {
            if (!extension || root.extension() == *extension) {
                files.push_back(root);
            }
            continue;
        }
        for (const auto& entry : std::filesystem::recursive_directory_iterator(root)) {
            if (entry.is_regular_file() && (!extension || entry.path().extension() == *extension)) {
                files.push_back(entry.path());
            }
        }
    }
    std::sort(files.begin(), files.end());
    return files;
}

std::vector<std::string> ExtractMarkdownLocalLinks(std::string_view content) {
    std::vector<std::string> links;
    std::size_t cursor = 0;
    while (cursor < content.size()) {
        const auto open = content.find('[', cursor);
        if (open == std::string_view::npos) {
            break;
        }
        if (open > 0 && content[open - 1] == '!') {
            cursor = open + 1;
            continue;
        }
        const auto close = content.find("](", open);
        if (close == std::string_view::npos) {
            break;
        }
        const auto target_begin = close + 2;
        const auto target_end = content.find(')', target_begin);
        if (target_end == std::string_view::npos) {
            break;
        }
        auto target = Trim(std::string(content.substr(target_begin, target_end - target_begin)));
        if (!target.empty() && target.front() != '#' && !HasUriScheme(target)) {
            links.push_back(std::move(target));
        }
        cursor = target_end + 1;
    }
    return links;
}

void CheckRequiredManualPages(CommandReport& report, const std::filesystem::path& root,
                              nlohmann::json& artifacts) {
    const std::array required_pages = {
        "docs/manual/README.md",
        "docs/manual/getting-started/README.md",
        "docs/manual/programming/README.md",
        "docs/manual/systems/README.md",
        "docs/manual/api/README.md",
        "docs/manual/editor/README.md",
        "docs/manual/samples/README.md",
        "docs/manual/migration/README.md",
        "docs/manual/release-notes/README.md",
        "docs/manual/concepts/README.md",
    };
    const std::array required_sections = {
        "## Overview",       "## Key Concepts", "## Architecture", "## Programming Guide",
        "## API Reference", "## Examples",     "## Troubleshooting",
    };

    artifacts["required_manual_pages"] = required_pages.size();
    artifacts["required_manual_sections_per_page"] = required_sections.size();

    for (const auto* relative_path : required_pages) {
        const auto path = root / relative_path;
        if (!std::filesystem::exists(path)) {
            AddDiagnostic(report, "ASTRA_DOC_MANUAL_PAGE_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Missing required manual page: " + std::string(relative_path), path);
            continue;
        }

        const auto content = ReadText(path);
        for (const auto* section : required_sections) {
            if (content.find(section) == std::string::npos) {
                AddDiagnostic(report, "ASTRA_DOC_MANUAL_SECTION_MISSING",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              StableSourcePath(path) + " is missing section '" + section + "'",
                              path);
            }
        }
    }
}

void CheckMarkdownLinks(CommandReport& report, const std::filesystem::path& root,
                        nlohmann::json& artifacts) {
    const auto markdown_files = CollectFiles({root / "README.md", root / "docs"}, ".md");
    std::size_t checked_links = 0;
    for (const auto& file : markdown_files) {
        const auto links = ExtractMarkdownLocalLinks(ReadText(file));
        for (const auto& target : links) {
            const auto without_anchor = target.substr(0, target.find('#'));
            if (Trim(without_anchor).empty()) {
                continue;
            }
            ++checked_links;
            const auto candidate = file.parent_path() / PercentDecode(without_anchor);
            if (!std::filesystem::exists(candidate)) {
                AddDiagnostic(report, "ASTRA_DOC_LINK_BROKEN",
                              Astra::Core::DiagnosticSeverity::Blocking,
                              "Broken link in " + StableSourcePath(file) + ": " + target, file);
            }
        }
    }
    artifacts["markdown_files_checked"] = markdown_files.size();
    artifacts["local_markdown_links_checked"] = checked_links;
}

void CheckRequiredDesignFiles(CommandReport& report, const std::filesystem::path& root,
                              nlohmann::json& artifacts) {
    const std::array required_design_files = {
        "docs/design/README.md",
        "docs/design/goals.md",
        "docs/design/architecture.md",
        "docs/design/implementation-coverage.md",
        "docs/design/roadmap.md",
        "docs/design/TODO.md",
        "docs/design/foundation-core-platform-property.md",
        "docs/design/extension-and-module-system.md",
        "docs/design/tools-release-observability.md",
        "docs/design/samples-and-test-matrix.md",
    };
    artifacts["required_design_files"] = required_design_files.size();
    for (const auto* relative_path : required_design_files) {
        const auto path = root / relative_path;
        if (!std::filesystem::exists(path)) {
            AddDiagnostic(report, "ASTRA_DOC_DESIGN_DOCUMENT_MISSING",
                          Astra::Core::DiagnosticSeverity::Blocking,
                          "Missing required design document: " + std::string(relative_path), path);
        }
    }
}

struct ForbiddenDocWording {
    std::regex pattern;
    std::regex allow;
};

void CheckStaleWording(CommandReport& report, const std::filesystem::path& root,
                       nlohmann::json& artifacts) {
    const auto flags = std::regex_constants::ECMAScript | std::regex_constants::icase;
    const std::vector<ForbiddenDocWording> checks = {
        {std::regex(R"(AstraGame(\.exe|`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex(R"(VNRuntimeServices(`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex(R"(Bootstrap(`|\b))", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|VNRuntimeServices.*Bootstrap.*AstraGame)",
                    flags)},
        {std::regex("MinimalVN", flags),
         std::regex(R"(do not revive|deleted legacy|historical|history|deleted|planned|roadmap)",
                    flags)},
        {std::regex(R"(AI\s+Workbench)", flags), std::regex(R"($^)", flags)},
    };

    const auto scan_files = CollectFiles({root / "README.md", root / "docs", root / ".github"},
                                         std::nullopt);
    std::size_t lines_checked = 0;
    for (const auto& file : scan_files) {
        std::ifstream stream(file, std::ios::binary);
        std::string line;
        Astra::Core::u32 line_number = 0;
        while (std::getline(stream, line)) {
            ++line_number;
            ++lines_checked;
            for (const auto& check : checks) {
                if (std::regex_search(line, check.pattern) &&
                    !std::regex_search(line, check.allow)) {
                    AddDiagnosticAtLine(report, "ASTRA_DOC_STALE_WORDING",
                                        Astra::Core::DiagnosticSeverity::Blocking,
                                        "Stale wording in " + StableSourcePath(file) + ":" +
                                            std::to_string(line_number) + ": " + line,
                                        file, line_number);
                }
            }
        }
    }
    artifacts["stale_wording_files_checked"] = scan_files.size();
    artifacts["stale_wording_lines_checked"] = lines_checked;
}

} // namespace

CommandReport DocCheck(const CommandOptions& options) {
    (void)options;
    auto report = MakeReport("astra doc-check");
    const auto root = SourceRoot();
    report.artifacts["doc_check"] = {
        {"schema", "astra.doc_check.v1"},
        {"source_root", root.generic_string()},
        {"checks",
         {"manual_required_pages", "markdown_local_links", "required_design_documents",
          "stale_wording"}},
    };
    auto& artifacts = report.artifacts["doc_check"];
    CheckRequiredManualPages(report, root, artifacts);
    CheckMarkdownLinks(report, root, artifacts);
    CheckRequiredDesignFiles(report, root, artifacts);
    CheckStaleWording(report, root, artifacts);
    return report;
}

} // namespace Astra::Tools
