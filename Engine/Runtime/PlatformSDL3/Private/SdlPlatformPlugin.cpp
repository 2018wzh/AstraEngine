#include <Astra/PlatformSDL3/SdlPlatformPlugin.h>

#include <Astra/PlatformSDL3/SdlPlatform.h>

namespace astra {

namespace {

class SdlPlatformRuntime final : public IPlatformRuntime {
public:
    explicit SdlPlatformRuntime(DiagnosticSink&) : platform_() {}

    [[nodiscard]] bool is_initialized() const override {
        return platform_.is_initialized();
    }

private:
    SdlPlatform platform_;
};

class SdlWindowRuntime final : public IWindow {
public:
    explicit SdlWindowRuntime(const ApplicationConfig& config) : window_(config) {}

    [[nodiscard]] bool is_open() const override {
        return window_.is_open();
    }

    [[nodiscard]] void* native_handle() const override {
        return window_.native_window();
    }

    [[nodiscard]] std::optional<PlatformEvent> poll_event() override {
        return window_.poll_event();
    }

    void set_title(const char* title) override {
        window_.set_title(title);
    }

private:
    SdlWindow window_;
};

class SdlPlatformProvider final : public IPlatformProvider {
public:
    [[nodiscard]] std::string id() const override {
        return "astra.platform.sdl3";
    }

    [[nodiscard]] std::unique_ptr<IPlatformRuntime>
    create_platform(DiagnosticSink& diagnostics) const override {
        return std::make_unique<SdlPlatformRuntime>(diagnostics);
    }

    [[nodiscard]] std::unique_ptr<IWindow>
    create_window(IPlatformRuntime& platform,
                  const ApplicationConfig& config,
                  DiagnosticSink& diagnostics) const override {
        if (!platform.is_initialized()) {
            diagnostics.error("platform.not_initialized", "Cannot create SDL window before SDL init");
            return nullptr;
        }
        auto window = std::make_unique<SdlWindowRuntime>(config);
        if (!window->is_open()) {
            diagnostics.error("platform.window_failed", "SDL window creation failed");
            return nullptr;
        }
        return window;
    }
};

} // namespace

void register_sdl_platform_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics) {
    (void)registry.register_platform_provider(std::make_shared<SdlPlatformProvider>(), diagnostics);
}

} // namespace astra
