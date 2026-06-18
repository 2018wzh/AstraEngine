#include <Astra/Platform/Platform.hpp>

#include "TargetPlatformInternal.hpp"

#include <algorithm>

namespace Astra::Platform {
namespace {

std::string HostTargetId() {
#if defined(_WIN32)
    return "win64";
#elif defined(__APPLE__)
#if defined(__aarch64__) || defined(__arm64__)
    return "macos-arm64";
#else
    return "macos-x64";
#endif
#else
    return "linux-x64";
#endif
}

std::vector<TargetPlatformDesc> TargetTable() {
    const auto host = HostTargetId();
    std::vector<TargetPlatformDesc> targets = {
        WindowsTarget(),
        LinuxTarget(),
        MacOsX64Target(),
        MacOsArm64Target(),
        IosTarget(),
        AndroidTarget(),
        WebTarget(),
        HeadlessTarget(),
    };
    for (auto& target : targets) {
        target.is_host = target.id == host;
    }
    return targets;
}

} // namespace

TargetPlatformDesc CurrentHostTargetPlatform() {
    auto result = FindTargetPlatform(HostTargetId());
    return result ? result.Value() : TargetPlatformDesc{};
}

Astra::Core::Result<TargetPlatformDesc> FindTargetPlatform(std::string_view id) {
    const auto targets = TargetTable();
    const auto it = std::ranges::find_if(targets, [id](const TargetPlatformDesc& target) {
        return target.id == id;
    });
    if (it == targets.end()) {
        return Astra::Core::Result<TargetPlatformDesc>::Failure(Astra::Core::ErrorCode::NotFound, "target platform not found");
    }
    return Astra::Core::Result<TargetPlatformDesc>::Success(*it);
}

std::vector<TargetPlatformDesc> KnownTargetPlatforms() {
    return TargetTable();
}

} // namespace Astra::Platform
