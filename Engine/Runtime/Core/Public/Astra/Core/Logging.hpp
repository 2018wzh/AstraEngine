#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Export.hpp>
#include <nlohmann/json.hpp>

#include <string>
#include <unordered_map>
#include <vector>

namespace Astra::Core {

struct LogEvent {
    u64 monotonic_ns = 0;
    std::string channel;
    DiagnosticSeverity severity = DiagnosticSeverity::Info;
    std::string message;
    std::unordered_map<std::string, std::string> fields;
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

[[nodiscard]] ASTRA_CORE_API nlohmann::json ToJson(const LogEvent& event);

} // namespace Astra::Core
