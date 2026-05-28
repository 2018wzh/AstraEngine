#include <Astra/Renderer2D/SdlGpuRenderer.h>

#include <SDL3/SDL.h>

#include <algorithm>
#include <cstdint>
#include <functional>

namespace astra {

namespace {

struct Color {
    std::uint8_t r = 0;
    std::uint8_t g = 0;
    std::uint8_t b = 0;
    std::uint8_t a = 255;
};

Color color_from_id(std::string_view id, Color fallback) {
    if (id.empty()) {
        return fallback;
    }
    const auto hash = std::hash<std::string_view>{}(id);
    return Color{static_cast<std::uint8_t>(64 + (hash & 0x7f)),
                 static_cast<std::uint8_t>(64 + ((hash >> 8) & 0x7f)),
                 static_cast<std::uint8_t>(64 + ((hash >> 16) & 0x7f)), 255};
}

void fill_rect(SDL_Renderer* renderer, const SDL_FRect& rect, Color color) {
    SDL_SetRenderDrawColor(renderer, color.r, color.g, color.b, color.a);
    SDL_RenderFillRect(renderer, &rect);
}

} // namespace

SdlGpuRenderer::SdlGpuRenderer(void* native_window, DiagnosticSink& diagnostics)
    : diagnostics_(diagnostics) {
    renderer_ = SDL_CreateGPURenderer(nullptr, static_cast<SDL_Window*>(native_window));
    if (renderer_ == nullptr) {
        diagnostics_.error("renderer.sdl_gpu_create_failed", SDL_GetError());
    }
}

SdlGpuRenderer::~SdlGpuRenderer() {
    if (renderer_ != nullptr) {
        SDL_DestroyRenderer(static_cast<SDL_Renderer*>(renderer_));
        renderer_ = nullptr;
    }
}

bool SdlGpuRenderer::available() const {
    return renderer_ != nullptr;
}

void SdlGpuRenderer::render(const RuntimeRenderSnapshot& snapshot) {
    if (renderer_ == nullptr) {
        return;
    }

    auto* renderer = static_cast<SDL_Renderer*>(renderer_);
    const Color background = color_from_id(snapshot.background_asset, Color{28, 34, 45, 255});
    SDL_SetRenderDrawColor(renderer, background.r, background.g, background.b, 255);
    SDL_RenderClear(renderer);

    fill_rect(renderer, SDL_FRect{0.0f, 0.0f, 1280.0f, 720.0f},
              Color{background.r, background.g, background.b, 255});

    for (const RenderSprite& sprite : snapshot.sprites) {
        float x = 490.0f;
        if (sprite.position == StagePosition::Left) {
            x = 240.0f;
        } else if (sprite.position == StagePosition::Right) {
            x = 740.0f;
        }
        const Color color = color_from_id(sprite.asset_id, Color{175, 130, 170, 255});
        fill_rect(renderer, SDL_FRect{x, 120.0f, 300.0f, 430.0f}, color);
        fill_rect(renderer, SDL_FRect{x + 75.0f, 55.0f, 150.0f, 150.0f},
                  Color{std::uint8_t(std::min(255, int(color.r) + 30)),
                        std::uint8_t(std::min(255, int(color.g) + 30)),
                        std::uint8_t(std::min(255, int(color.b) + 30)), 255});
    }

    fill_rect(renderer, SDL_FRect{80.0f, 520.0f, 1120.0f, 150.0f}, Color{18, 20, 24, 230});
    SDL_SetRenderDrawColor(renderer, 235, 235, 225, 255);
    SDL_RenderDebugText(renderer, 105.0f, 545.0f, snapshot.speaker.c_str());
    SDL_RenderDebugText(renderer, 105.0f, 575.0f, snapshot.dialogue.c_str());

    float y = 430.0f;
    for (std::size_t i = 0; i < snapshot.choices.size(); ++i) {
        fill_rect(renderer, SDL_FRect{850.0f, y, 320.0f, 34.0f}, Color{42, 62, 76, 255});
        SDL_SetRenderDrawColor(renderer, 245, 245, 245, 255);
        const std::string line = std::to_string(i + 1) + ". " + snapshot.choices[i];
        SDL_RenderDebugText(renderer, 865.0f, y + 9.0f, line.c_str());
        y += 44.0f;
    }

    SDL_RenderPresent(renderer);
}

} // namespace astra
