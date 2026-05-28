#pragma once

#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/Core/Diagnostics.h>
#include <Astra/VNRuntimeServices/RuntimeCommand.h>

namespace astra {

class SdlGpuRenderer {
public:
    SdlGpuRenderer(void* native_window, DiagnosticSink& diagnostics);
    ~SdlGpuRenderer();

    SdlGpuRenderer(const SdlGpuRenderer&) = delete;
    SdlGpuRenderer& operator=(const SdlGpuRenderer&) = delete;

    [[nodiscard]] bool available() const;
    void render(const RuntimeRenderSnapshot& snapshot);

  private:
    void* renderer_ = nullptr;
    DiagnosticSink& diagnostics_;
};

void register_sdl_gpu_renderer_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics);

} // namespace astra
