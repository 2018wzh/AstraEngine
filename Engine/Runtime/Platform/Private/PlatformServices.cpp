#include <Astra/Platform/PlatformServices.h>

#include <Astra/Platform/SdlPlatform.h>

namespace astra {

Expected<PlatformServicesBundle> create_default_platform_services(DiagnosticSink& diagnostics) {
    return create_sdl_platform_services(diagnostics);
}

} // namespace astra
