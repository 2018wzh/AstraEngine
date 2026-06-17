#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Export.hpp>
#include <nlohmann/json.hpp>

#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace Astra::Core {

enum class LogLevel {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
    Fatal
};

struct LogEvent {
    std::string schema = "astra.log.event.v1";
    u64 sequence = 0;
    u64 monotonic_ns = 0;
    std::string thread_id;
    std::string channel;
    std::string component;
    LogLevel level = LogLevel::Info;
    std::string message;
    std::vector<DiagnosticObject> objects;
    std::unordered_map<std::string, std::string> fields;
    SourceLocation source;
    std::string diagnostic_code;
    std::optional<u64> frame_index;
    std::string package_id;
    std::string module_id;
    std::string asset_id;
};

struct LogConfig {
    std::filesystem::path log_directory;
    std::filesystem::path log_file;
    LogLevel console_level = LogLevel::Info;
    LogLevel file_level = LogLevel::Debug;
    LogLevel flush_on = LogLevel::Error;
    std::size_t rotation_bytes = 10 * 1024 * 1024;
    std::size_t rotation_files = 5;
    std::size_t async_queue_size = 8192;
    std::size_t recent_event_count = 256;
    bool async = true;
    bool console_enabled = true;
    bool file_enabled = true;
    bool capture_memory = false;
    std::string detail_mode = "full";
};

class ASTRA_CORE_API Logger {
public:
    Logger();
    ~Logger();

    Logger(Logger&&) noexcept;
    Logger& operator=(Logger&&) noexcept;

    Logger(const Logger&) = delete;
    Logger& operator=(const Logger&) = delete;

    void Configure(LogConfig config);
    void Write(LogEvent event);
    void Log(std::string channel,
             std::string component,
             LogLevel level,
             std::string message,
             std::unordered_map<std::string, std::string> fields = {});
    void Flush();
    [[nodiscard]] std::vector<LogEvent> RecentEvents() const;
    [[nodiscard]] std::vector<LogEvent> MemoryEvents() const;
    void ResetForTests();

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

class ASTRA_CORE_API MemoryLogSink {
public:
    void Write(LogEvent event);
    [[nodiscard]] const std::vector<LogEvent>& Events() const;

private:
    std::vector<LogEvent> events_;
};

class ASTRA_CORE_API ConsoleLogSink {
public:
    void Write(const LogEvent& event) const;
};

class ASTRA_CORE_API FileLogSink {
public:
    explicit FileLogSink(std::string path, std::size_t max_bytes_before_rotation = 1024 * 1024);
    void Write(const LogEvent& event) const;

private:
    std::string path_;
    std::size_t max_bytes_before_rotation_ = 0;
};

[[nodiscard]] ASTRA_CORE_API Logger& DefaultLogger();
ASTRA_CORE_API void ConfigureLogging(LogConfig config);
ASTRA_CORE_API void FlushLogs();
ASTRA_CORE_API void ResetLoggingForTests();
ASTRA_CORE_API void LogDiagnostic(const Diagnostic& diagnostic, std::string channel, std::string component);
[[nodiscard]] ASTRA_CORE_API std::string ToString(LogLevel level);
[[nodiscard]] ASTRA_CORE_API LogLevel LogLevelFromString(std::string_view value);
[[nodiscard]] ASTRA_CORE_API nlohmann::json ToJson(const LogEvent& event);

} // namespace Astra::Core
