#include <Astra/Core/Profiling.hpp>

#include <algorithm>
#include <ranges>
#include <utility>

namespace Astra::Core {

void ProfilingCapture::Begin(std::string name, std::string category, u64 timestamp_ns) {
    markers_.push_back({std::move(name), std::move(category), timestamp_ns, 0, {}});
}

void ProfilingCapture::End(std::string_view name, u64 timestamp_ns) {
    for (auto& marker : markers_ | std::views::reverse) {
        if (marker.name == name && marker.end_ns == 0) {
            marker.end_ns = timestamp_ns;
            return;
        }
    }
}

const std::vector<ProfilingMarker>& ProfilingCapture::Markers() const {
    return markers_;
}

void ProfilingCapture::Clear() {
    markers_.clear();
}

} // namespace Astra::Core
