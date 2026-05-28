#pragma once

#include <Astra/ApplicationCore/Application.h>
#include <Astra/AssetRegistry/AssetRegistry.h>
#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>
#include <Astra/VFS/DirectoryMount.h>
#include <Astra/VNRuntimeServices/RuntimeCommand.h>

#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

namespace astra {

enum class PlatformEventType {
    Quit,
    Advance,
    Choice1,
    Choice2,
};

struct PlatformEvent {
    PlatformEventType type = PlatformEventType::Quit;
};

class IPlatformRuntime {
  public:
    virtual ~IPlatformRuntime() = default;
    [[nodiscard]] virtual bool is_initialized() const = 0;
};

class IWindow {
  public:
    virtual ~IWindow() = default;
    [[nodiscard]] virtual bool is_open() const = 0;
    [[nodiscard]] virtual void* native_handle() const = 0;
    [[nodiscard]] virtual std::optional<PlatformEvent> poll_event() = 0;
    virtual void set_title(const char* title) = 0;
};

class IRenderer2D {
  public:
    virtual ~IRenderer2D() = default;
    [[nodiscard]] virtual bool available() const = 0;
    virtual void render(const RuntimeRenderSnapshot& snapshot) = 0;
};

class IAudioRuntime {
  public:
    virtual ~IAudioRuntime() = default;
    [[nodiscard]] virtual bool available() const = 0;
    virtual void play_sound(const std::filesystem::path& path) = 0;
    virtual void stop_all() = 0;
};

struct ProjectContentLoadResult {
    std::filesystem::path project_root;
    std::filesystem::path content_root;
    std::filesystem::path entry_script;
};

class IPlatformProvider {
  public:
    virtual ~IPlatformProvider() = default;
    [[nodiscard]] virtual std::string id() const = 0;
    [[nodiscard]] virtual std::unique_ptr<IPlatformRuntime>
    create_platform(DiagnosticSink& diagnostics) const = 0;
    [[nodiscard]] virtual std::unique_ptr<IWindow>
    create_window(IPlatformRuntime& platform, const ApplicationConfig& config,
                  DiagnosticSink& diagnostics) const = 0;
};

class IRendererProvider {
  public:
    virtual ~IRendererProvider() = default;
    [[nodiscard]] virtual std::string id() const = 0;
    [[nodiscard]] virtual std::unique_ptr<IRenderer2D>
    create_renderer(IWindow& window, DiagnosticSink& diagnostics) const = 0;
};

class IAudioProvider {
  public:
    virtual ~IAudioProvider() = default;
    [[nodiscard]] virtual std::string id() const = 0;
    [[nodiscard]] virtual std::unique_ptr<IAudioRuntime>
    create_audio(DiagnosticSink& diagnostics) const = 0;
};

class IProjectContentProvider {
  public:
    virtual ~IProjectContentProvider() = default;
    [[nodiscard]] virtual std::string id() const = 0;
    [[nodiscard]] virtual Expected<ProjectContentLoadResult>
    load_project_content(const std::filesystem::path& project_root, Vfs& vfs,
                         AssetRegistry& asset_registry, DiagnosticSink& diagnostics) const = 0;
};

class RuntimeProviderRegistry {
  public:
    VoidResult register_platform_provider(std::shared_ptr<IPlatformProvider> provider,
                                          DiagnosticSink& diagnostics);
    VoidResult register_renderer_provider(std::shared_ptr<IRendererProvider> provider,
                                          DiagnosticSink& diagnostics);
    VoidResult register_audio_provider(std::shared_ptr<IAudioProvider> provider,
                                       DiagnosticSink& diagnostics);
    VoidResult register_project_content_provider(std::shared_ptr<IProjectContentProvider> provider,
                                                 DiagnosticSink& diagnostics);

    [[nodiscard]] Expected<std::shared_ptr<IPlatformProvider>>
    platform_provider(std::string_view id = {}) const;
    [[nodiscard]] Expected<std::shared_ptr<IRendererProvider>>
    renderer_provider(std::string_view id = {}) const;
    [[nodiscard]] Expected<std::shared_ptr<IAudioProvider>>
    audio_provider(std::string_view id = {}) const;
    [[nodiscard]] Expected<std::shared_ptr<IProjectContentProvider>>
    project_content_provider(std::string_view id = {}) const;

    [[nodiscard]] std::vector<std::string> provider_ids() const;
    void clear();

  private:
    template <typename Provider>
    VoidResult register_provider(std::unordered_map<std::string, std::shared_ptr<Provider>>& map,
                                 std::shared_ptr<Provider> provider, std::string_view kind,
                                 DiagnosticSink& diagnostics);

    template <typename Provider>
    Expected<std::shared_ptr<Provider>>
    find_provider(const std::unordered_map<std::string, std::shared_ptr<Provider>>& map,
                  std::string_view kind, std::string_view id) const;

    std::unordered_map<std::string, std::shared_ptr<IPlatformProvider>> platform_providers_;
    std::unordered_map<std::string, std::shared_ptr<IRendererProvider>> renderer_providers_;
    std::unordered_map<std::string, std::shared_ptr<IAudioProvider>> audio_providers_;
    std::unordered_map<std::string, std::shared_ptr<IProjectContentProvider>>
        project_content_providers_;
};

VoidResult register_default_project_content_plugin(RuntimeProviderRegistry& registry,
                                                   DiagnosticSink& diagnostics);

} // namespace astra
