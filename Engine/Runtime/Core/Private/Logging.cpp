#include <Astra/Core/Logging.hpp>

#include <spdlog/async.h>
#include <spdlog/sinks/rotating_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>

#include <atomic>
#include <chrono>
#include <deque>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <mutex>
#include <sstream>
#include <thread>
#include <utility>

namespace Astra::Core {

namespace {

std::atomic<u64> g_sequence{1};
std::once_flag g_thread_pool_once;

u64 NowNanoseconds() {
    return static_cast<u64>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now().time_since_epoch())
            .count());
}

std::string CurrentThreadId() {
    std::ostringstream stream;
    stream << std::this_thread::get_id();
    return stream.str();
}

spdlog::level::level_enum ToSpdlogLevel(LogLevel level) {
    switch (level) {
    case LogLevel::Trace:
        return spdlog::level::trace;
    case LogLevel::Debug:
        return spdlog::level::debug;
    case LogLevel::Info:
        return spdlog::level::info;
    case LogLevel::Warning:
        return spdlog::level::warn;
    case LogLevel::Error:
        return spdlog::level::err;
    case LogLevel::Fatal:
        return spdlog::level::critical;
    }
    return spdlog::level::info;
}

LogLevel FromDiagnosticSeverity(DiagnosticSeverity severity) {
    switch (severity) {
    case DiagnosticSeverity::Info:
        return LogLevel::Info;
    case DiagnosticSeverity::Warning:
        return LogLevel::Warning;
    case DiagnosticSeverity::Error:
    case DiagnosticSeverity::Blocking:
        return LogLevel::Error;
    case DiagnosticSeverity::Fatal:
        return LogLevel::Fatal;
    }
    return LogLevel::Error;
}

std::filesystem::path DefaultLogFile(const LogConfig& config) {
    if (!config.log_file.empty()) {
        return config.log_file;
    }
    const auto directory = config.log_directory.empty() ? std::filesystem::path("Saved/Logs") : config.log_directory;
    return directory / "astra.log.jsonl";
}

} // namespace

class Logger::Impl {
public:
    void Configure(LogConfig new_config) {
        std::lock_guard lock(mutex_);
        config_ = std::move(new_config);
        console_logger_.reset();
        file_logger_.reset();

        if (config_.console_enabled) {
            auto console = std::make_shared<spdlog::sinks::stderr_color_sink_mt>();
            console->set_level(ToSpdlogLevel(config_.console_level));
            console->set_pattern("%^[%l]%$ %v");
            console_logger_ = MakeLogger("astra.console", {std::move(console)});
            console_logger_->flush_on(ToSpdlogLevel(config_.flush_on));
        }
        if (config_.file_enabled) {
            const auto file = DefaultLogFile(config_);
            if (file.has_parent_path()) {
                std::filesystem::create_directories(file.parent_path());
            }
            auto rotating = std::make_shared<spdlog::sinks::rotating_file_sink_mt>(
                file.string(), config_.rotation_bytes, config_.rotation_files);
            rotating->set_level(ToSpdlogLevel(config_.file_level));
            rotating->set_pattern("%v");
            file_logger_ = MakeLogger("astra.file", {std::move(rotating)});
            file_logger_->flush_on(ToSpdlogLevel(config_.flush_on));
        }
    }

    void Write(LogEvent event) {
        if (event.sequence == 0) {
            event.sequence = g_sequence.fetch_add(1);
        }
        if (event.monotonic_ns == 0) {
            event.monotonic_ns = NowNanoseconds();
        }
        if (event.thread_id.empty()) {
            event.thread_id = CurrentThreadId();
        }

        std::shared_ptr<spdlog::logger> console_logger;
        std::shared_ptr<spdlog::logger> file_logger;
        LogLevel console_level = LogLevel::Info;
        LogLevel file_level = LogLevel::Debug;
        {
            std::lock_guard lock(mutex_);
            PushRecent(event);
            if (config_.capture_memory) {
                memory_.push_back(event);
            }
            console_logger = console_logger_;
            file_logger = file_logger_;
            console_level = config_.console_level;
            file_level = config_.file_level;
        }

        const auto json_line = ToJson(event).dump();
        const auto console_line = event.component.empty()
                                      ? event.channel + ": " + event.message
                                      : event.channel + " [" + event.component + "]: " + event.message;
        if (console_logger && event.level >= console_level) {
            console_logger->log(ToSpdlogLevel(event.level), console_line);
        }
        if (file_logger && event.level >= file_level) {
            file_logger->log(ToSpdlogLevel(event.level), json_line);
        }
    }

    void Flush() {
        std::shared_ptr<spdlog::logger> console_logger;
        std::shared_ptr<spdlog::logger> file_logger;
        {
            std::lock_guard lock(mutex_);
            console_logger = console_logger_;
            file_logger = file_logger_;
        }
        if (console_logger) {
            console_logger->flush();
        }
        if (file_logger) {
            file_logger->flush();
        }
    }

    std::vector<LogEvent> RecentEvents() const {
        std::lock_guard lock(mutex_);
        return {recent_.begin(), recent_.end()};
    }

    std::vector<LogEvent> MemoryEvents() const {
        std::lock_guard lock(mutex_);
        return memory_;
    }

    void ResetForTests() {
        std::lock_guard lock(mutex_);
        recent_.clear();
        memory_.clear();
        console_logger_.reset();
        file_logger_.reset();
    }

private:
    std::shared_ptr<spdlog::logger> MakeLogger(const std::string& name, std::vector<spdlog::sink_ptr> sinks) const {
        std::shared_ptr<spdlog::logger> logger;
        if (config_.async) {
            std::call_once(g_thread_pool_once, [&] {
                spdlog::init_thread_pool(config_.async_queue_size, 1);
            });
            logger = std::make_shared<spdlog::async_logger>(
                name,
                sinks.begin(),
                sinks.end(),
                spdlog::thread_pool(),
                spdlog::async_overflow_policy::block);
        } else {
            logger = std::make_shared<spdlog::logger>(name, sinks.begin(), sinks.end());
        }
        logger->set_level(spdlog::level::trace);
        return logger;
    }

    void PushRecent(const LogEvent& event) {
        if (config_.recent_event_count == 0) {
            return;
        }
        recent_.push_back(event);
        while (recent_.size() > config_.recent_event_count) {
            recent_.pop_front();
        }
    }

    mutable std::mutex mutex_;
    LogConfig config_;
    std::shared_ptr<spdlog::logger> console_logger_;
    std::shared_ptr<spdlog::logger> file_logger_;
    std::deque<LogEvent> recent_;
    std::vector<LogEvent> memory_;
};

Logger::Logger() : impl_(std::make_unique<Impl>()) {}

Logger::~Logger() = default;

Logger::Logger(Logger&&) noexcept = default;

Logger& Logger::operator=(Logger&&) noexcept = default;

void Logger::Configure(LogConfig config) {
    impl_->Configure(std::move(config));
}

void Logger::Write(LogEvent event) {
    impl_->Write(std::move(event));
}

void Logger::Log(std::string channel,
                 std::string component,
                 LogLevel level,
                 std::string message,
                 std::unordered_map<std::string, std::string> fields) {
    LogEvent event;
    event.channel = std::move(channel);
    event.component = std::move(component);
    event.level = level;
    event.message = std::move(message);
    event.fields = std::move(fields);
    Write(std::move(event));
}

void Logger::Flush() {
    impl_->Flush();
}

std::vector<LogEvent> Logger::RecentEvents() const {
    return impl_->RecentEvents();
}

std::vector<LogEvent> Logger::MemoryEvents() const {
    return impl_->MemoryEvents();
}

void Logger::ResetForTests() {
    impl_->ResetForTests();
}

void MemoryLogSink::Write(LogEvent event) {
    events_.push_back(std::move(event));
}

const std::vector<LogEvent>& MemoryLogSink::Events() const {
    return events_;
}

void ConsoleLogSink::Write(const LogEvent& event) const {
    std::cout << "[" << ToString(event.level) << "] " << event.channel << ": " << event.message << '\n';
}

FileLogSink::FileLogSink(std::string path, std::size_t max_bytes_before_rotation)
    : path_(std::move(path)), max_bytes_before_rotation_(max_bytes_before_rotation) {}

void FileLogSink::Write(const LogEvent& event) const {
    if (max_bytes_before_rotation_ > 0 && std::filesystem::exists(path_) && std::filesystem::file_size(path_) >= max_bytes_before_rotation_) {
        const auto rotated = path_ + ".1";
        std::error_code ignored;
        std::filesystem::remove(rotated, ignored);
        std::filesystem::rename(path_, rotated, ignored);
    }
    std::ofstream file(path_, std::ios::app | std::ios::binary);
    file << ToJson(event).dump() << '\n';
}

Logger& DefaultLogger() {
    static Logger logger;
    return logger;
}

void ConfigureLogging(LogConfig config) {
    DefaultLogger().Configure(std::move(config));
}

void FlushLogs() {
    DefaultLogger().Flush();
}

void ResetLoggingForTests() {
    DefaultLogger().ResetForTests();
}

void LogDiagnostic(const Diagnostic& diagnostic, std::string channel, std::string component) {
    LogEvent event;
    event.channel = std::move(channel);
    event.component = std::move(component);
    event.level = FromDiagnosticSeverity(diagnostic.severity);
    event.message = diagnostic.message;
    event.objects = diagnostic.objects;
    event.fields = diagnostic.context;
    event.source = diagnostic.source;
    event.diagnostic_code = diagnostic.code;
    DefaultLogger().Write(std::move(event));
}

std::string ToString(LogLevel level) {
    switch (level) {
    case LogLevel::Trace:
        return "trace";
    case LogLevel::Debug:
        return "debug";
    case LogLevel::Info:
        return "info";
    case LogLevel::Warning:
        return "warning";
    case LogLevel::Error:
        return "error";
    case LogLevel::Fatal:
        return "fatal";
    }
    return "info";
}

LogLevel LogLevelFromString(std::string_view value) {
    if (value == "trace") {
        return LogLevel::Trace;
    }
    if (value == "debug") {
        return LogLevel::Debug;
    }
    if (value == "warning" || value == "warn") {
        return LogLevel::Warning;
    }
    if (value == "error") {
        return LogLevel::Error;
    }
    if (value == "fatal" || value == "critical") {
        return LogLevel::Fatal;
    }
    return LogLevel::Info;
}

nlohmann::json ToJson(const LogEvent& event) {
    nlohmann::json objects = nlohmann::json::array();
    for (const auto& object : event.objects) {
        objects.push_back({{"kind", object.kind}, {"id", object.id}});
    }

    nlohmann::json json = {
        {"schema", event.schema},
        {"sequence", event.sequence},
        {"time_ns", event.monotonic_ns},
        {"thread_id", event.thread_id},
        {"channel", event.channel},
        {"component", event.component},
        {"level", ToString(event.level)},
        {"message", event.message},
        {"objects", objects},
        {"fields", event.fields},
        {"source", {{"file", event.source.file}, {"line", event.source.line}, {"column", event.source.column}}},
    };
    if (!event.diagnostic_code.empty()) {
        json["diagnostic_code"] = event.diagnostic_code;
    }
    if (event.frame_index.has_value()) {
        json["frame_index"] = *event.frame_index;
    }
    if (!event.package_id.empty()) {
        json["package_id"] = event.package_id;
    }
    if (!event.module_id.empty()) {
        json["module_id"] = event.module_id;
    }
    if (!event.asset_id.empty()) {
        json["asset_id"] = event.asset_id;
    }
    return json;
}

} // namespace Astra::Core
