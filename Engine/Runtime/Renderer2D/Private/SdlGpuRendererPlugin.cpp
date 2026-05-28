#include <Astra/Renderer2D/SdlGpuRendererPlugin.h>

#include <Astra/Renderer2D/SdlGpuRenderer.h>

namespace astra {

namespace {

class SdlGpuRendererRuntime final : public IRenderer2D {
public:
    SdlGpuRendererRuntime(IWindow& window, DiagnosticSink& diagnostics)
        : renderer_(window.native_handle(), diagnostics) {}

    [[nodiscard]] bool available() const override {
        return renderer_.available();
    }

    void render(const RuntimeRenderSnapshot& snapshot) override {
        renderer_.render(snapshot);
    }

private:
    SdlGpuRenderer renderer_;
};

class SdlGpuRendererProvider final : public IRendererProvider {
public:
    [[nodiscard]] std::string id() const override {
        return "astra.renderer.sdl_gpu";
    }

    [[nodiscard]] std::unique_ptr<IRenderer2D>
    create_renderer(IWindow& window, DiagnosticSink& diagnostics) const override {
        auto renderer = std::make_unique<SdlGpuRendererRuntime>(window, diagnostics);
        if (!renderer->available()) {
            diagnostics.error("renderer.unavailable", "SDL GPU renderer provider failed");
            return nullptr;
        }
        return renderer;
    }
};

} // namespace

void register_sdl_gpu_renderer_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics) {
    (void)registry.register_renderer_provider(std::make_shared<SdlGpuRendererProvider>(), diagnostics);
}

} // namespace astra
