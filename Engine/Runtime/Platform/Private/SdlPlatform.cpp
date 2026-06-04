#include <Astra/Platform/SdlPlatform.h>

#include <Astra/Core/Log.h>

#include <SDL3/SDL.h>

#include <cstdlib>
#include <cstring>
#include <thread>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#endif

namespace astra {

namespace {

std::filesystem::path env_path(const char* name) {
    char* value = nullptr;
    std::size_t length = 0;
#if defined(_WIN32)
    if (_dupenv_s(&value, &length, name) != 0) {
        value = nullptr;
    }
#else
    value = std::getenv(name);
    length = value == nullptr ? 0 : std::strlen(value);
#endif
    if (value == nullptr || length == 0 || value[0] == '\0') {
#if defined(_WIN32)
        free(value);
#endif
        return {};
    }
    const auto path = std::filesystem::path(value);
#if defined(_WIN32)
    free(value);
#endif
    return path;
}

std::filesystem::path fallback_user_directory() {
#if defined(_WIN32)
    if (auto user = env_path("USERPROFILE"); !user.empty()) {
        return user;
    }
#else
    if (auto home = env_path("HOME"); !home.empty()) {
        return home;
    }
#endif
    return std::filesystem::current_path();
}

class SdlWindow final : public IWindow {
  public:
    explicit SdlWindow(SDL_Window* window) : window_(window) {}
    ~SdlWindow() override {
        if (window_ != nullptr) {
            SDL_DestroyWindow(window_);
        }
    }

    [[nodiscard]] bool is_open() const override {
        return open_;
    }

    [[nodiscard]] std::optional<PlatformEvent> poll_event() override {
        if (window_ == nullptr) {
            return std::nullopt;
        }
        SDL_Event event{};
        if (!SDL_PollEvent(&event)) {
            return std::nullopt;
        }
        if (event.type == SDL_EVENT_QUIT ||
            (event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED &&
             event.window.windowID == SDL_GetWindowID(window_))) {
            open_ = false;
            return PlatformEvent{PlatformEventType::Quit};
        }
        return std::nullopt;
    }

    void close() override {
        open_ = false;
    }

  private:
    SDL_Window* window_ = nullptr;
    bool open_ = true;
};

class SdlWindowService final : public IWindowService {
  public:
    SdlWindowService() {
        video_initialized_ = SDL_InitSubSystem(SDL_INIT_VIDEO);
        if (!video_initialized_) {
            log::error(log::LogPlatform, "SDL video init failed: {}", SDL_GetError());
        }
    }

    ~SdlWindowService() override {
        if (video_initialized_) {
            SDL_QuitSubSystem(SDL_INIT_VIDEO);
        }
    }

    [[nodiscard]] Expected<std::unique_ptr<IWindow>>
    create_window(const WindowCreateDesc& desc, DiagnosticSink& diagnostics) override {
        if (!video_initialized_) {
            diagnostics.error("platform.video_unavailable", "SDL video subsystem is unavailable");
            return std::unexpected(
                make_error("platform.video_unavailable", "SDL video subsystem is unavailable"));
        }
        SDL_WindowFlags flags = SDL_WINDOW_RESIZABLE;
        if (desc.hidden) {
            flags |= SDL_WINDOW_HIDDEN;
        }
        SDL_Window* window =
            SDL_CreateWindow(desc.title.c_str(), desc.width, desc.height, flags);
        if (window == nullptr) {
            diagnostics.error("platform.window_create", SDL_GetError());
            return std::unexpected(make_error("platform.window_create", SDL_GetError()));
        }
        return std::make_unique<SdlWindow>(window);
    }

  private:
    bool video_initialized_ = false;
};

class SdlInputService final : public IInputService {
  public:
    [[nodiscard]] std::string backend_name() const override {
        return "SDL3";
    }
};

class SdlFileSystemService final : public IFileSystemService {
  public:
    [[nodiscard]] bool exists(const std::filesystem::path& path) const override {
        return std::filesystem::exists(path);
    }

    [[nodiscard]] std::filesystem::path current_path() const override {
        return std::filesystem::current_path();
    }

    [[nodiscard]] std::filesystem::path user_directory() const override {
        return fallback_user_directory();
    }

    [[nodiscard]] std::filesystem::path save_directory() const override {
#if defined(_WIN32)
        if (auto app_data = env_path("APPDATA"); !app_data.empty()) {
            return app_data / "AstraEngine" / "Saved";
        }
#endif
        return user_directory() / ".astraengine" / "Saved";
    }

    [[nodiscard]] std::filesystem::path cache_directory() const override {
#if defined(_WIN32)
        if (auto local_app_data = env_path("LOCALAPPDATA"); !local_app_data.empty()) {
            return local_app_data / "AstraEngine" / "Cache";
        }
#endif
        return user_directory() / ".cache" / "AstraEngine";
    }
};

class SdlTimerService final : public ITimerService {
  public:
    [[nodiscard]] double now_seconds() const override {
        const auto counter = static_cast<double>(SDL_GetPerformanceCounter());
        const auto frequency = static_cast<double>(SDL_GetPerformanceFrequency());
        return frequency == 0.0 ? 0.0 : counter / frequency;
    }
};

class DefaultThreadService final : public IThreadService {
  public:
    [[nodiscard]] unsigned int hardware_concurrency() const override {
        const auto value = std::thread::hardware_concurrency();
        return value == 0 ? 1u : value;
    }
};

class NativeDynamicLibrary final : public IDynamicLibrary {
  public:
    explicit NativeDynamicLibrary(void* handle) : handle_(handle) {}
    ~NativeDynamicLibrary() override {
        if (handle_ == nullptr) {
            return;
        }
#if defined(_WIN32)
        FreeLibrary(static_cast<HMODULE>(handle_));
#else
        dlclose(handle_);
#endif
    }

    [[nodiscard]] bool is_loaded() const override {
        return handle_ != nullptr;
    }

    [[nodiscard]] void* symbol(const char* name) const override {
#if defined(_WIN32)
        return reinterpret_cast<void*>(GetProcAddress(static_cast<HMODULE>(handle_), name));
#else
        return dlsym(handle_, name);
#endif
    }

  private:
    void* handle_ = nullptr;
};

class NativeDynamicLibraryService final : public IDynamicLibraryService {
  public:
    [[nodiscard]] Expected<std::unique_ptr<IDynamicLibrary>>
    open_library(const std::filesystem::path& path, DiagnosticSink& diagnostics) const override {
#if defined(_WIN32)
        HMODULE handle = LoadLibraryW(path.wstring().c_str());
        if (handle == nullptr) {
            diagnostics.error("platform.library_open", "LoadLibrary failed");
            return std::unexpected(make_error("platform.library_open", "LoadLibrary failed"));
        }
        return std::make_unique<NativeDynamicLibrary>(reinterpret_cast<void*>(handle));
#else
        void* handle = dlopen(path.string().c_str(), RTLD_NOW);
        if (handle == nullptr) {
            diagnostics.error("platform.library_open", dlerror());
            return std::unexpected(make_error("platform.library_open", dlerror()));
        }
        return std::make_unique<NativeDynamicLibrary>(handle);
#endif
    }
};

} // namespace

Expected<PlatformServicesBundle> create_sdl_platform_services(DiagnosticSink& diagnostics) {
    if (!SDL_Init(SDL_INIT_EVENTS)) {
        diagnostics.error("platform.sdl_events_init", SDL_GetError());
        return std::unexpected(
            make_error("platform.sdl_events_init", "SDL event subsystem initialization failed"));
    }

    PlatformServicesBundle bundle;
    bundle.window = std::make_shared<SdlWindowService>();
    bundle.input = std::make_shared<SdlInputService>();
    bundle.filesystem = std::make_shared<SdlFileSystemService>();
    bundle.timer = std::make_shared<SdlTimerService>();
    bundle.thread = std::make_shared<DefaultThreadService>();
    bundle.dynamic_library = std::make_shared<NativeDynamicLibraryService>();
    return bundle;
}

} // namespace astra
