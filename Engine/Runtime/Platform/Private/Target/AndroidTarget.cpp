#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc AndroidTarget() {
    return {"android-arm64", TargetOsFamily::Android, TargetArchitecture::Arm64, "astra", ".so", "android-arm64", "", "", false, {false, false, true, false, true, false, false, true, false}};
}

} // namespace Astra::Platform
