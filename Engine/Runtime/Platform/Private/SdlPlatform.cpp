#include <Astra/Platform/Platform.hpp>

#include <SDL3/SDL.h>

#include <algorithm>
#include <sstream>

namespace Astra::Platform {

namespace {

std::string WindowFrameHash(const WindowFrameDesc& frame) {
    constexpr Astra::Core::u64 offset = 14695981039346656037ull;
    constexpr Astra::Core::u64 prime = 1099511628211ull;
    Astra::Core::u64 value = offset;
    auto mix = [&](std::string_view text) {
        for (const auto character : text) {
            value ^= static_cast<unsigned char>(character);
            value *= prime;
        }
    };
    mix(std::to_string(frame.frame_index));
    mix(std::to_string(frame.width));
    mix(std::to_string(frame.height));
    for (const auto& primitive : frame.primitives) {
        mix(primitive.id);
        mix(primitive.kind);
        mix(std::to_string(primitive.x));
        mix(std::to_string(primitive.y));
        mix(std::to_string(primitive.width));
        mix(std::to_string(primitive.height));
        mix(primitive.label);
        mix(std::to_string(primitive.image_width));
        mix(std::to_string(primitive.image_height));
        for (const auto byte : primitive.image_rgba) {
            value ^= byte;
            value *= prime;
        }
    }
    std::ostringstream output;
    output << std::hex << value;
    return output.str();
}

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
        binding_ = {1, "sdl3", desc.width, desc.height};
        renderer_ = SDL_CreateRenderer(window_, nullptr);
        if (renderer_ == nullptr) {
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_PLATFORM_SDL_RENDERER";
            diagnostic.category = "platform.sdl";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = SDL_GetError();
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InternalError, SDL_GetError());
        }
        return Astra::Core::Result<void>::Success();
    }

    Astra::Core::Result<WindowPresentEvidence> PresentFrame(const WindowFrameDesc& frame, Astra::Core::DiagnosticSink& diagnostics) override {
        if (window_ == nullptr || renderer_ == nullptr) {
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_PLATFORM_SDL_WINDOW";
            diagnostic.category = "platform.sdl";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = "SDL window must be created before presenting a frame.";
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<WindowPresentEvidence>::Failure(Astra::Core::ErrorCode::InvalidArgument, "window not created");
        }

        SDL_SetRenderDrawColor(renderer_, frame.clear_color.r, frame.clear_color.g, frame.clear_color.b, frame.clear_color.a);
        SDL_RenderClear(renderer_);
        for (const auto& primitive : frame.primitives) {
            SDL_FRect rect{primitive.x, primitive.y, primitive.width, primitive.height};
            if (!primitive.image_rgba.empty() && primitive.image_width > 0 && primitive.image_height > 0) {
                SDL_Texture* texture = SDL_CreateTexture(
                    renderer_,
                    SDL_PIXELFORMAT_RGBA32,
                    SDL_TEXTUREACCESS_STATIC,
                    static_cast<int>(primitive.image_width),
                    static_cast<int>(primitive.image_height));
                if (texture != nullptr) {
                    SDL_SetTextureBlendMode(texture, SDL_BLENDMODE_BLEND);
                    SDL_UpdateTexture(texture, nullptr, primitive.image_rgba.data(), static_cast<int>(primitive.image_width * 4));
                    SDL_RenderTexture(renderer_, texture, nullptr, &rect);
                    SDL_DestroyTexture(texture);
                    continue;
                }
                Astra::Core::Diagnostic diagnostic;
                diagnostic.code = "ASTRA_PLATFORM_SDL_TEXTURE";
                diagnostic.category = "platform.sdl";
                diagnostic.severity = Astra::Core::DiagnosticSeverity::Warning;
                diagnostic.message = SDL_GetError();
                diagnostics.Emit(std::move(diagnostic));
            }
            SDL_SetRenderDrawColor(renderer_, primitive.color.r, primitive.color.g, primitive.color.b, primitive.color.a);
            SDL_RenderFillRect(renderer_, &rect);
        }
        SDL_RenderPresent(renderer_);

        WindowPresentEvidence evidence;
        evidence.presented = true;
        evidence.backend = "sdl3";
        evidence.frame_index = frame.frame_index;
        evidence.primitive_count = static_cast<Astra::Core::u32>(frame.primitives.size());
        evidence.image_primitive_count = static_cast<Astra::Core::u32>(std::ranges::count_if(frame.primitives, [](const WindowFramePrimitive& primitive) {
            return !primitive.image_rgba.empty();
        }));
        evidence.frame_hash = WindowFrameHash(frame);
        return Astra::Core::Result<WindowPresentEvidence>::Success(std::move(evidence));
    }

    WindowGraphicsBinding GraphicsBinding() const override { return binding_; }

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
        if (renderer_ != nullptr) {
            SDL_DestroyRenderer(renderer_);
            renderer_ = nullptr;
        }
        if (window_ != nullptr) {
            SDL_DestroyWindow(window_);
            window_ = nullptr;
        }
        binding_ = {};
        SDL_QuitSubSystem(SDL_INIT_VIDEO | SDL_INIT_EVENTS);
    }

private:
    SDL_Window* window_ = nullptr;
    SDL_Renderer* renderer_ = nullptr;
    WindowGraphicsBinding binding_;
    bool close_requested_ = false;
};

} // namespace

std::unique_ptr<IWindowService> CreateSdlWindowService() {
    return std::make_unique<SdlWindowService>();
}

} // namespace Astra::Platform
