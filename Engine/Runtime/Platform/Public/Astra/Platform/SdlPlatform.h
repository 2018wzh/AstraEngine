#pragma once

#include <Astra/Platform/PlatformServices.h>

namespace astra {

Expected<PlatformServicesBundle> create_sdl_platform_services(DiagnosticSink& diagnostics);

} // namespace astra
