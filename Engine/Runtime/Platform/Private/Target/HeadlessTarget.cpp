#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc HeadlessTarget() {
    return {"headless", TargetOsFamily::Unknown, TargetArchitecture::Unknown, "", "", "headless", "", "", false, {true, false, true, true, true, true, true, true, true}};
}

} // namespace Astra::Platform
