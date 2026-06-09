#include <Astra/Core/Logging.hpp>

#include <fstream>
#include <iostream>
#include <filesystem>

namespace Astra::Core {

void MemoryLogSink::Write(LogEvent event) {
    events_.push_back(std::move(event));
}

const std::vector<LogEvent>& MemoryLogSink::Events() const {
    return events_;
}

void ConsoleLogSink::Write(const LogEvent& event) const {
    std::cout << "[" << ToString(event.severity) << "] " << event.channel << ": " << event.message << '\n';
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

nlohmann::json ToJson(const LogEvent& event) {
    return {
        {"time", event.monotonic_ns},
        {"channel", event.channel},
        {"severity", ToString(event.severity)},
        {"message", event.message},
        {"fields", event.fields},
    };
}

} // namespace Astra::Core
