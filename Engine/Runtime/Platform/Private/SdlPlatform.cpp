#include <Astra/Platform/Platform.hpp>

#include <SDL3/SDL.h>

namespace Astra::Platform {

namespace {

class SdlWindowService final : public IWindowService {
public:
    ~SdlWindowService() override {
        Close();
    }

    Astra::Core::Result<void> Create(WindowDesc desc, Astra::Core::DiagnosticSink& diagnostics) override {
        if (!SDL_Init(SDL_INIT_VIDEO | SDL_INIT_EVENTS)) {
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_PLATFORM_SDL_INIT";
            diagnostic.category = "platform.sdl";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = SDL_GetError();
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InternalError, SDL_GetError());
        }

        window_ = SDL_CreateWindow(desc.title.c_str(), static_cast<int>(desc.width), static_cast<int>(desc.height), 0);
        if (window_ == nullptr) {
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_PLATFORM_SDL_WINDOW";
            diagnostic.category = "platform.sdl";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = SDL_GetError();
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InternalError, SDL_GetError());
        }
        return Astra::Core::Result<void>::Success();
    }

    void PumpEvents() override {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_EVENT_QUIT) {
                close_requested_ = true;
            }
        }
    }

    bool ShouldClose() const override { return close_requested_; }

    void Close() override {
        if (window_ != nullptr) {
            SDL_DestroyWindow(window_);
            window_ = nullptr;
        }
        SDL_QuitSubSystem(SDL_INIT_VIDEO | SDL_INIT_EVENTS);
    }

private:
    SDL_Window* window_ = nullptr;
    bool close_requested_ = false;
};

} // namespace

Astra::Core::Result<PlatformServices> CreateSdlPlatform(Astra::Core::DiagnosticSink&) {
    PlatformServices services = CreateHeadlessPlatform();
    services.impl_->kind = BackendKind::Sdl;
    services.impl_->window = std::make_unique<SdlWindowService>();
    return Astra::Core::Result<PlatformServices>::Success(std::move(services));
}

} // namespace Astra::Platform
