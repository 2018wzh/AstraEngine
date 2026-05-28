#pragma once

#include <Astra/Bootstrap/RuntimeProviderRegistry.h>

namespace astra {

void register_sdl_platform_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics);

} // namespace astra
