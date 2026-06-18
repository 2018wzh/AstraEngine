#include "TargetPlatformInternal.hpp"

namespace Astra::Platform {

TargetPlatformDesc WebTarget() {
    return {"web-wasm32", TargetOsFamily::Web, TargetArchitecture::Wasm32, "astra.html", ".wasm", "web-wasm32", "", "", false, {false, false, true, false, false, false, false, true, false}};
}

} // namespace Astra::Platform
