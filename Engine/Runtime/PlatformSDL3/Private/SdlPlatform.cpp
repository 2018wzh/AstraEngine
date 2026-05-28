#include <Astra/PlatformSDL3/SdlPlatform.h>

#include <Astra/Core/Log.h>

#include <SDL3/SDL.h>

namespace astra {

SdlPlatform::SdlPlatform() {
    initialized_ = SDL_Init(SDL_INIT_VIDEO | SDL_INIT_AUDIO);
    if (!initialized_) {
        log::error(log::LogPlatform, "SDL init failed: {}", SDL_GetError());
    }
}

SdlPlatform::~SdlPlatform() {
    if (initialized_) {
        SDL_Quit();
    }
}

bool SdlPlatform::is_initialized() const {
    return initialized_;
}

SdlWindow::SdlWindow(const ApplicationConfig& config) {
    window_ = SDL_CreateWindow(config.title, config.width, config.height, SDL_WINDOW_RESIZABLE);
    open_ = window_ != nullptr;
    if (!open_) {
        log::error(log::LogPlatform, "SDL window creation failed: {}", SDL_GetError());
    }
}

SdlWindow::~SdlWindow() {
    if (window_ != nullptr) {
        SDL_DestroyWindow(static_cast<SDL_Window*>(window_));
        window_ = nullptr;
    }
}

bool SdlWindow::is_open() const {
    return open_;
}

void* SdlWindow::native_window() const {
    return window_;
}

std::optional<PlatformEvent> SdlWindow::poll_event() {
    SDL_Event event{};
    if (!SDL_PollEvent(&event)) {
        return std::nullopt;
    }

    if (event.type == SDL_EVENT_QUIT) {
        open_ = false;
        return PlatformEvent{PlatformEventType::Quit};
    }
    if (event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED &&
        event.window.windowID == SDL_GetWindowID(static_cast<SDL_Window*>(window_))) {
        open_ = false;
        return PlatformEvent{PlatformEventType::Quit};
    }
    if (event.type == SDL_EVENT_KEY_DOWN) {
        switch (event.key.key) {
        case SDLK_SPACE:
        case SDLK_RETURN:
            return PlatformEvent{PlatformEventType::Advance};
        case SDLK_1:
            return PlatformEvent{PlatformEventType::Choice1};
        case SDLK_2:
            return PlatformEvent{PlatformEventType::Choice2};
        case SDLK_ESCAPE:
            open_ = false;
            return PlatformEvent{PlatformEventType::Quit};
        default:
            break;
        }
    }
    if (event.type == SDL_EVENT_MOUSE_BUTTON_DOWN) {
        return PlatformEvent{PlatformEventType::Advance};
    }
    return std::nullopt;
}

void SdlWindow::set_title(const char* title) {
    SDL_SetWindowTitle(static_cast<SDL_Window*>(window_), title);
}

} // namespace astra
