#pragma once

#include <Astra/Core/Types.hpp>

#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace Astra::Core {

struct ProfilingMarker {
    std::string name;
    std::string category;
    u64 begin_ns = 0;
    u64 end_ns = 0;
    std::unordered_map<std::string, std::string> fields;
};

class ProfilingCapture {
public:
    void Begin(std::string name, std::string category, u64 timestamp_ns);
    void End(std::string_view name, u64 timestamp_ns);
    [[nodiscard]] const std::vector<ProfilingMarker>& Markers() const;
    void Clear();

private:
    std::vector<ProfilingMarker> markers_;
};

} // namespace Astra::Core
