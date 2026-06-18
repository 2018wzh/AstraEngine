#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc LinuxTarget() {
    return {"linux-x64", TargetOsFamily::Linux, TargetArchitecture::X64, "astra", ".so", "linux-x64", "play-", ".sh", false, {true, true, true, true, true, true, true, true, true}};
}

} // namespace Astra::Platform
