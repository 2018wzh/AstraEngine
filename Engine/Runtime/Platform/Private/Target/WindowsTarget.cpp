#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc WindowsTarget() {
    return {"win64", TargetOsFamily::Windows, TargetArchitecture::X64, "astra.exe", ".dll", "win64", "Play", ".bat", false, {true, true, true, true, true, true, true, true, true}};
}

} // namespace Astra::Platform
