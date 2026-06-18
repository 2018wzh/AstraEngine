#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc IosTarget() {
    return {"ios-arm64", TargetOsFamily::IOS, TargetArchitecture::Arm64, "astra", "", "ios-arm64", "", "", false, {false, false, true, false, true, false, false, true, false}};
}

} // namespace Astra::Platform
