#pragma once

#include <Astra/ApplicationCore/Application.h>
#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/Core/Result.h>

#include <memory>
#include <optional>

namespace astra {

class SdlPlatform {
public:
    SdlPlatform();
    ~SdlPlatform();

    SdlPlatform(const SdlPlatform&) = delete;
    SdlPlatform& operator=(const SdlPlatform&) = delete;

    [[nodiscard]] bool is_initialized() const;

  private:
    bool initialized_ = false;
};

class SdlWindow {
  public:
    explicit SdlWindow(const ApplicationConfig& config);
    ~SdlWindow();

    SdlWindow(const SdlWindow&) = delete;
    SdlWindow& operator=(const SdlWindow&) = delete;

    [[nodiscard]] bool is_open() const;
    [[nodiscard]] void* native_window() const;
    [[nodiscard]] std::optional<PlatformEvent> poll_event();
    void set_title(const char* title);

  private:
    void* window_ = nullptr;
    bool open_ = false;
};

void register_sdl_platform_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics);

} // namespace astra
