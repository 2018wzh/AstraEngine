#pragma once

#include <Astra/Bootstrap/RuntimeProviderRegistry.h>

namespace astra {

void register_miniaudio_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics);

} // namespace astra
