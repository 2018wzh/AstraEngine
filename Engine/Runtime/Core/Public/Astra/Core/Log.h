#pragma once

#include <Astra/Core/Diagnostics.h>

#include <fmt/format.h>

#include <cstddef>
#include <filesystem>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace astra::log {

enum class Verbosity {
    Fatal = 0,
    Error = 1,
    Warning = 2,
    Display = 3,
    Log = 4,
    Verbose = 5,
    VeryVerbose = 6,
};

struct Category {
    std::string_view name;
    Verbosity default_verbosity = Verbosity::Log;
};

struct InitializeOptions {
    std::filesystem::path log_directory = "Saved/Logs";
    std::string file_stem = "Astra";
    bool enable_console = true;
    bool enable_file = true;
    std::size_t memory_capacity = 1024;
    Verbosity global_verbosity = Verbosity::Log;
};

struct Record {
    std::string category;
    Verbosity verbosity = Verbosity::Log;
    std::string message;
};

inline constexpr Category LogCore{"LogCore", Verbosity::Log};
inline constexpr Category LogApplication{"LogApplication", Verbosity::Log};
inline constexpr Category LogDiagnostics{"LogDiagnostics", Verbosity::Log};
inline constexpr Category LogPlatform{"LogPlatform", Verbosity::Log};
inline constexpr Category LogRuntime{"LogRuntime", Verbosity::Log};
inline constexpr Category LogModule{"LogModule", Verbosity::Log};
inline constexpr Category LogAsset{"LogAsset", Verbosity::Log};
inline constexpr Category LogRenderer{"LogRenderer", Verbosity::Log};
inline constexpr Category LogAudio{"LogAudio", Verbosity::Log};

void initialize(const InitializeOptions& options = {});
void shutdown();

void set_global_verbosity(Verbosity verbosity);
void set_category_verbosity(Category category, Verbosity verbosity);
[[nodiscard]] bool should_log(Category category, Verbosity verbosity);

[[nodiscard]] std::vector<Record> recent_records();
void clear_recent_records();

[[nodiscard]] std::string_view verbosity_name(Verbosity verbosity);

namespace detail {

void write_message(Category category, Verbosity verbosity, std::string message);

} // namespace detail

void write(Category category, Verbosity verbosity, std::string_view message);
void error(Category category, std::string_view message);
void warn(Category category, std::string_view message);
void display(Category category, std::string_view message);
void info(Category category, std::string_view message);
void verbose(Category category, std::string_view message);
void very_verbose(Category category, std::string_view message);

template <typename... Args>
void write(Category category, Verbosity verbosity, fmt::format_string<Args...> format,
           Args&&... args) {
    if (!should_log(category, verbosity)) {
        return;
    }
    detail::write_message(category, verbosity, fmt::format(format, std::forward<Args>(args)...));
}

template <typename... Args>
void error(Category category, fmt::format_string<Args...> format, Args&&... args) {
    write(category, Verbosity::Error, format, std::forward<Args>(args)...);
}

template <typename... Args>
void warn(Category category, fmt::format_string<Args...> format, Args&&... args) {
    write(category, Verbosity::Warning, format, std::forward<Args>(args)...);
}

template <typename... Args>
void display(Category category, fmt::format_string<Args...> format, Args&&... args) {
    write(category, Verbosity::Display, format, std::forward<Args>(args)...);
}

template <typename... Args>
void info(Category category, fmt::format_string<Args...> format, Args&&... args) {
    write(category, Verbosity::Log, format, std::forward<Args>(args)...);
}

template <typename... Args>
void verbose(Category category, fmt::format_string<Args...> format, Args&&... args) {
    write(category, Verbosity::Verbose, format, std::forward<Args>(args)...);
}

template <typename... Args>
void very_verbose(Category category, fmt::format_string<Args...> format, Args&&... args) {
    write(category, Verbosity::VeryVerbose, format, std::forward<Args>(args)...);
}

void info(std::string_view message);
void warn(std::string_view message);
void error(std::string_view message);

void write_diagnostic(const Diagnostic& diagnostic, Category category = LogDiagnostics);
void write_diagnostics(const DiagnosticSink& diagnostics, Category category = LogDiagnostics);

} // namespace astra::log
