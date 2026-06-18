#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc MacOsX64Target() {
    return {"macos-x64", TargetOsFamily::MacOS, TargetArchitecture::X64, "astra", ".dylib", "macos-x64", "play-", ".command", false, {true, true, true, true, true, true, true, true, true}};
}

TargetPlatformDesc MacOsArm64Target() {
    return {"macos-arm64", TargetOsFamily::MacOS, TargetArchitecture::Arm64, "astra", ".dylib", "macos-arm64", "play-", ".command", false, {true, true, true, true, true, true, true, true, true}};
}

} // namespace Astra::Platform
