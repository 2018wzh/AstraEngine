#include <Astra/Core/Log.h>

#include <spdlog/logger.h>
#include <spdlog/sinks/null_sink.h>
#include <spdlog/sinks/rotating_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>

#include <algorithm>
#include <deque>
#include <filesystem>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace astra::log {

namespace {

constexpr std::size_t default_max_file_size = 5u * 1024u * 1024u;
constexpr std::size_t default_max_file_count = 3u;

struct LogState {
    std::mutex mutex;
    bool initialized = false;
    Verbosity global_verbosity = Verbosity::Log;
    std::size_t memory_capacity = 1024;
    std::shared_ptr<spdlog::logger> logger;
    std::unordered_map<std::string, Verbosity> category_verbosity;
    std::deque<Record> records;
};

LogState& state() {
    static LogState log_state;
    return log_state;
}

int verbosity_rank(Verbosity verbosity) {
    return static_cast<int>(verbosity);
}

spdlog::level::level_enum spdlog_level(Verbosity verbosity) {
    switch (verbosity) {
    case Verbosity::Fatal:
        return spdlog::level::critical;
    case Verbosity::Error:
        return spdlog::level::err;
    case Verbosity::Warning:
        return spdlog::level::warn;
    case Verbosity::Display:
    case Verbosity::Log:
        return spdlog::level::info;
    case Verbosity::Verbose:
        return spdlog::level::debug;
    case Verbosity::VeryVerbose:
        return spdlog::level::trace;
    }
    return spdlog::level::info;
}

Verbosity effective_verbosity_locked(const LogState& log_state, Category category) {
    Verbosity category_verbosity = category.default_verbosity;
    const auto found = log_state.category_verbosity.find(std::string(category.name));
    if (found != log_state.category_verbosity.end()) {
        category_verbosity = found->second;
    }
    return verbosity_rank(log_state.global_verbosity) < verbosity_rank(category_verbosity)
               ? log_state.global_verbosity
               : category_verbosity;
}

bool should_log_locked(const LogState& log_state, Category category, Verbosity verbosity) {
    const Verbosity effective = effective_verbosity_locked(log_state, category);
    return verbosity_rank(verbosity) <= verbosity_rank(effective);
}

void configure_locked(LogState& log_state, const InitializeOptions& options) {
    std::vector<spdlog::sink_ptr> sinks;
    if (options.enable_console) {
        auto console_sink = std::make_shared<spdlog::sinks::stderr_color_sink_mt>();
        console_sink->set_pattern("[%H:%M:%S] %v");
        sinks.push_back(console_sink);
    }
    if (options.enable_file) {
        std::filesystem::create_directories(options.log_directory);
        const auto file_path = options.log_directory / (options.file_stem + ".log");
        auto file_sink = std::make_shared<spdlog::sinks::rotating_file_sink_mt>(
            file_path.string(), default_max_file_size, default_max_file_count);
        file_sink->set_pattern("[%Y-%m-%d %H:%M:%S.%e] %v");
        sinks.push_back(file_sink);
    }
    if (sinks.empty()) {
        sinks.push_back(std::make_shared<spdlog::sinks::null_sink_mt>());
    }

    log_state.logger = std::make_shared<spdlog::logger>("Astra", sinks.begin(), sinks.end());
    log_state.logger->set_level(spdlog::level::trace);
    log_state.logger->flush_on(spdlog::level::warn);
    log_state.initialized = true;
    log_state.global_verbosity = options.global_verbosity;
    log_state.memory_capacity = options.memory_capacity;
    log_state.category_verbosity.clear();
    log_state.records.clear();
}

void ensure_initialized_locked(LogState& log_state) {
    if (!log_state.initialized) {
        configure_locked(log_state, InitializeOptions{});
    }
}

std::string format_line(Category category, Verbosity verbosity, std::string_view message) {
    return fmt::format("{}: {}: {}", category.name, verbosity_name(verbosity), message);
}

Verbosity diagnostic_verbosity(DiagnosticSeverity severity) {
    switch (severity) {
    case DiagnosticSeverity::Info:
        return Verbosity::Display;
    case DiagnosticSeverity::Warning:
        return Verbosity::Warning;
    case DiagnosticSeverity::Error:
        return Verbosity::Error;
    }
    return Verbosity::Display;
}

} // namespace

void initialize(const InitializeOptions& options) {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    if (log_state.initialized && log_state.logger) {
        log_state.logger->flush();
    }
    configure_locked(log_state, options);
}

void shutdown() {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    if (log_state.logger) {
        log_state.logger->flush();
    }
    log_state.logger.reset();
    log_state.initialized = false;
    log_state.global_verbosity = Verbosity::Log;
    log_state.memory_capacity = 1024;
    log_state.category_verbosity.clear();
    log_state.records.clear();
}

void set_global_verbosity(Verbosity verbosity) {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    log_state.global_verbosity = verbosity;
}

void set_category_verbosity(Category category, Verbosity verbosity) {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    log_state.category_verbosity[std::string(category.name)] = verbosity;
}

bool should_log(Category category, Verbosity verbosity) {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    return should_log_locked(log_state, category, verbosity);
}

std::vector<Record> recent_records() {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    return {log_state.records.begin(), log_state.records.end()};
}

void clear_recent_records() {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    log_state.records.clear();
}

std::string_view verbosity_name(Verbosity verbosity) {
    switch (verbosity) {
    case Verbosity::Fatal:
        return "Fatal";
    case Verbosity::Error:
        return "Error";
    case Verbosity::Warning:
        return "Warning";
    case Verbosity::Display:
        return "Display";
    case Verbosity::Log:
        return "Log";
    case Verbosity::Verbose:
        return "Verbose";
    case Verbosity::VeryVerbose:
        return "VeryVerbose";
    }
    return "Log";
}

namespace detail {

void write_message(Category category, Verbosity verbosity, std::string message) {
    LogState& log_state = state();
    std::scoped_lock lock(log_state.mutex);
    ensure_initialized_locked(log_state);

    if (log_state.memory_capacity > 0) {
        log_state.records.push_back(Record{std::string(category.name), verbosity, message});
        while (log_state.records.size() > log_state.memory_capacity) {
            log_state.records.pop_front();
        }
    }

    log_state.logger->log(spdlog_level(verbosity), "{}", format_line(category, verbosity, message));
    if (verbosity == Verbosity::Fatal || verbosity == Verbosity::Error) {
        log_state.logger->flush();
    }
}

} // namespace detail

void write(Category category, Verbosity verbosity, std::string_view message) {
    if (!should_log(category, verbosity)) {
        return;
    }
    detail::write_message(category, verbosity, std::string(message));
}

void error(Category category, std::string_view message) {
    write(category, Verbosity::Error, message);
}

void warn(Category category, std::string_view message) {
    write(category, Verbosity::Warning, message);
}

void display(Category category, std::string_view message) {
    write(category, Verbosity::Display, message);
}

void info(Category category, std::string_view message) {
    write(category, Verbosity::Log, message);
}

void verbose(Category category, std::string_view message) {
    write(category, Verbosity::Verbose, message);
}

void very_verbose(Category category, std::string_view message) {
    write(category, Verbosity::VeryVerbose, message);
}

void info(std::string_view message) {
    info(LogCore, message);
}

void warn(std::string_view message) {
    warn(LogCore, message);
}

void error(std::string_view message) {
    error(LogCore, message);
}

void write_diagnostic(const Diagnostic& diagnostic, Category category) {
    write(category, diagnostic_verbosity(diagnostic.severity), "{}: {}", diagnostic.code,
          diagnostic.message);
}

void write_diagnostics(const DiagnosticSink& diagnostics, Category category) {
    for (const Diagnostic& diagnostic : diagnostics.diagnostics()) {
        write_diagnostic(diagnostic, category);
    }
}

} // namespace astra::log
